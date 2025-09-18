// Copyright (C) 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#[cfg(test)]
mod tests;

use crate::{
    VerbosityLevel,
    error::{Error, Result},
};
use ant_service_management::{
    Error as ServiceError, NodeRegistryManager, ServiceStartupStatus, ServiceStateActions,
    ServiceStatus, UpgradeOptions, UpgradeResult, control::ServiceControl,
};
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use semver::Version;
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

const DEFAULT_PROGRESS_TIMEOUT: Duration = Duration::from_secs(14 * 60);

/// A manager for batch operations on multiple services.
/// This is similar to the `ServiceManager` but designed to handle multiple services together.
// todo: implement a channel to receive updates on the status of each service during any process.
pub struct BatchServiceManager<T: ServiceStateActions + Send> {
    /// The list of services to manage.
    services: Vec<T>,
    service_control: Box<dyn ServiceControl + Send>,
    node_registry: NodeRegistryManager,
    verbosity: VerbosityLevel,
    progress_timeout: Duration,
}

impl<T: ServiceStateActions + Send> BatchServiceManager<T> {
    pub fn new(
        services: Vec<T>,
        service_control: Box<dyn ServiceControl + Send>,
        node_registry: NodeRegistryManager,
        verbosity: VerbosityLevel,
    ) -> Self {
        BatchServiceManager {
            services,
            service_control,
            node_registry,
            verbosity,
            progress_timeout: DEFAULT_PROGRESS_TIMEOUT,
        }
    }

    pub fn set_progress_timeout(&mut self, timeout: Duration) {
        self.progress_timeout = timeout;
    }

    /// Starts all the services in the batch with a fixed interval between each start.
    ///
    /// If `startup_check` is false, the startup check will be skipped for all services and we'll return immediately
    /// after starting them. This is useful when the user wants to start the services but doesn't
    /// want to wait for them to be fully started.
    #[tracing::instrument(skip(self))]
    pub async fn start_all(&self, fixed_interval: u64, startup_check: bool) -> BatchResult {
        let batch_result = self
            .start_all_inner(fixed_interval, Default::default(), startup_check)
            .await;

        if !batch_result.errors.is_empty() {
            error!("Failed to start one or more services: {batch_result:?}");
        }

        batch_result
    }

    #[tracing::instrument(skip(self))]
    pub async fn stop_all(&self, interval: Option<u64>) -> BatchResult {
        let mut batch_result = BatchResult::default();

        for service in &self.services {
            let service_name = service.name().await.clone();

            if let Some(interval) = interval
                && service.status().await == ServiceStatus::Running
            {
                debug!(
                    "Sleeping for {interval} milliseconds before stopping service {service_name}"
                );
                std::thread::sleep(Duration::from_millis(interval));
            }

            match Self::stop(service, self.service_control.as_ref(), self.verbosity).await {
                Ok(()) => {
                    info!("Stopped service {service_name}");
                    if let Err(err) = self.node_registry.save().await {
                        error!(
                            "Failed to save node registry after stopping service {service_name}: {err}"
                        );
                    }

                    if self.verbosity != VerbosityLevel::Minimal {
                        println!("{} Service {service_name} was stopped", "✓".green());
                    }
                }
                Err(err) => {
                    error!("Failed to stop service {service_name}: {err}");

                    batch_result.insert_error(service_name, err);
                }
            }
        }

        if let Err(err) = self.node_registry.save().await {
            error!("Failed to save node registry after stopping all services: {err}");
        }

        batch_result
    }

