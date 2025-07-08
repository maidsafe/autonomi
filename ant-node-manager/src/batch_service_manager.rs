// Copyright (C) 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{cmd::print_upgrade_summary, error::Error, VerbosityLevel};
use ant_service_management::{
    control::ServiceControl, Error as ServiceError, NodeRegistryManager, ServiceStateActions,
    ServiceStatus, UpgradeOptions, UpgradeResult,
};
use color_eyre::{eyre::eyre, Section};
use colored::Colorize;
use semver::Version;
use std::collections::{HashMap, HashSet};

/// A manager for batch operations on multiple services.
/// This is similar to the `ServiceManager` but designed to handle multiple services together.
// todo: implement a channel to receive updates on the status of each service during any process.
pub struct BatchServiceManager<T: ServiceStateActions + Send> {
    /// The list of services to manage.
    services: Vec<T>,
    service_control: Box<dyn ServiceControl>,
    node_registry: NodeRegistryManager,
    verbosity: VerbosityLevel,
}

impl<T: ServiceStateActions + Send> BatchServiceManager<T> {
    pub async fn new(
        services: Vec<T>,
        service_control: Box<dyn ServiceControl>,
        node_registry: NodeRegistryManager,
        verbosity: VerbosityLevel,
    ) -> Self {
        BatchServiceManager {
            services,
            service_control,
            node_registry,
            verbosity,
        }
    }

    /// Starts all the services in the batch with a fixed interval between each start.
    pub async fn start_all(&self, fixed_interval: u64) -> color_eyre::Result<()> {
        let batch_result = self
            .start_all_inner(fixed_interval, Default::default())
            .await;

        if !batch_result.errors.is_empty() {
            error!("Failed to start one or more services: {batch_result:?}");
        }

        batch_result.summarise("start", self.verbosity)
    }

    pub async fn stop_all(&self, interval: Option<u64>) -> color_eyre::Result<()> {
        let mut batch_result = BatchResult::default();

        for service in &self.services {
            let service_name = service.name().await.clone();
            if service.status().await == ServiceStatus::Running {
                if let Some(interval) = interval {
                    debug!("Sleeping for {interval} milliseconds before stopping service {service_name}");
                    std::thread::sleep(std::time::Duration::from_millis(interval));
                }
            }

            match Self::stop(service, self.service_control.as_ref(), self.verbosity).await {
                Ok(()) => {
                    info!("Stopped service {service_name}");
                    self.node_registry.save().await?;
                }
                Err(err) => {
                    error!("Failed to stop service {service_name}: {err}");

                    batch_result.insert_error(service_name, err);
                }
            }
        }

        self.node_registry.save().await?;

        batch_result.summarise("stop", self.verbosity)?;
        Ok(())
    }

    pub async fn upgrade_all(
        &self,
        options: UpgradeOptions,
        fixed_interval: u64,
    ) -> color_eyre::Result<()> {
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
                    info!("Service {service_name} is already at the target version or lower. No upgrade is required.");
                    skip_services.insert(service_name.clone());
                    upgrade_summary.insert(service_name.clone(), UpgradeResult::NotRequired);
                }
                Err(err) => {
                    error!("Failed to upgrade service {service_name}: {err}");
                    batch_result.insert_error(service_name.clone(), err);
                }
            }
        }

        let batch_result = self
            .start_all_inner(fixed_interval, skip_services.clone())
            .await;

        if !batch_result.errors.is_empty() {
            info!("All services have been started after upgrade.");
        } else {
            error!("Failed to start one or more services after upgrade: {batch_result:?}");
        }

        for service in &self.services {
            let old_service_version = service.version().await;
            service
                .set_version(&options.target_version.to_string())
                .await;

            let service_name = service.name().await;
            match batch_result.get_errors(&service_name) {
                Some(err) => {
                    info!("The service {service_name} has been upgraded but could not be started: {err}");
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

        if self.verbosity != VerbosityLevel::Minimal {
            print_upgrade_summary(upgrade_summary.clone());
        }

        if upgrade_summary.iter().any(|(_, r)| {
            matches!(r, UpgradeResult::Error(_))
                || matches!(r, UpgradeResult::UpgradedButNotStarted(_, _, _))
        }) {
            return Err(eyre!("There was a problem upgrading one or more nodes").suggestion(
            "For any services that were upgraded but did not start, you can attempt to start them \
                again using the 'start' command."));
        }

        Ok(())
    }

    pub async fn remove_all(&self, keep_directories: bool) -> color_eyre::Result<()> {
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
                    error!("The service: {service_name} was marked as running but it had actually stopped. You may want to check the logs for errors before removing it. To remove the service, run the command again.");

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
                        warn!("The user appears to have removed the {name} service manually. Skipping the error.",);
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
                        warn!("The service {name} has most probably been removed already, it does not exists. Skipping the error.");
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
                    std::fs::remove_dir_all(data_dir_path)?;
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

        batch_result.summarise("remove", self.verbosity)
    }

    async fn start_all_inner(
        &self,
        fixed_interval: u64,
        skip_services: HashSet<String>,
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
                    info!("Started service {service_name}, waiting for fixed interval of {fixed_interval} seconds before checking if it has started");
                    self.service_control.wait(fixed_interval);
                }
                Err(err) => {
                    error!("Failed to start service {service_name}: {err}");
                    batch_result.insert_error(service_name.clone(), err.into());
                }
            };
        }

        // Now we wait for the service to be started.
        for service in &self.services {
            let service_name = service.name().await;
            if skip_services.contains(&service_name) || batch_result.contains_errors(&service_name)
            {
                debug!("Skipping service {service_name} as it is marked to be skipped");
                continue;
            }

            info!("Waiting for service {service_name} to start...");
            if self.verbosity != VerbosityLevel::Minimal {
                println!("Waiting for {service_name} to start...");
            }
            if let Err(err) = service.wait_until_started().await {
                error!("Service {service_name} failed to wait_until_started: {err}");
                batch_result.insert_error(service_name.clone(), err.into());
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

            info!("Service {service_name} has started with PID {pid}, running on_start");
            match service.on_start(Some(pid), true).await {
                Ok(_) => {
                    info!("Service {service_name} has run on_start successfully");
                    if let Err(err) = self.node_registry.save().await {
                        error!("Failed to save node registry after starting service {service_name}: {err}");
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
            info!("The service {service_name} is already at the latest version. No upgrade is required.",);
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
struct BatchResult {
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

    fn get_errors(&self, service_name: &str) -> Option<&crate::error::Error> {
        self.errors.get(service_name).and_then(|errors| {
            if errors.is_empty() {
                None
            } else {
                // get the last error
                Some(&errors[errors.len() - 1])
            }
        })
    }

    fn summarise(&self, verb: &str, verbosity: VerbosityLevel) -> color_eyre::Result<()> {
        let failed_services: Vec<(String, String)> = self
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
            return Err(eyre!("Failed to {verb} one or more services"));
        }
        Ok(())
    }
}
