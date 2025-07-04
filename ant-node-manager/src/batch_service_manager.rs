// Copyright (C) 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{error::Error, helpers::summarise_any_failed_ops, VerbosityLevel};
use ant_service_management::{
    control::ServiceControl, NodeRegistryManager, ServiceStateActions, ServiceStatus,
    UpgradeOptions, UpgradeResult,
};
use colored::Colorize;
use semver::Version;
use std::{collections::HashSet, time::Duration};

/// A manager for batch operations on multiple services.
/// This is similar to the `ServiceManager` but designed to handle multiple services together.
// todo: implement a channel to receive updates on the status of each service during any process.
pub struct BatchServiceManager<T: ServiceStateActions + Send> {
    /// The list of services to manage.
    services: Vec<T>,
    // Any Service name inside here will be skipped during the iteration.
    // This is primarily used to skip services that has failed a previous step.
    skip_services: HashSet<String>,
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
            skip_services: Default::default(),
            service_control,
            node_registry,
            verbosity,
        }
    }

    /// Starts all the services in the batch with a fixed interval between each start.
    pub async fn start_all(&mut self, fixed_interval: u64) -> color_eyre::Result<()> {
        let mut failed_services = HashSet::new();

        for service in &self.services {
            let service_name = service.name().await;
            if self.skip_services.contains(&service_name) {
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
                    self.skip_services.insert(service_name.clone());
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
                    failed_services.insert((service_name.clone(), err.to_string()));
                    self.skip_services.insert(service_name.clone());
                }
            };
        }

        // Now we wait for the service to be started.
        for service in &self.services {
            let service_name = service.name().await;
            if self.skip_services.contains(&service_name) {
                debug!("Skipping service {service_name} as it is marked to be skipped");
                continue;
            }

            info!("Waiting for service {service_name} to start...");
            if self.verbosity != VerbosityLevel::Minimal {
                println!("Waiting for {service_name} to start...");
            }
            if let Err(err) = service.wait_until_started().await {
                error!("Service {service_name} failed to wait_until_started: {err}");
                failed_services.insert((service_name.clone(), err.to_string()));
                self.skip_services.insert(service_name.clone());
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
                    failed_services.insert((service_name.clone(), err.to_string()));
                    self.skip_services.insert(service_name.clone());
                    continue;
                }
            };

            info!("Service {service_name} has started with PID {pid}, running on_start");
            match service.on_start(Some(pid), true).await {
                Ok(_) => {
                    info!("Service {service_name} has run on_start successfully");
                    self.node_registry.save().await?;

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
                    failed_services.insert((service_name.clone(), err.to_string()));
                    self.skip_services.insert(service_name.clone());
                    continue;
                }
            }

            info!("Service {service_name} has been started successfully");
        }

        self.node_registry.save().await?;

        summarise_any_failed_ops(failed_services, "start", self.verbosity)
    }

    pub async fn stop_all(&mut self) -> color_eyre::Result<()> {
        let mut failed_services = HashSet::new();

        for service in &self.services {
            let service_name = service.name().await.clone();
            match Self::stop(service, &self.service_control, self.verbosity).await {
                Ok(()) => {
                    info!("Stopped service {service_name}");
                    self.node_registry.save().await?;
                }
                Err(err) => {
                    error!("Failed to stop service {service_name}: {err}");

                    self.skip_services.insert(service_name.clone());
                    failed_services.insert((service_name, err.to_string()));
                }
            }
        }

        self.node_registry.save().await?;

        summarise_any_failed_ops(failed_services, "stop", self.verbosity)
    }

    pub async fn upgrade_all(
        &self,
        options: UpgradeOptions,
        fixed_interval: u64,
    ) -> color_eyre::Result<Vec<(String, String)>> {
        let mut upgrade_summary = HashSet::new();

        for service in &self.services {
            let service_name = service.name().await;
            info!("Upgrading the {service_name} service");

            match Self::reinstall_service(
                service,
                &self.service_control,
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
                    upgrade_summary.insert((service_name.clone(), UpgradeResult::NotRequired));
                }
                Err(err) => {
                    error!("Failed to upgrade service {service_name}: {err}");
                    upgrade_summary
                        .insert((service_name.clone(), UpgradeResult::Error(err.to_string())));
                }
            }
        }

        match self.start_all(fixed_interval).await.inspect_err(|err| {
            error!("Failed to start all services after upgrade: {err}");
        }) {
            Ok(_) => {
                info!("All services have been started after upgrade.");
            }
            Err(err) => {
                error!("Failed to start all services after upgrade: {err}");
                todo!("need 1 result per service here");
                // upgrade_summary.insert((service_name.clone(), UpgradeResult::Error(err.to_string())));
            }
        }

        for service in &self.services {
            service
                .set_version(&options.target_version.to_string())
                .await;
        }

        summarise_any_failed_ops(failed_services, "upgrade", self.verbosity)?;

        Ok(failed_services)
    }

    async fn stop(
        service: &T,
        service_control: &Box<dyn ServiceControl>,
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
        service_control: &Box<dyn ServiceControl>,
        verbosity: VerbosityLevel,
        options: UpgradeOptions,
    ) -> color_eyre::Result<bool> {
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
        Self::stop(service, &service_control, verbosity)
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