    #[tracing::instrument(skip(self, options))]
    pub async fn upgrade_all(
        &self,
        options: UpgradeOptions,
        fixed_interval: u64,
        startup_check: bool,
    ) -> (BatchResult, HashMap<String, UpgradeResult>) {
        let mut skip_services = HashSet::new();
        let mut batch_result = BatchResult::default();
        let mut upgrade_summary: HashMap<String, UpgradeResult> = HashMap::new();

        for service in &self.services {
            let service_name = service.name().await;
            info!("Upgrading the {service_name} service");

            match Self::reinstall_service(
                service,
                self.service_control.as_ref(),
                self.verbosity,
                options.clone(),
            )
            .await
            {
                Ok(true) => {
                    info!("Service {service_name} has been reinstalled successfully.");
                }
                Ok(false) => {
                    info!(
                        "Service {service_name} is already at the target version or lower. No upgrade is required."
                    );
                    skip_services.insert(service_name.clone());
                    upgrade_summary.insert(service_name.clone(), UpgradeResult::NotRequired);
                }
                Err(err) => {
                    error!("Failed to upgrade service {service_name}: {err}");
                    batch_result.insert_error(service_name.clone(), err);
                }
            }
        }

        if options.start_service {
            let start_batch_result = self
                .start_all_inner(fixed_interval, skip_services.clone(), startup_check)
                .await;

            if !batch_result.errors.is_empty() {
                info!("All services have been started after upgrade.");
            } else {
                error!("Failed to start one or more services after upgrade: {batch_result:?}");
            }

            // Merge the start errors into the main batch result
            for (service_name, errors) in start_batch_result.errors {
                for error in errors {
                    batch_result.insert_error(service_name.clone(), error);
                }
            }
        }

        for service in &self.services {
            let service_name = service.name().await;

            // Only process services that weren't skipped during reinstall
            if skip_services.contains(&service_name) {
                // Skip services that didn't need upgrade - they already have NotRequired in summary
                continue;
            }

            let old_service_version = service.version().await;
            // Only set version for services that were actually upgraded
            service
                .set_version(&options.target_version.to_string())
                .await;

            match batch_result.get_errors(&service_name) {
                Some(err) => {
                    info!(
                        "The service {service_name} has been upgraded but could not be started: {err}"
                    );
                    upgrade_summary.insert(
                        service_name.clone(),
                        UpgradeResult::UpgradedButNotStarted(
                            old_service_version.clone(),
                            options.target_version.to_string(),
                            err.to_string(),
                        ),
                    );
                }
                None => {
                    if options.force {
                        upgrade_summary.insert(
                            service_name.clone(),
                            UpgradeResult::Forced(
                                old_service_version.clone(),
                                options.target_version.to_string(),
                            ),
                        );
                    } else {
                        upgrade_summary.insert(
                            service_name.clone(),
                            UpgradeResult::Upgraded(
                                old_service_version.clone(),
                                options.target_version.to_string(),
                            ),
                        );
                    }
                }
            }
        }

        if let Err(err) = self.node_registry.save().await {
            error!("Failed to save node registry after setting new version post upgrade: {err}");
        }

        (batch_result, upgrade_summary)
    }

