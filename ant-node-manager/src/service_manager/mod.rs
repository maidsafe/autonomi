// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#[cfg(test)]
mod test;

use crate::{
    RPC_START_UP_DELAY_MS, VerbosityLevel,
    error::{Error, Result},
};
use ant_service_management::{
    ServiceStateActions, ServiceStatus, UpgradeOptions, UpgradeResult, control::ServiceControl,
    error::Error as ServiceError,
};
use colored::Colorize;
use semver::Version;
use tracing::debug;

pub struct ServiceManager<T: ServiceStateActions + Send> {
    pub service: T,
    pub service_control: Box<dyn ServiceControl + Send>,
    pub verbosity: VerbosityLevel,
}

impl<T: ServiceStateActions + Send> ServiceManager<T> {
    pub fn new(
        service: T,
        service_control: Box<dyn ServiceControl + Send>,
        verbosity: VerbosityLevel,
    ) -> Self {
        ServiceManager {
            service,
            service_control,
            verbosity,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        let service_name = self.service.name().await;
        info!("Starting the {service_name} service");
        if ServiceStatus::Running == self.service.status().await {
            // The last time we checked the service was running, but it doesn't mean it's actually
            // running now. If it is running, we don't need to do anything. If it stopped because
            // of a fault, we will drop to the code below and attempt to start it again.
            // We use `get_process_pid` because it searches for the process with the service binary
            // path, and this path is unique to each service.
            if self
                .service_control
                .get_process_pid(&self.service.bin_path().await)
                .is_ok()
            {
                debug!("The {service_name} service is already running",);
                if self.verbosity != VerbosityLevel::Minimal {
                    println!("The {service_name} service is already running",);
                }
                return Ok(());
            }
        }

        // At this point the service either hasn't been started for the first time or it has been
        // stopped. If it was stopped, it was either intentional or because it crashed.
        if self.verbosity != VerbosityLevel::Minimal {
            println!("Attempting to start {service_name}...");
        }
        self.service_control
            .start(&service_name, self.service.is_user_mode().await)?;
        self.service_control.wait(RPC_START_UP_DELAY_MS);

        // This is an attempt to see whether the service process has actually launched. You don't
        // always get an error from the service infrastructure.
        //
        // There might be many different `antnode` processes running, but since each service has
        // its own isolated binary, we use the binary path to uniquely identify it.
        match self
            .service_control
            .get_process_pid(&self.service.bin_path().await)
        {
            Ok(pid) => {
                debug!(
                    "Service process started for {service_name} with PID {}",
                    pid
                );
                self.service.on_start(Some(pid), true).await?;

                info!("Service {service_name} has been started successfully");
            }
            Err(ant_service_management::error::Error::ServiceProcessNotFound(_)) => {
                error!(
                    "The '{service_name}' service has failed to start because ServiceProcessNotFound when fetching PID"
                );
                return Err(Error::PidNotFoundAfterStarting);
            }
            Err(err) => {
                error!("Failed to start service, because PID could not be obtained: {err}");
                return Err(err.into());
            }
        };

        if self.verbosity != VerbosityLevel::Minimal {
            println!("{} Started {service_name} service", "✓".green(),);
            println!(
                "  - PID: {}",
                self.service
                    .pid()
                    .await
                    .map_or("-".to_string(), |p| p.to_string())
            );
            println!(
                "  - Bin path: {}",
                self.service.bin_path().await.to_string_lossy()
            );
            println!(
                "  - Data path: {}",
                self.service.data_dir_path().await.to_string_lossy()
            );
            println!(
                "  - Logs path: {}",
                self.service.log_dir_path().await.to_string_lossy()
            );
        }
        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        let service_name = self.service.name().await;
        info!("Stopping the {service_name} service");
        match self.service.status().await {
            ServiceStatus::Added => {
                debug!("The {service_name} service has not been started since it was installed",);
                if self.verbosity != VerbosityLevel::Minimal {
                    println!("Service {service_name} has not been started since it was installed",);
                }
                Ok(())
            }
            ServiceStatus::Removed => {
                debug!("The {service_name} service has been removed");
                if self.verbosity != VerbosityLevel::Minimal {
                    println!("Service {service_name} has been removed");
                }
                Ok(())
            }
            ServiceStatus::Running => {
                let pid = self.service.pid().await.ok_or(Error::PidNotSet)?;

                if self
                    .service_control
                    .get_process_pid(&self.service.bin_path().await)
                    .is_ok()
                {
                    if self.verbosity != VerbosityLevel::Minimal {
                        println!("Attempting to stop {service_name}...");
                    }
                    self.service_control
                        .stop(&service_name, self.service.is_user_mode().await)?;
                    if self.verbosity != VerbosityLevel::Minimal {
                        println!(
                            "{} Service {service_name} with PID {} was stopped",
                            "✓".green(),
                            pid
                        );
                    }
                } else if self.verbosity != VerbosityLevel::Minimal {
                    debug!("Service {service_name} was already stopped");
                    println!("{} Service {service_name} was already stopped", "✓".green());
                }

                self.service.on_stop().await?;
                info!("Service {service_name} has been stopped successfully.");
                Ok(())
            }
            ServiceStatus::Stopped => {
                debug!("Service {service_name} was already stopped");
                if self.verbosity != VerbosityLevel::Minimal {
                    println!("{} Service {service_name} was already stopped", "✓".green(),);
                }
                Ok(())
            }
        }
    }

    pub async fn remove(&mut self, keep_directories: bool) -> Result<()> {
        let service_name = self.service.name().await;
        info!("Removing the {service_name} service");
        if let ServiceStatus::Running = self.service.status().await {
            if self
                .service_control
                .get_process_pid(&self.service.bin_path().await)
                .is_ok()
            {
                error!("Service {service_name} is already running. Stop it before removing it",);
                return Err(Error::ServiceAlreadyRunning(vec![service_name]));
            } else {
                // If the node wasn't actually running, we should give the user an opportunity to
                // check why it may have failed before removing everything.
                self.service.on_stop().await?;
                error!(
                    "The service: {service_name} was marked as running but it had actually stopped. You may want to check the logs for errors before removing it. To remove the service, run the command again."
                );
                return Err(Error::ServiceStatusMismatch {
                    expected: ServiceStatus::Running,
                });
            }
        }

        match self
            .service_control
            .uninstall(&service_name, self.service.is_user_mode().await)
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
                        println!("The user appears to have removed the {name} service manually");
                    }
                }
                ServiceError::ServiceDoesNotExists(name) => {
                    warn!(
                        "The service {name} has most probably been removed already, it does not exists. Skipping the error."
                    );
                }
                _ => {
                    error!("Error uninstalling the service: {err}");
                    return Err(err.into());
                }
            },
        }

        if !keep_directories {
            debug!("Removing data and log directories for {service_name}");
            // It's possible the user deleted either of these directories manually.
            // We can just proceed with removing the service from the registry.
            let data_dir_path = self.service.data_dir_path().await;
            if data_dir_path.exists() {
                debug!("Removing data directory {data_dir_path:?}");
                std::fs::remove_dir_all(data_dir_path)?;
            }
            let log_dir_path = self.service.log_dir_path().await;
            if log_dir_path.exists() {
                debug!("Removing log directory {log_dir_path:?}");
                std::fs::remove_dir_all(log_dir_path)?;
            }
        }

        self.service.on_remove().await;
        info!("Service {service_name} has been removed successfully.");

        if self.verbosity != VerbosityLevel::Minimal {
            println!("{} Service {service_name} was removed", "✓".green());
        }

        Ok(())
    }

    pub async fn upgrade(&mut self, options: UpgradeOptions) -> Result<UpgradeResult> {
        let current_version = Version::parse(&self.service.version().await)?;
        if !options.force
            && (current_version == options.target_version
                || options.target_version < current_version)
        {
            info!(
                "The service {} is already at the latest version. No upgrade is required.",
                self.service.name().await
            );
            return Ok(UpgradeResult::NotRequired);
        }

        debug!("Stopping the service and copying the binary");
        self.stop().await?;
        std::fs::copy(
            options.clone().target_bin_path,
            self.service.bin_path().await,
        )?;

        self.service_control.uninstall(
            &self.service.name().await,
            self.service.is_user_mode().await,
        )?;
        self.service_control.install(
            self.service
                .build_upgrade_install_context(options.clone())
                .await?,
            self.service.is_user_mode().await,
        )?;

        if options.start_service {
            match self.start().await {
                Ok(start_duration) => start_duration,
                Err(err) => {
                    self.service
                        .set_version(&options.target_version.to_string())
                        .await;
                    info!("The service has been upgraded but could not be started: {err}");
                    return Ok(UpgradeResult::UpgradedButNotStarted(
                        current_version.to_string(),
                        options.target_version.to_string(),
                        err.to_string(),
                    ));
                }
            }
        }
        self.service
            .set_version(&options.target_version.to_string())
            .await;

        if options.force {
            Ok(UpgradeResult::Forced(
                current_version.to_string(),
                options.target_version.to_string(),
            ))
        } else {
            Ok(UpgradeResult::Upgraded(
                current_version.to_string(),
                options.target_version.to_string(),
            ))
        }
    }
}