    #[tracing::instrument(skip(self))]
    pub async fn remove_all(&self, keep_directories: bool) -> BatchResult {
        let mut batch_result = BatchResult::default();

        for service in &self.services {
            let service_name = service.name().await;
            info!("Removing the {service_name} service");

            if let ServiceStatus::Running = service.status().await {
                if self
                    .service_control
                    .get_process_pid(&service.bin_path().await)
                    .is_ok()
                {
                    error!("Service {service_name} is already running. Stop it before removing it",);
                    batch_result.insert_error(
                        service_name.clone(),
                        Error::ServiceAlreadyRunning(vec![service_name.clone()]),
                    );
                    continue;
                } else {
                    // If the node wasn't actually running, we should give the user an opportunity to
                    // check why it may have failed before removing everything.
                    if let Err(err) = service.on_stop().await {
                        error!("Failed to run on_stop for service {service_name}: {err}");
                        batch_result.insert_error(service_name.clone(), err.into());
                        continue;
                    }
                    error!(
                        "The service: {service_name} was marked as running but it had actually stopped. You may want to check the logs for errors before removing it. To remove the service, run the command again."
                    );

                    batch_result.insert_error(
                        service_name.clone(),
                        Error::ServiceStatusMismatch {
                            expected: ServiceStatus::Running,
                        },
                    );
                }
            }
        }

        for service in &self.services {
            let service_name = service.name().await;
            if batch_result.contains_errors(&service_name) {
                debug!("Skipping service {service_name} as it has errors");
                continue;
            }

            match self
                .service_control
                .uninstall(&service_name, service.is_user_mode().await)
            {
                Ok(()) => {
                    debug!("Service {service_name} has been uninstalled");
                }
                Err(err) => match err {
                    ServiceError::ServiceRemovedManually(name) => {
                        warn!(
                            "The user appears to have removed the {name} service manually. Skipping the error.",
                        );
                        // The user has deleted the service definition file, which the service manager
                        // crate treats as an error. We then return our own error type, which allows us
                        // to handle it here and just proceed with removing the service from the
                        // registry.
                        if self.verbosity != VerbosityLevel::Minimal {
                            println!(
                                "The user appears to have removed the {name} service manually"
                            );
                        }
                    }
                    ServiceError::ServiceDoesNotExists(name) => {
                        warn!(
                            "The service {name} has most probably been removed already, it does not exists. Skipping the error."
                        );
                    }
                    _ => {
                        error!("Error uninstalling the service: {err}");
                        batch_result.insert_error(service_name.clone(), err.into());
                        continue;
                    }
                },
            }

            if !keep_directories {
                debug!("Removing data and log directories for {service_name}");
                // It's possible the user deleted either of these directories manually.
                // We can just proceed with removing the service from the registry.
                let data_dir_path = service.data_dir_path().await;
                if data_dir_path.exists() {
                    debug!("Removing data directory {data_dir_path:?}");
                    if let Err(err) = std::fs::remove_dir_all(&data_dir_path) {
                        error!("Failed to remove data directory {data_dir_path:?}: {err}");
                        batch_result.insert_error(service_name.clone(), err.into());
                        continue;
                    }
                }
                let log_dir_path = service.log_dir_path().await;

                if log_dir_path.exists() {
                    debug!("Removing log directory {log_dir_path:?}");
                    if let Err(err) = std::fs::remove_dir_all(&log_dir_path) {
                        error!("Failed to remove log directory {log_dir_path:?}: {err}");
                        batch_result.insert_error(service_name.clone(), err.into());
                    }
                }
            }

            service.on_remove().await;
            info!("Service {service_name} has been removed successfully.");

            if self.verbosity != VerbosityLevel::Minimal {
                println!("{} Service {service_name} was removed", "✓".green());
            }

            if let Err(err) = self.node_registry.save().await {
                error!("Failed to save node registry after removing service {service_name}: {err}");
            }
        }

        if let Err(err) = self.node_registry.save().await {
            error!("Failed to save node registry after removing all services: {err}");
        }

        batch_result
    }

    #[tracing::instrument(skip(self, skip_services))]
    async fn start_all_inner(
        &self,
        fixed_interval: u64,
        skip_services: HashSet<String>,
        startup_check: bool,
    ) -> BatchResult {
        let mut batch_result = BatchResult::default();
        let mut skip_services = skip_services;

        for service in &self.services {
            let service_name = service.name().await;
            if skip_services.contains(&service_name) {
                debug!("Skipping service {service_name} as it is marked to be skipped");
                continue;
            }
            info!("Starting the {service_name} service...");

            if self.verbosity != VerbosityLevel::Minimal {
                println!("Attempting to start {service_name}...");
            }

            if ServiceStatus::Running == service.status().await {
                // The last time we checked the service was running, but it doesn't mean it's actually
                // running now. If it is running, we don't need to do anything. If it stopped because
                // of a fault, we will drop to the code below and attempt to start it again.
                // We use `get_process_pid` because it searches for the process with the service binary
                // path, and this path is unique to each service.
                if self
                    .service_control
                    .get_process_pid(&service.bin_path().await)
                    .is_ok()
                {
                    debug!("The {service_name} service is already running",);
                    if self.verbosity != VerbosityLevel::Minimal {
                        println!("The {service_name} service is already running",);
                    }
                    skip_services.insert(service_name.clone());
                    continue;
                }
            }

            match self
                .service_control
                .start(&service_name, service.is_user_mode().await)
            {
                Ok(_) => {
                    info!(
                        "Started service {service_name}, waiting for fixed interval of {fixed_interval} ms before checking if it has started"
                    );
                    // setting the status to Running here, the node could error out due to status failure though.
                    service.set_status(ServiceStatus::Running).await;
                    if let Err(err) = self.node_registry.save().await {
                        error!("Failed to save node registry after starting services: {err}");
                    }
                    self.service_control.wait(fixed_interval);
                }
                Err(err) => {
                    error!("Failed to start service {service_name}: {err}");

                    batch_result.insert_error(service_name.clone(), err.into());
                }
            };
        }

        if let Err(err) = self.node_registry.save().await {
            error!("Failed to save node registry after starting services: {err}");
        }

        let multi_progress = if self.verbosity != VerbosityLevel::Minimal {
            Some(MultiProgress::new())
        } else {
            None
        };
        let mut progress_bars = HashMap::new();

        if let Some(ref mp) = multi_progress {
            for service in &self.services {
                let service_name = service.name().await;
                if !batch_result.contains_errors(&service_name) {
                    let pb = mp.add(ProgressBar::new(100));
                    #[allow(clippy::expect_used)]
                    pb.set_style(
                        ProgressStyle::with_template(
                            "{prefix:>15} [{bar:40.cyan/blue}] {pos:>3}% {msg}",
                        )
                        .expect("Failed to create progress bar template")
                        .progress_chars("##-"),
                    );
                    pb.set_prefix(service_name.clone());
                    pb.set_message("Node starting, running reachability check...");
                    progress_bars.insert(service_name, pb);
                }
            }
        }

        let progress_start_time = std::time::Instant::now();
        let mut completed_services = HashSet::<String>::new();

        if !startup_check {
            info!("Skipping startup check as requested.");
            if self.verbosity != VerbosityLevel::Minimal {
                println!("Skipping startup check as requested.");
            }
        } else {
            info!("Waiting for all the services to start...");
            if self.verbosity != VerbosityLevel::Minimal {
                println!("Waiting for all the services to start...");
            }
        }
        loop {
            if !startup_check {
                info!("Skipping startup check as requested, breaking out of wait loop.");
                break;
            }

            if progress_start_time.elapsed() > self.progress_timeout {
                error!(
                    "Progress monitoring timed out after {:?}. Some services may not have completed their reachability check.",
                    self.progress_timeout
                );
                for service in &self.services {
                    let service_name = service.name().await;
                    if !skip_services.contains(&service_name)
                        && !batch_result.contains_errors(&service_name)
                        && !completed_services.contains(&service_name)
                    {
                        batch_result.insert_error(
                            service_name.clone(),
                            crate::error::Error::ServiceProgressTimeout {
                                service_name: service_name.clone(),
                                timeout: self.progress_timeout,
                            },
                        );
                        if let Some(pb) = progress_bars.get(&service_name) {
                            pb.finish_with_message("Timed out ⏰".red().to_string());
                        }
                    }
                }
                break;
            }

            let mut all_complete = true;

            for service in &self.services {
                let service_name = service.name().await;
                if skip_services.contains(&service_name)
                    || batch_result.contains_errors(&service_name)
                {
                    debug!(
                        "Skipping service (progress) {service_name} as it is marked to be skipped"
                    );
                    continue;
                }

                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                match service.startup_status().await {
                    ServiceStartupStatus::InProgress(progress) => {
                        info!("The reachability check progress for {service_name} is {progress}%");
                        all_complete = false;
                        if let Some(pb) = progress_bars.get(&service_name) {
                            pb.set_position(progress as u64);
                            pb.set_message("◔ Reachability Check".to_string());
                        }
                    }
                    ServiceStartupStatus::Started => {
                        info!("The reachability check for {service_name} is complete");
                        completed_services.insert(service_name.clone());
                        if let Some(pb) = progress_bars.get(&service_name) {
                            pb.finish_with_message(
                                "Node started, reachability check complete ✓"
                                    .green()
                                    .to_string(),
                            );
                        }
                    }
                    ServiceStartupStatus::Failed { reason } => {
                        error!(
                            "The reachability check / node startup failed for {service_name}: {reason}"
                        );
                        batch_result.insert_error(
                            service_name.clone(),
                            Error::ServiceStartupFailed {
                                service_name: service_name.clone(),
                                reason,
                            },
                        );
                        if let Some(pb) = progress_bars.get(&service_name) {
                            pb.finish_with_message("Failed ✗".red().to_string());
                        }
                    }
                }
            }

            if all_complete {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // Finish any remaining progress bars
        if let Some(ref _mp) = multi_progress {
            for (_service_name, pb) in progress_bars {
                if !pb.is_finished() {
                    pb.finish_with_message("Complete".to_string());
                }
            }
        }

        // Now we update the service data.
        for service in &self.services {
            let service_name = service.name().await;
            if skip_services.contains(&service_name) {
                debug!("Skipping service {service_name} as it is marked to be skipped");
                if self.verbosity != VerbosityLevel::Minimal {
                    println!("{} Service {service_name} is already running", "✓".green(),);
                    println!(
                        "  - PID: {}",
                        service
                            .pid()
                            .await
                            .map_or("-".to_string(), |p| p.to_string())
                    );
                    println!(
                        "  - Bin path: {}",
                        service.bin_path().await.to_string_lossy()
                    );
                    println!(
                        "  - Data path: {}",
                        service.data_dir_path().await.to_string_lossy()
                    );
                    println!(
                        "  - Logs path: {}",
                        service.log_dir_path().await.to_string_lossy()
                    );
                }
                continue;
            } else if batch_result.contains_errors(&service_name) {
                debug!("Service {service_name} has errors, skipping.");
                if self.verbosity != VerbosityLevel::Minimal {
                    println!("{} Failed to start {service_name} service", "✗".red());
                }
                continue;
            }

            // This is an attempt to see whether the service process has actually launched. You don't
            // always get an error from the service infrastructure.
            //
            // There might be many different `antnode` processes running, but since each service has
            // its own isolated binary, we use the binary path to uniquely identify it.
            let pid = match self
                .service_control
                .get_process_pid(&service.bin_path().await)
            {
                Ok(pid) => pid,
                Err(err) => {
                    error!("Failed to start service {service_name}: {err}");
                    batch_result.insert_error(service_name.clone(), err.into());
                    continue;
                }
            };

            let full_refresh = startup_check;
            info!(
                "Service {service_name} has started with PID {pid}, running on_start with full_refresh={full_refresh}",
            );
            match service.on_start(Some(pid), full_refresh).await {
                Ok(_) => {
                    info!("Service {service_name} has run on_start successfully");
                    if let Err(err) = self.node_registry.save().await {
                        error!(
                            "Failed to save node registry after starting service {service_name}: {err}"
                        );
                        continue;
                    }

                    if self.verbosity != VerbosityLevel::Minimal {
                        println!("{} Started {service_name} service", "✓".green(),);
                        println!(
                            "  - PID: {}",
                            service
                                .pid()
                                .await
                                .map_or("-".to_string(), |p| p.to_string())
                        );
                        println!(
                            "  - Bin path: {}",
                            service.bin_path().await.to_string_lossy()
                        );
                        println!(
                            "  - Data path: {}",
                            service.data_dir_path().await.to_string_lossy()
                        );
                        println!(
                            "  - Logs path: {}",
                            service.log_dir_path().await.to_string_lossy()
                        );
                    }
                }
                Err(err) => {
                    error!("Service {service_name} failed to run on_start: {err}");
                    if self.verbosity != VerbosityLevel::Minimal {
                        println!("{} Failed to start {service_name} service", "✗".red());
                    }
                    batch_result.insert_error(service_name.clone(), err.into());
                    continue;
                }
            }

            info!("Service {service_name} has been started successfully");
        }

        if let Err(err) = self.node_registry.save().await {
            error!("Failed to save node registry after starting services: {err}");
        }

        batch_result
    }

    #[tracing::instrument(skip(service, service_control), err)]
    async fn stop(
        service: &T,
        service_control: &dyn ServiceControl,
        verbosity: VerbosityLevel,
    ) -> crate::error::Result<()> {
        let service_name = service.name().await;
        info!("Stopping the {service_name} service");
        match service.status().await {
            ServiceStatus::Added => {
                debug!("The {service_name} service has not been started since it was installed",);
                if verbosity != VerbosityLevel::Minimal {
                    println!("Service {service_name} has not been started since it was installed",);
                }
                Ok(())
            }
            ServiceStatus::Removed => {
                debug!("The {service_name} service has been removed");
                if verbosity != VerbosityLevel::Minimal {
                    println!("Service {service_name} has been removed");
                }
                Ok(())
            }
            ServiceStatus::Running => {
                let pid = service.pid().await.ok_or(Error::PidNotSet)?;

                if service_control
                    .get_process_pid(&service.bin_path().await)
                    .is_ok()
                {
                    if verbosity != VerbosityLevel::Minimal {
                        println!("Attempting to stop {service_name}...");
                    }
                    service_control.stop(&service_name, service.is_user_mode().await)?;
                    if verbosity != VerbosityLevel::Minimal {
                        println!(
                            "{} Service {service_name} with PID {} was stopped",
                            "✓".green(),
                            pid
                        );
                    }
                } else if verbosity != VerbosityLevel::Minimal {
                    debug!("Service {service_name} was already stopped");
                    println!("{} Service {service_name} was already stopped", "✓".green());
                }

                service.on_stop().await?;
                info!("Service {service_name} has been stopped successfully.");
                Ok(())
            }
            ServiceStatus::Stopped => {
                debug!("Service {service_name} was already stopped");
                if verbosity != VerbosityLevel::Minimal {
                    println!("{} Service {service_name} was already stopped", "✓".green(),);
                }
                Ok(())
            }
        }
    }

    /// Reinstall the service with the new binary.
    /// Returns `false` if the service is already at the target version or if the target version is lower than the current version.
    #[tracing::instrument(skip(service, service_control, options), err)]
    async fn reinstall_service(
        service: &T,
        service_control: &dyn ServiceControl,
        verbosity: VerbosityLevel,
        options: UpgradeOptions,
    ) -> crate::error::Result<bool> {
        let service_name = service.name().await;

        let current_version = Version::parse(&service.version().await)?;
        if !options.force
            && (current_version == options.target_version
                || options.target_version < current_version)
        {
            info!(
                "The service {service_name} is already at the latest version. No upgrade is required.",
            );
            return Ok(false);
        }

        info!("Upgrading {service_name} by stopping the service and copying the binary");
        Self::stop(service, service_control, verbosity)
            .await
            .inspect_err(|err| {
                error!("Failed to stop service {service_name}: {err}");
            })?;
        std::fs::copy(&options.target_bin_path, service.bin_path().await).inspect_err(|err| {
            error!("Failed to copy the binary for service {service_name}: {err}");
        })?;

        service_control
            .uninstall(&service_name, service.is_user_mode().await)
            .inspect_err(|err| {
                error!("Failed to uninstall service {service_name}: {err}");
            })?;

        service.set_metrics_port_if_not_set(service_control).await?;
        service_control.install(
            service
                .build_upgrade_install_context(options.clone())
                .await
                .inspect_err(|err| {
                    error!("Failed to build upgrade install context for service {service_name}: {err}");
                })?,
            service.is_user_mode().await,
        )
        .inspect_err(|err| {
            error!("Failed to install service {service_name}: {err}");
        })?;

        Ok(true)
    }
}

#[derive(Default, Debug)]
pub struct BatchResult {
    errors: std::collections::HashMap<String, Vec<crate::error::Error>>,
}

impl BatchResult {
    fn insert_error(&mut self, service_name: String, error: crate::error::Error) {
        self.errors.entry(service_name).or_default().push(error);
    }

    fn contains_errors(&self, service_name: &str) -> bool {
        self.errors
            .get(service_name)
            .is_some_and(|errors| !errors.is_empty())
    }

    pub fn get_errors(&self, service_name: &str) -> Option<&crate::error::Error> {
        self.errors.get(service_name).and_then(|errors| {
            if errors.is_empty() {
                None
            } else {
                // get the last error
                Some(&errors[errors.len() - 1])
            }
        })
    }
}

/// Summarise the batch result and print errors if any
pub fn summarise_batch_result(
    batch_result: &BatchResult,
    verb: &str,
    verbosity: VerbosityLevel,
) -> Result<()> {
    let failed_services: Vec<(String, String)> = batch_result
        .errors
        .iter()
        .flat_map(|(service_name, errors)| {
            errors
                .iter()
                .map(move |error| (service_name.clone(), error.to_string()))
        })
        .collect();

    if !failed_services.is_empty() {
        if verbosity != VerbosityLevel::Minimal {
            println!("Failed to {verb} {} service(s):", failed_services.len());
            for failed in failed_services.iter() {
                println!("{} {}: {}", "✕".red(), failed.0, failed.1);
            }
        }

        error!("Failed to {verb} one or more services");
        return Err(Error::ServiceBatchOperationFailed {
            verb: verb.to_string(),
        });
    }
    Ok(())
}
