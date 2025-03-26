// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#[macro_use]
extern crate tracing;

pub mod add_services;
pub mod cmd;
pub mod config;
pub mod error;
pub mod helpers;
pub mod local;
pub mod metric;

pub const DEFAULT_NODE_STARTUP_CONNECTION_TIMEOUT_S: u64 = 300;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum VerbosityLevel {
    Minimal,
    Normal,
    Full,
}

impl From<u8> for VerbosityLevel {
    fn from(verbosity: u8) -> Self {
        match verbosity {
            1 => VerbosityLevel::Minimal,
            2 => VerbosityLevel::Normal,
            3 => VerbosityLevel::Full,
            _ => VerbosityLevel::Normal,
        }
    }
}

use std::sync::Arc;

use crate::error::{Error, Result};
use ant_service_management::metric::{MetricActions, MetricClient};
use ant_service_management::{
    control::ServiceControl, error::Error as ServiceError, NodeRegistry, NodeService,
    NodeServiceData, ServiceStateActions, ServiceStatus, UpgradeOptions, UpgradeResult,
};
use colored::Colorize;
use libp2p::futures::future::try_join_all;
use semver::Version;
use tracing::debug;

pub const DAEMON_DEFAULT_PORT: u16 = 12500;
pub const DAEMON_SERVICE_NAME: &str = "antctld";

const METRIC_START_UP_DELAY_MS: u64 = 3000;

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
        info!("Starting the {} service", self.service.name());
        if ServiceStatus::Running == self.service.status() {
            // The last time we checked the service was running, but it doesn't mean it's actually
            // running now. If it is running, we don't need to do anything. If it stopped because
            // of a fault, we will drop to the code below and attempt to start it again.
            // We use `get_process_pid` because it searches for the process with the service binary
            // path, and this path is unique to each service.
            if self
                .service_control
                .get_process_pid(&self.service.bin_path())
                .is_ok()
            {
                debug!("The {} service is already running", self.service.name());
                if self.verbosity != VerbosityLevel::Minimal {
                    println!("The {} service is already running", self.service.name());
                }
                return Ok(());
            }
        }

        // At this point the service either hasn't been started for the first time or it has been
        // stopped. If it was stopped, it was either intentional or because it crashed.
        if self.verbosity != VerbosityLevel::Minimal {
            println!("Attempting to start {}...", self.service.name());
        }
        self.service_control
            .start(&self.service.name(), self.service.is_user_mode())?;
        self.service_control.wait(METRIC_START_UP_DELAY_MS);

        // This is an attempt to see whether the service process has actually launched. You don't
        // always get an error from the service infrastructure.
        //
        // There might be many different `antnode` processes running, but since each service has
        // its own isolated binary, we use the binary path to uniquely identify it.
        match self
            .service_control
            .get_process_pid(&self.service.bin_path())
        {
            Ok(pid) => {
                debug!(
                    "Service process started for {} with PID {}",
                    self.service.name(),
                    pid
                );
                self.service.on_start(Some(pid), true).await?;

                info!(
                    "Service {} has been started successfully",
                    self.service.name()
                );
            }
            Err(ant_service_management::error::Error::ServiceProcessNotFound(_)) => {
                error!("The '{}' service has failed to start because ServiceProcessNotFound when fetching PID", self.service.name());
                return Err(Error::PidNotFoundAfterStarting);
            }
            Err(err) => {
                error!("Failed to start service, because PID could not be obtained: {err}");
                return Err(err.into());
            }
        };

        if self.verbosity != VerbosityLevel::Minimal {
            println!("{} Started {} service", "✓".green(), self.service.name());
            println!(
                "  - PID: {}",
                self.service
                    .pid()
                    .map_or("-".to_string(), |p| p.to_string())
            );
            println!(
                "  - Bin path: {}",
                self.service.bin_path().to_string_lossy()
            );
            println!(
                "  - Data path: {}",
                self.service.data_dir_path().to_string_lossy()
            );
            println!(
                "  - Logs path: {}",
                self.service.log_dir_path().to_string_lossy()
            );
        }
        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        info!("Stopping the {} service", self.service.name());
        match self.service.status() {
            ServiceStatus::Added => {
                debug!(
                    "The {} service has not been started since it was installed",
                    self.service.name()
                );
                if self.verbosity != VerbosityLevel::Minimal {
                    println!(
                        "Service {} has not been started since it was installed",
                        self.service.name()
                    );
                }
                Ok(())
            }
            ServiceStatus::Removed => {
                debug!("The {} service has been removed", self.service.name());
                if self.verbosity != VerbosityLevel::Minimal {
                    println!("Service {} has been removed", self.service.name());
                }
                Ok(())
            }
            ServiceStatus::Running => {
                let pid = self.service.pid().ok_or(Error::PidNotSet)?;
                let name = self.service.name();

                if self
                    .service_control
                    .get_process_pid(&self.service.bin_path())
                    .is_ok()
                {
                    if self.verbosity != VerbosityLevel::Minimal {
                        println!("Attempting to stop {}...", name);
                    }
                    self.service_control
                        .stop(&name, self.service.is_user_mode())?;
                    if self.verbosity != VerbosityLevel::Minimal {
                        println!(
                            "{} Service {} with PID {} was stopped",
                            "✓".green(),
                            name,
                            pid
                        );
                    }
                } else if self.verbosity != VerbosityLevel::Minimal {
                    debug!("Service {name} was already stopped");
                    println!("{} Service {} was already stopped", "✓".green(), name);
                }

                self.service.on_stop().await?;
                info!("Service {name} has been stopped successfully.");
                Ok(())
            }
            ServiceStatus::Stopped => {
                debug!("Service {} was already stopped", self.service.name());
                if self.verbosity != VerbosityLevel::Minimal {
                    println!(
                        "{} Service {} was already stopped",
                        "✓".green(),
                        self.service.name()
                    );
                }
                Ok(())
            }
        }
    }

    pub async fn remove(&mut self, keep_directories: bool) -> Result<()> {
        if let ServiceStatus::Running = self.service.status() {
            if self
                .service_control
                .get_process_pid(&self.service.bin_path())
                .is_ok()
            {
                error!(
                    "Service {} is already running. Stop it before removing it",
                    self.service.name()
                );
                return Err(Error::ServiceAlreadyRunning(vec![self.service.name()]));
            } else {
                // If the node wasn't actually running, we should give the user an opportunity to
                // check why it may have failed before removing everything.
                self.service.on_stop().await?;
                error!(
                "The service: {} was marked as running but it had actually stopped. You may want to check the logs for errors before removing it. To remove the service, run the command again.",
                self.service.name()
            );
                return Err(Error::ServiceStatusMismatch {
                    expected: ServiceStatus::Running,
                });
            }
        }

        match self
            .service_control
            .uninstall(&self.service.name(), self.service.is_user_mode())
        {
            Ok(()) => {
                debug!("Service {} has been uninstalled", self.service.name());
            }
            Err(err) => match err {
                ServiceError::ServiceRemovedManually(name) => {
                    warn!("The user appears to have removed the {name} service manually. Skipping the error.",);
                    // The user has deleted the service definition file, which the service manager
                    // crate treats as an error. We then return our own error type, which allows us
                    // to handle it here and just proceed with removing the service from the
                    // registry.
                    if self.verbosity != VerbosityLevel::Minimal {
                        println!("The user appears to have removed the {name} service manually");
                    }
                }
                ServiceError::ServiceDoesNotExists(name) => {
                    warn!("The service {name} has most probably been removed already, it does not exists. Skipping the error.");
                }
                _ => {
                    error!("Error uninstalling the service: {err}");
                    return Err(err.into());
                }
            },
        }

        if !keep_directories {
            debug!(
                "Removing data and log directories for {}",
                self.service.name()
            );
            // It's possible the user deleted either of these directories manually.
            // We can just proceed with removing the service from the registry.
            if self.service.data_dir_path().exists() {
                debug!("Removing data directory {:?}", self.service.data_dir_path());
                std::fs::remove_dir_all(self.service.data_dir_path())?;
            }
            if self.service.log_dir_path().exists() {
                debug!("Removing log directory {:?}", self.service.log_dir_path());
                std::fs::remove_dir_all(self.service.log_dir_path())?;
            }
        }

        self.service.on_remove();
        info!(
            "Service {} has been removed successfully.",
            self.service.name()
        );

        if self.verbosity != VerbosityLevel::Minimal {
            println!(
                "{} Service {} was removed",
                "✓".green(),
                self.service.name()
            );
        }

        Ok(())
    }

    pub async fn upgrade(&mut self, options: UpgradeOptions) -> Result<UpgradeResult> {
        let current_version = Version::parse(&self.service.version())?;
        if !options.force
            && (current_version == options.target_version
                || options.target_version < current_version)
        {
            info!(
                "The service {} is already at the latest version. No upgrade is required.",
                self.service.name()
            );
            return Ok(UpgradeResult::NotRequired);
        }

        debug!("Stopping the service and copying the binary");
        self.stop().await?;
        std::fs::copy(options.clone().target_bin_path, self.service.bin_path())?;

        self.service_control
            .uninstall(&self.service.name(), self.service.is_user_mode())?;
        self.service_control.install(
            self.service
                .build_upgrade_install_context(options.clone())?,
            self.service.is_user_mode(),
        )?;

        if options.start_service {
            match self.start().await {
                Ok(start_duration) => start_duration,
                Err(err) => {
                    self.service
                        .set_version(&options.target_version.to_string());
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
            .set_version(&options.target_version.to_string());

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

pub async fn status_report(
    node_registry: &mut NodeRegistry,
    service_control: Arc<dyn ServiceControl + Send + Sync>,
    detailed_view: bool,
    output_json: bool,
    fail: bool,
    is_local_network: bool,
) -> Result<()> {
    // let service_control: Arc<dyn ServiceControl + Send + Sync> = Arc::new(ServiceControl::new());
    refresh_node_registry(
        node_registry,
        service_control,
        !output_json,
        true,
        is_local_network,
    )
    .await?;

    if output_json {
        let json = serde_json::to_string_pretty(&node_registry.to_status_summary())?;
        println!("{json}");
    } else if detailed_view {
        for node in &node_registry.nodes {
            print_banner(&format!(
                "{} - {}",
                &node.service_name,
                format_status_without_colour(&node.status)
            ));
            println!("Version: {}", node.version);
            println!(
                "Peer ID: {}",
                node.peer_id.map_or("-".to_string(), |p| p.to_string())
            );
            println!("Listen Addresses: {:?}", node.listen_addr);
            println!(
                "PID: {}",
                node.pid.map_or("-".to_string(), |p| p.to_string())
            );
            if node.status == ServiceStatus::Stopped {
                if let Some(failure_reason) = node.get_critical_failure() {
                    println!(
                        "Failure reason: [{}] {}",
                        failure_reason.0, failure_reason.1
                    );
                }
            }
            println!("Data path: {}", node.data_dir_path.to_string_lossy());
            println!("Log path: {}", node.log_dir_path.to_string_lossy());
            println!("Bin path: {}", node.antnode_path.to_string_lossy());
            // println!(
            //     "Connected peers is: {}",
            //     node.connected_peers
            //         .as_ref()
            //         .map_or("-".to_string(), |p| p.len().to_string())
            // );
            println!(
                "Reward balance: {}",
                node.reward_balance
                    .map_or("-".to_string(), |b| b.to_string())
            );
            println!("Rewards address: {}", node.rewards_address);
            println!();
        }

        if let Some(daemon) = &node_registry.daemon {
            print_banner(&format!(
                "{} - {}",
                &daemon.service_name,
                format_status(&daemon.status)
            ));
            println!("Version: {}", daemon.version);
            println!("Bin path: {}", daemon.daemon_path.to_string_lossy());
        }

        if let Some(faucet) = &node_registry.faucet {
            print_banner(&format!(
                "{} - {}",
                &faucet.service_name,
                format_status(&faucet.status)
            ));
            println!("Version: {}", faucet.version);
            println!("Bin path: {}", faucet.faucet_path.to_string_lossy());
            println!("Log path: {}", faucet.log_dir_path.to_string_lossy());
        }
    } else {
        println!(
            "{:<18} {:<52} {:<7} {:<}",
            // {:>15}",
            "Service Name",
            "Peer ID",
            "Status",
            //"Connected Peers",
            "Failure"
        );
        let nodes = node_registry
            .nodes
            .iter()
            .filter(|n| n.status != ServiceStatus::Removed)
            .collect::<Vec<&NodeServiceData>>();
        for node in nodes {
            let peer_id = node.peer_id.map_or("-".to_string(), |p| p.to_string());
            let _connected_peers = node
                .connected_peers
                .clone()
                .map_or("-".to_string(), |p| p.len().to_string());
            let failure_reason = if node.status == ServiceStatus::Stopped {
                node.get_critical_failure()
                    .map_or("-".to_string(), |(_time, reason)| reason)
            } else {
                "-".to_string()
            };
            println!(
                "{:<18} {:<52} {:<7} {:<}",
                // {:>15}",
                node.service_name,
                peer_id,
                format_status(&node.status),
                //connected_peers
                failure_reason
            );
        }
        if let Some(daemon) = &node_registry.daemon {
            println!(
                "{:<18} {:<52} {:<7} {:>15} {:>15}",
                daemon.service_name,
                "-",
                format_status(&daemon.status),
                "-",
                "-"
            );
        }
        if let Some(faucet) = &node_registry.faucet {
            println!(
                "{:<18} {:<52} {:<7} {:>15} {:>15}",
                faucet.service_name,
                "-",
                format_status(&faucet.status),
                "-",
                "-"
            );
        }
    }

    if fail {
        let non_running_services = node_registry
            .nodes
            .iter()
            .filter_map(|n| {
                if n.status != ServiceStatus::Running {
                    Some(n.service_name.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<String>>();
        if non_running_services.is_empty() {
            info!("Fail is set to true, but all services are running.");
        } else {
            error!(
                "One or more nodes are not in a running state: {non_running_services:?}
            "
            );

            return Err(Error::ServiceNotRunning(non_running_services));
        }
    }

    Ok(())
}

/// Refreshes the status of the node registry's services.
///
/// The mechanism is different, depending on whether it's a service-based network or a local
/// network.
///
/// For a service-based network, at a minimum, the refresh determines if each service is running.
/// It does that by trying to find a process whose binary path matches the path of the binary for
/// the service. Since each service uses its own binary, the path is a unique identifer. So you can
/// know if any *particular* service is running or not. A full refresh uses the RPC client to
/// connect to the node's RPC service to determine things like the number of connected peers.
///
/// For a local network, the node paths are not unique, so we can't use that. We consider the node
/// running if we can connect to its RPC service; otherwise it is considered stopped.
pub async fn refresh_node_registry(
    node_registry: &mut NodeRegistry,
    service_control: Arc<dyn ServiceControl + Send + Sync>,
    print_refresh_message: bool,
    full_refresh: bool,
    is_local_network: bool,
) -> Result<()> {
    if print_refresh_message {
        println!("Refreshing the node registry...");
    }
    info!("Refreshing the node registry");

    let mut tasks = vec![];

    for node in node_registry.nodes.iter_mut() {
        // Clone things that are needed inside the task
        let mut node = node.clone(); // Necessary because you can't move &mut node across .await safely
        let service_control = Arc::clone(&service_control); // trait object already valid for sync reference
        let full_refresh = full_refresh;
        let is_local_network = is_local_network;

        let metrics_port = node.metrics_port
                                    .ok_or_else
                                    (|| ant_service_management::Error::MetricServicePortNumUnavailable)?;

        let task = tokio::task::spawn(async move {
            node.reward_balance = None;
            // let metrics_port = node.metrics_port.ok_or(Error::MetricPortEmpty)?;

            let metric_client = MetricClient::new(metrics_port);
            let mut service = NodeService::new(&mut node, Box::new(metric_client.clone()));
            
            if is_local_network {
                match metric_client.node_info().await {
                    Ok(info) => {
                        let pid = info.pid;
                        debug!(
                            "local node {} is running with PID {pid}",
                            service.service_data.service_name
                        );
                        service.on_start(Some(pid), full_refresh).await
                    }
                    Err(_) => {
                        debug!(
                            "Failed to retrieve PID for local node {}",
                            service.service_data.service_name
                        );
                        service.on_stop().await
                    }
                }
            } else {
                match service_control.get_process_pid(&service.bin_path()) {
                    Ok(pid) => {
                        debug!(
                            "{} is running with PID {pid}",
                            service.service_data.service_name
                        );
                        service.on_start(Some(pid), full_refresh).await
                    }
                    Err(_) => match service.status() {
                        ServiceStatus::Added => {
                            debug!(
                                "{} has not been started since it was installed",
                                service.service_data.service_name
                            );
                            Ok(())
                        }
                        ServiceStatus::Removed => {
                            debug!("{} has been removed", service.service_data.service_name);
                            Ok(())
                        }
                        _ => {
                            debug!(
                                "Failed to retrieve PID for {}",
                                service.service_data.service_name
                            );
                            service.on_stop().await
                        }
                    },
                }
            }
        });

        tasks.push(task);
    }

    // Wait for all tasks to complete
    let results: Vec<Result<(), Error>> = try_join_all(tasks)
                                            .await?.into_iter()
                                            .map(|res| res.map_err(|e| e.into())).collect();

    // Each task returns a Result<()>, so we propagate any inner error
    for result in &results {
        _ =result; // Prevents moving `result`
    }
    Ok(())
}


pub fn print_banner(text: &str) {
    let padding = 2;
    let text_width = text.len() + padding * 2;
    let border_chars = 2;
    let total_width = text_width + border_chars;
    let top_bottom = "═".repeat(total_width);

    println!("╔{}╗", top_bottom);
    println!("║ {:^width$} ║", text, width = text_width);
    println!("╚{}╝", top_bottom);
}

fn format_status(status: &ServiceStatus) -> String {
    match status {
        ServiceStatus::Running => "RUNNING".green().to_string(),
        ServiceStatus::Stopped => "STOPPED".red().to_string(),
        ServiceStatus::Added => "ADDED".yellow().to_string(),
        ServiceStatus::Removed => "REMOVED".red().to_string(),
    }
}

fn format_status_without_colour(status: &ServiceStatus) -> String {
    match status {
        ServiceStatus::Running => "RUNNING".to_string(),
        ServiceStatus::Stopped => "STOPPED".to_string(),
        ServiceStatus::Added => "ADDED".to_string(),
        ServiceStatus::Removed => "REMOVED".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ant_bootstrap::InitialPeersConfig;
    use ant_evm::{AttoTokens, CustomNetwork, EvmNetwork, RewardsAddress};
    use ant_logging::LogFormat;
    use ant_service_management::{
        error::{Error as ServiceControlError, Result as ServiceControlResult},
        metric::{NetworkInfo, NodeInfo},
        node::{NodeService, NodeServiceData},
        UpgradeOptions, UpgradeResult,
    };
    use assert_fs::prelude::*;
    use assert_matches::assert_matches;
    use async_trait::async_trait;
    use color_eyre::eyre::Result;
    use libp2p_identity::PeerId;
    use mockall::{mock, predicate::*};
    use predicates::prelude::*;
    use service_manager::ServiceInstallCtx;
    use std::{
        ffi::OsString,
        net::Ipv4Addr,
        path::{Path, PathBuf},
        str::FromStr,
        time::Duration,
    };

    mock! {
        pub MetricClient {}
        #[async_trait]
        impl MetricActions for MetricClient {
            async fn node_info(&self) -> ServiceControlResult<NodeInfo>;
            async fn network_info(&self) -> ServiceControlResult<NetworkInfo>;
            async fn is_node_connected_to_network(&self, timeout: std::time::Duration) -> ServiceControlResult<()>;
        }
    }

    mock! {
        pub ServiceControl {}
        impl ServiceControl for ServiceControl {
            fn create_service_user(&self, username: &str) -> ServiceControlResult<()>;
            fn get_available_port(&self) -> ServiceControlResult<u16>;
            fn install(&self, install_ctx: ServiceInstallCtx, user_mode: bool) -> ServiceControlResult<()>;
            fn get_process_pid(&self, bin_path: &Path) -> ServiceControlResult<u32>;
            fn start(&self, service_name: &str, user_mode: bool) -> ServiceControlResult<()>;
            fn stop(&self, service_name: &str, user_mode: bool) -> ServiceControlResult<()>;
            fn uninstall(&self, service_name: &str, user_mode: bool) -> ServiceControlResult<()>;
            fn wait(&self, delay: u64);
        }
    }

    #[tokio::test]
    async fn start_should_start_a_newly_installed_service() -> Result<()> {
        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
            .times(1)
            .returning(|_| Ok(1000));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 1000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: "0.98.1".to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: vec![PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?],
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: None,
            peers_args: InitialPeersConfig::default(),
            pid: None,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Added,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager.start().await?;

        assert_eq!(
            service_manager.service.service_data.connected_peers,
            Some(vec![PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?,])
        );
        assert_eq!(service_manager.service.service_data.pid, Some(1000));
        assert_eq!(
            service_manager.service.service_data.peer_id,
            Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR"
            )?)
        );
        assert_matches!(
            service_manager.service.service_data.status,
            ServiceStatus::Running
        );

        Ok(())
    }

    #[tokio::test]
    async fn start_should_start_a_stopped_service() -> Result<()> {
        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
            .times(1)
            .returning(|_| Ok(1000));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 1000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: "0.98.1".to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: None,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Stopped,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager.start().await?;

        assert_eq!(service_manager.service.service_data.pid, Some(1000));
        assert_eq!(
            service_manager.service.service_data.peer_id,
            Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR"
            )?)
        );
        assert_matches!(
            service_manager.service.service_data.status,
            ServiceStatus::Running
        );

        Ok(())
    }

    #[tokio::test]
    async fn start_should_not_attempt_to_start_a_running_service() -> Result<()> {
        let mut mock_service_control = MockServiceControl::new();
        let mock_metric_client = MockMetricClient::new();

        mock_service_control
            .expect_get_process_pid()
            .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
            .times(1)
            .returning(|_| Ok(100));

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager.start().await?;

        assert_eq!(service_manager.service.service_data.pid, Some(1000));
        assert_eq!(
            service_manager.service.service_data.peer_id,
            Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR"
            )?)
        );
        assert_matches!(
            service_manager.service.service_data.status,
            ServiceStatus::Running
        );

        Ok(())
    }

    #[tokio::test]
    async fn start_should_start_a_service_marked_as_running_but_had_since_stopped() -> Result<()> {
        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        mock_service_control
            .expect_get_process_pid()
            .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
            .times(1)
            .returning(|_| {
                Err(ServiceError::ServiceProcessNotFound(
                    "Could not find process at '/var/antctl/services/antnode1/antnode'".to_string(),
                ))
            });
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
            .times(1)
            .returning(|_| Ok(1000));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 1000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: "0.98.1".to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager.start().await?;

        assert_eq!(service_manager.service.service_data.pid, Some(1000));
        assert_eq!(
            service_manager.service.service_data.peer_id,
            Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR"
            )?)
        );
        assert_matches!(
            service_manager.service.service_data.status,
            ServiceStatus::Running
        );

        Ok(())
    }

    #[tokio::test]
    async fn start_should_return_an_error_if_the_process_was_not_found() -> Result<()> {
        let mut mock_service_control = MockServiceControl::new();

        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
            .times(1)
            .returning(|_| {
                Err(ServiceControlError::ServiceProcessNotFound(
                    "/var/antctl/services/antnode1/antnode".to_string(),
                ))
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: None,
            peers_args: InitialPeersConfig::default(),
            pid: None,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Added,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(MockMetricClient::new()));
        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        let result = service_manager.start().await;
        match result {
            Ok(_) => panic!("This test should have resulted in an error"),
            Err(e) => assert_eq!(
                "The PID of the process was not found after starting it.",
                e.to_string()
            ),
        }

        Ok(())
    }

    #[tokio::test]
    async fn start_should_start_a_user_mode_service() -> Result<()> {
        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(true))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
            .times(1)
            .returning(|_| Ok(100));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 1000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: "0.98.1".to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: None,
            peers_args: InitialPeersConfig::default(),
            pid: None,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Added,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: true,
            version: "0.98.1".to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager.start().await?;

        Ok(())
    }

    #[tokio::test]
    async fn start_should_use_dynamic_startup_delay_if_set() -> Result<()> {
        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
            .times(1)
            .returning(|_| Ok(1000));
        mock_metric_client
            .expect_is_node_connected_to_network()
            .times(1)
            .returning(|_| Ok(()));
        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 1000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: "0.98.1".to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: vec![PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?],
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: None,
            peers_args: InitialPeersConfig::default(),
            pid: None,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Added,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client))
            .with_connection_timeout(Duration::from_secs(
                DEFAULT_NODE_STARTUP_CONNECTION_TIMEOUT_S,
            ));
        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager.start().await?;

        Ok(())
    }

    #[tokio::test]
    async fn stop_should_stop_a_running_service() -> Result<()> {
        let mut mock_service_control = MockServiceControl::new();

        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_get_process_pid()
            .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
            .times(1)
            .returning(|_| Ok(100));

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(MockMetricClient::new()));
        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager.stop().await?;

        assert_eq!(service_manager.service.service_data.pid, None);
        assert_eq!(service_manager.service.service_data.connected_peers, None);
        assert_matches!(
            service_manager.service.service_data.status,
            ServiceStatus::Stopped
        );
        Ok(())
    }

    #[tokio::test]
    async fn stop_should_not_return_error_for_attempt_to_stop_installed_service() -> Result<()> {
        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: None,
            peers_args: InitialPeersConfig::default(),
            pid: None,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Added,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(MockMetricClient::new()));
        let mut service_manager = ServiceManager::new(
            service,
            Box::new(MockServiceControl::new()),
            VerbosityLevel::Normal,
        );

        let result = service_manager.stop().await;

        match result {
            Ok(()) => Ok(()),
            Err(_) => {
                panic!("The stop command should be idempotent and do nothing for an added service");
            }
        }
    }

    #[tokio::test]
    async fn stop_should_return_ok_when_attempting_to_stop_service_that_was_already_stopped(
    ) -> Result<()> {
        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: None,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Stopped,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(MockMetricClient::new()));
        let mut service_manager = ServiceManager::new(
            service,
            Box::new(MockServiceControl::new()),
            VerbosityLevel::Normal,
        );

        let result = service_manager.stop().await;

        match result {
            Ok(()) => Ok(()),
            Err(_) => {
                panic!(
                    "The stop command should be idempotent and do nothing for an stopped service"
                );
            }
        }
    }

    #[tokio::test]
    async fn stop_should_return_ok_when_attempting_to_stop_a_removed_service() -> Result<()> {
        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: None,
            peers_args: InitialPeersConfig::default(),
            pid: None,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Removed,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(MockMetricClient::new()));
        let mut service_manager = ServiceManager::new(
            service,
            Box::new(MockServiceControl::new()),
            VerbosityLevel::Normal,
        );

        let result = service_manager.stop().await;

        match result {
            Ok(()) => Ok(()),
            Err(_) => {
                panic!(
                    "The stop command should be idempotent and do nothing for a removed service"
                );
            }
        }
    }

    #[tokio::test]
    async fn stop_should_stop_a_user_mode_service() -> Result<()> {
        let mut mock_service_control = MockServiceControl::new();

        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(true))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_get_process_pid()
            .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
            .times(1)
            .returning(|_| Ok(100));

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: None,
            user_mode: true,
            version: "0.98.1".to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(MockMetricClient::new()));
        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager.stop().await?;

        assert_eq!(service_manager.service.service_data.pid, None);
        assert_eq!(service_manager.service.service_data.connected_peers, None);
        assert_matches!(
            service_manager.service.service_data.status,
            ServiceStatus::Stopped
        );
        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_upgrade_a_service_to_a_new_version() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(always(), always())
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(2000));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));
        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        let upgrade_result = service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        match upgrade_result {
            UpgradeResult::Upgraded(old_version, new_version) => {
                assert_eq!(old_version, current_version);
                assert_eq!(new_version, target_version);
            }
            _ => panic!(
                "Expected UpgradeResult::Upgraded but was {:#?}",
                upgrade_result
            ),
        }

        assert_eq!(service_manager.service.service_data.pid, Some(2000));
        assert_eq!(
            service_manager.service.service_data.peer_id,
            Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?)
        );
        assert_eq!(service_manager.service.service_data.version, target_version);

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_not_be_required_if_target_is_less_than_current_version() -> Result<()> {
        let current_version = "0.2.0";
        let target_version = "0.1.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mock_service_control = MockServiceControl::new();
        let mock_metric_client = MockMetricClient::new();

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        let upgrade_result = service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        assert_matches!(upgrade_result, UpgradeResult::NotRequired);

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_downgrade_to_a_previous_version_if_force_is_used() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(always(), always())
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(2000));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        let upgrade_result = service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: true,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        match upgrade_result {
            UpgradeResult::Forced(old_version, new_version) => {
                assert_eq!(old_version, current_version);
                assert_eq!(new_version, target_version);
            }
            _ => panic!(
                "Expected UpgradeResult::Forced but was {:#?}",
                upgrade_result
            ),
        }

        assert_eq!(service_manager.service.service_data.pid, Some(2000));
        assert_eq!(
            service_manager.service.service_data.peer_id,
            Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?)
        );
        assert_eq!(service_manager.service.service_data.version, target_version);

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_upgrade_and_not_start_the_service() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(always(), always())
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(0)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(0)
            .returning(|_| ());
        mock_metric_client
            .expect_node_info()
            .times(0)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(0)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        let upgrade_result = service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: false,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        match upgrade_result {
            UpgradeResult::Upgraded(old_version, new_version) => {
                assert_eq!(old_version, current_version);
                assert_eq!(new_version, target_version);
            }
            _ => panic!(
                "Expected UpgradeResult::Upgraded but was {:#?}",
                upgrade_result
            ),
        }

        assert_eq!(service_manager.service.service_data.pid, None);
        assert_eq!(
            service_manager.service.service_data.peer_id,
            Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?)
        );
        assert_eq!(service_manager.service.service_data.version, target_version);
        assert_matches!(
            service_manager.service.service_data.status,
            ServiceStatus::Stopped
        );

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_return_upgraded_but_not_started_if_service_did_not_start() -> Result<()>
    {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let current_node_bin_str = current_node_bin.to_path_buf().to_string_lossy().to_string();

        let mut mock_service_control = MockServiceControl::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(always(), always())
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(move |_| {
                Err(ServiceControlError::ServiceProcessNotFound(
                    current_node_bin_str.clone(),
                ))
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(MockMetricClient::new()));
        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        let upgrade_result = service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        match upgrade_result {
            UpgradeResult::UpgradedButNotStarted(old_version, new_version, _) => {
                assert_eq!(old_version, current_version);
                assert_eq!(new_version, target_version);
            }
            _ => panic!(
                "Expected UpgradeResult::UpgradedButNotStarted but was {:#?}",
                upgrade_result
            ),
        }

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_upgrade_a_service_in_user_mode() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(true))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(true))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(always(), eq(true))
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(true))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(2000));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: None,
            user_mode: true,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        let upgrade_result = service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        match upgrade_result {
            UpgradeResult::Upgraded(old_version, new_version) => {
                assert_eq!(old_version, current_version);
                assert_eq!(new_version, target_version);
            }
            _ => panic!(
                "Expected UpgradeResult::Upgraded but was {:#?}",
                upgrade_result
            ),
        }

        assert_eq!(service_manager.service.service_data.pid, Some(2000));
        assert_eq!(
            service_manager.service.service_data.peer_id,
            Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?)
        );
        assert_eq!(service_manager.service.service_data.version, target_version);

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_retain_the_first_flag() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--root-dir"),
                        OsString::from("/var/antctl/services/antnode1"),
                        OsString::from("--log-output-dest"),
                        OsString::from("/var/log/antnode/antnode1"),
                        OsString::from("--first"),
                        OsString::from("--rewards-address"),
                        OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                        OsString::from("evm-arbitrum-one"),
                    ],
                    autostart: false,
                    contents: None,
                    environment: None,
                    label: "antnode1".parse()?,
                    program: current_node_bin.to_path_buf(),
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(100));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::ArbitrumOne,
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig {
                first: true,
                addrs: vec![],
                network_contacts_url: vec![],
                local: false,
                disable_mainnet_contacts: false,
                ignore_cache: false,
                bootstrap_cache_dir: None,
            },
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        assert!(service_manager.service.service_data.peers_args.first);

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_retain_the_peers_arg() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--root-dir"),
                        OsString::from("/var/antctl/services/antnode1"),
                        OsString::from("--log-output-dest"),
                        OsString::from("/var/log/antnode/antnode1"),
                        OsString::from("--peer"),
                        OsString::from(
                            "/ip4/127.0.0.1/tcp/8080/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
                        ),
                        OsString::from("--rewards-address"),
                        OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                        OsString::from("evm-arbitrum-one"),
                    ],
                    autostart: false,
                    contents: None,
                    environment: None,
                    label: "antnode1".parse()?,
                    program: current_node_bin.to_path_buf(),
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(100));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::ArbitrumOne,
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args:  InitialPeersConfig {
                first: false,
                addrs: vec![
                    "/ip4/127.0.0.1/tcp/8080/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
                        .parse()?,
                ],
                network_contacts_url: vec![],
                local: false,
                disable_mainnet_contacts: false,
                ignore_cache: false,
        bootstrap_cache_dir: None,
    },
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        assert!(!service_manager
            .service
            .service_data
            .peers_args
            .addrs
            .is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_retain_the_network_id_arg() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--root-dir"),
                        OsString::from("/var/antctl/services/antnode1"),
                        OsString::from("--log-output-dest"),
                        OsString::from("/var/log/antnode/antnode1"),
                        OsString::from("--network-id"),
                        OsString::from("5"),
                        OsString::from("--rewards-address"),
                        OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                        OsString::from("evm-arbitrum-one"),
                    ],
                    autostart: false,
                    contents: None,
                    environment: None,
                    label: "antnode1".parse()?,
                    program: current_node_bin.to_path_buf(),
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(100));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::ArbitrumOne,
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: Some(5),
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: Default::default(),
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        assert_eq!(service_manager.service.service_data.network_id, Some(5));

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_retain_the_local_flag() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--root-dir"),
                        OsString::from("/var/antctl/services/antnode1"),
                        OsString::from("--log-output-dest"),
                        OsString::from("/var/log/antnode/antnode1"),
                        OsString::from("--local"),
                        OsString::from("--rewards-address"),
                        OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                        OsString::from("evm-arbitrum-one"),
                    ],
                    autostart: false,
                    contents: None,
                    environment: None,
                    label: "antnode1".parse()?,
                    program: current_node_bin.to_path_buf(),
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(100));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::ArbitrumOne,
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig {
                first: false,
                addrs: vec![],
                network_contacts_url: vec![],
                local: true,
                disable_mainnet_contacts: false,
                ignore_cache: false,
                bootstrap_cache_dir: None,
            },
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        assert!(service_manager.service.service_data.peers_args.local);

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_retain_the_network_contacts_url_arg() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--root-dir"),
                        OsString::from("/var/antctl/services/antnode1"),
                        OsString::from("--log-output-dest"),
                        OsString::from("/var/log/antnode/antnode1"),
                        OsString::from("--network-contacts-url"),
                        OsString::from("http://localhost:8080/contacts.json,http://localhost:8081/contacts.json"),
                        OsString::from("--rewards-address"),
                        OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                        OsString::from("evm-arbitrum-one"),
                    ],
                    autostart: false,
                    contents: None,
                    environment: None,
                    label: "antnode1".parse()?,
                    program: current_node_bin.to_path_buf(),
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(100));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::ArbitrumOne,
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig {
                first: false,
                addrs: vec![],
                network_contacts_url: vec![
                    "http://localhost:8080/contacts.json".to_string(),
                    "http://localhost:8081/contacts.json".to_string(),
                ],
                local: false,
                disable_mainnet_contacts: false,
                ignore_cache: false,
                bootstrap_cache_dir: None,
            },
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        assert_eq!(
            service_manager
                .service
                .service_data
                .peers_args
                .network_contacts_url
                .len(),
            2
        );

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_retain_the_testnet_flag() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--root-dir"),
                        OsString::from("/var/antctl/services/antnode1"),
                        OsString::from("--log-output-dest"),
                        OsString::from("/var/log/antnode/antnode1"),
                        OsString::from("--testnet"),
                        OsString::from("--rewards-address"),
                        OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                        OsString::from("evm-arbitrum-one"),
                    ],
                    autostart: false,
                    contents: None,
                    environment: None,
                    label: "antnode1".parse()?,
                    program: current_node_bin.to_path_buf(),
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(100));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::ArbitrumOne,
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig {
                first: false,
                addrs: vec![],
                network_contacts_url: vec![],
                local: false,
                disable_mainnet_contacts: true,
                ignore_cache: false,
                bootstrap_cache_dir: None,
            },
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        assert!(
            service_manager
                .service
                .service_data
                .peers_args
                .disable_mainnet_contacts
        );

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_retain_the_ignore_cache_flag() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--root-dir"),
                        OsString::from("/var/antctl/services/antnode1"),
                        OsString::from("--log-output-dest"),
                        OsString::from("/var/log/antnode/antnode1"),
                        OsString::from("--ignore-cache"),
                        OsString::from("--rewards-address"),
                        OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                        OsString::from("evm-arbitrum-one"),
                    ],
                    autostart: false,
                    contents: None,
                    environment: None,
                    label: "antnode1".parse()?,
                    program: current_node_bin.to_path_buf(),
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(100));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::ArbitrumOne,
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig {
                first: false,
                addrs: vec![],
                network_contacts_url: vec![],
                local: false,
                disable_mainnet_contacts: false,
                ignore_cache: true,
                bootstrap_cache_dir: None,
            },
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        assert!(service_manager.service.service_data.peers_args.ignore_cache);

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_retain_the_custom_bootstrap_cache_path() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--root-dir"),
                        OsString::from("/var/antctl/services/antnode1"),
                        OsString::from("--log-output-dest"),
                        OsString::from("/var/log/antnode/antnode1"),
                        OsString::from("--bootstrap-cache-dir"),
                        OsString::from("/var/antctl/services/antnode1/bootstrap_cache"),
                        OsString::from("--rewards-address"),
                        OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                        OsString::from("evm-arbitrum-one"),
                    ],
                    autostart: false,
                    contents: None,
                    environment: None,
                    label: "antnode1".parse()?,
                    program: current_node_bin.to_path_buf(),
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(100));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::ArbitrumOne,
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig {
                first: false,
                addrs: vec![],
                network_contacts_url: vec![],
                local: false,
                disable_mainnet_contacts: false,
                ignore_cache: false,
                bootstrap_cache_dir: Some(PathBuf::from(
                    "/var/antctl/services/antnode1/bootstrap_cache",
                )),
            },
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        assert_eq!(
            service_manager
                .service
                .service_data
                .peers_args
                .bootstrap_cache_dir,
            Some(PathBuf::from(
                "/var/antctl/services/antnode1/bootstrap_cache"
            ))
        );

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_retain_the_upnp_flag() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--root-dir"),
                        OsString::from("/var/antctl/services/antnode1"),
                        OsString::from("--log-output-dest"),
                        OsString::from("/var/log/antnode/antnode1"),
                        OsString::from("--upnp"),
                        OsString::from("--rewards-address"),
                        OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                        OsString::from("evm-arbitrum-one"),
                    ],
                    autostart: false,
                    contents: None,
                    environment: None,
                    label: "antnode1".parse()?,
                    program: current_node_bin.to_path_buf(),
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(100));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::ArbitrumOne,
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: true,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        assert!(service_manager.service.service_data.upnp);

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_retain_the_log_format_flag() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--root-dir"),
                        OsString::from("/var/antctl/services/antnode1"),
                        OsString::from("--log-output-dest"),
                        OsString::from("/var/log/antnode/antnode1"),
                        OsString::from("--log-format"),
                        OsString::from("json"),
                        OsString::from("--rewards-address"),
                        OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                        OsString::from("evm-arbitrum-one"),
                    ],
                    autostart: false,
                    contents: None,
                    environment: None,
                    label: "antnode1".parse()?,
                    program: current_node_bin.to_path_buf(),
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(100));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::ArbitrumOne,
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: Some(LogFormat::Json),
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        assert!(service_manager.service.service_data.log_format.is_some());
        assert_eq!(
            service_manager.service.service_data.log_format,
            Some(LogFormat::Json)
        );

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_retain_the_home_network_flag() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--root-dir"),
                        OsString::from("/var/antctl/services/antnode1"),
                        OsString::from("--log-output-dest"),
                        OsString::from("/var/log/antnode/antnode1"),
                        OsString::from("--home-network"),
                        OsString::from("--rewards-address"),
                        OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                        OsString::from("evm-arbitrum-one"),
                    ],
                    autostart: false,
                    contents: None,
                    environment: None,
                    label: "antnode1".parse()?,
                    program: current_node_bin.to_path_buf(),
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(100));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::ArbitrumOne,
            home_network: true,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        assert!(service_manager.service.service_data.home_network);

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_retain_custom_node_ip() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--root-dir"),
                        OsString::from("/var/antctl/services/antnode1"),
                        OsString::from("--log-output-dest"),
                        OsString::from("/var/log/antnode/antnode1"),
                        OsString::from("--ip"),
                        OsString::from("192.168.1.1"),
                        OsString::from("--rewards-address"),
                        OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                        OsString::from("evm-arbitrum-one"),
                    ],
                    autostart: false,
                    contents: None,
                    environment: None,
                    label: "antnode1".parse()?,
                    program: current_node_bin.to_path_buf(),
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(100));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::ArbitrumOne,
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            number: 1,
            node_ip: Some(Ipv4Addr::new(192, 168, 1, 1)),
            node_port: None,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        assert_eq!(
            service_manager.service.service_data.node_ip,
            Some(Ipv4Addr::new(192, 168, 1, 1))
        );

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_retain_custom_node_ports() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--root-dir"),
                        OsString::from("/var/antctl/services/antnode1"),
                        OsString::from("--log-output-dest"),
                        OsString::from("/var/log/antnode/antnode1"),
                        OsString::from("--port"),
                        OsString::from("12000"),
                        OsString::from("--rewards-address"),
                        OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                        OsString::from("evm-arbitrum-one"),
                    ],
                    autostart: false,
                    contents: None,
                    environment: None,
                    label: "antnode1".parse()?,
                    program: current_node_bin.to_path_buf(),
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(100));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::ArbitrumOne,
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            number: 1,
            node_ip: None,
            node_port: Some(12000),
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        assert_eq!(service_manager.service.service_data.node_port, Some(12000));

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_retain_max_archived_log_files() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--root-dir"),
                        OsString::from("/var/antctl/services/antnode1"),
                        OsString::from("--log-output-dest"),
                        OsString::from("/var/log/antnode/antnode1"),
                        OsString::from("--max-archived-log-files"),
                        OsString::from("20"),
                        OsString::from("--rewards-address"),
                        OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                        OsString::from("evm-arbitrum-one"),
                    ],
                    autostart: false,
                    contents: None,
                    environment: None,
                    label: "antnode1".parse()?,
                    program: current_node_bin.to_path_buf(),
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(100));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: Some(20),
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
            evm_network: EvmNetwork::ArbitrumOne,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        assert_matches!(
            service_manager.service.service_data.max_archived_log_files,
            Some(20)
        );

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_retain_max_log_files() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--root-dir"),
                        OsString::from("/var/antctl/services/antnode1"),
                        OsString::from("--log-output-dest"),
                        OsString::from("/var/log/antnode/antnode1"),
                        OsString::from("--max-log-files"),
                        OsString::from("20"),
                        OsString::from("--rewards-address"),
                        OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                        OsString::from("evm-arbitrum-one"),
                    ],
                    autostart: false,
                    contents: None,
                    environment: None,
                    label: "antnode1".parse()?,
                    program: current_node_bin.to_path_buf(),
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(100));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: Some(20),
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
            evm_network: EvmNetwork::ArbitrumOne,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        assert_matches!(service_manager.service.service_data.max_log_files, Some(20));

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_retain_custom_metrics_ports() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--root-dir"),
                        OsString::from("/var/antctl/services/antnode1"),
                        OsString::from("--log-output-dest"),
                        OsString::from("/var/log/antnode/antnode1"),
                        OsString::from("--metrics-server-port"),
                        OsString::from("12000"),
                        OsString::from("--rewards-address"),
                        OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                        OsString::from("evm-arbitrum-one"),
                    ],
                    autostart: false,
                    contents: None,
                    environment: None,
                    label: "antnode1".parse()?,
                    program: current_node_bin.to_path_buf(),
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(100));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::ArbitrumOne,
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: Some(12000),
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        assert_eq!(
            service_manager.service.service_data.metrics_port,
            Some(12000)
        );

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_retain_custom_rpc_ports() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--root-dir"),
                        OsString::from("/var/antctl/services/antnode1"),
                        OsString::from("--log-output-dest"),
                        OsString::from("/var/log/antnode/antnode1"),
                        OsString::from("--metrics-server-port"),
                        OsString::from("12000"),
                        OsString::from("--rewards-address"),
                        OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                        OsString::from("evm-arbitrum-one"),
                    ],
                    autostart: false,
                    contents: None,
                    environment: None,
                    label: "antnode1".parse()?,
                    program: current_node_bin.to_path_buf(),
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(100));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::ArbitrumOne,
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: Some(12000),
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        assert_eq!(service_manager.service.service_data.rpc_socket_addr, None);

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_retain_auto_restart() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--root-dir"),
                        OsString::from("/var/antctl/services/antnode1"),
                        OsString::from("--log-output-dest"),
                        OsString::from("/var/log/antnode/antnode1"),
                        OsString::from("--rewards-address"),
                        OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                        OsString::from("evm-arbitrum-one"),
                    ],
                    autostart: true,
                    contents: None,
                    environment: None,
                    label: "antnode1".parse()?,
                    program: current_node_bin.to_path_buf(),
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(100));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: true,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::ArbitrumOne,
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager
            .upgrade(UpgradeOptions {
                auto_restart: true,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        assert!(service_manager.service.service_data.auto_restart,);

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_retain_evm_network_settings() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--root-dir"),
                        OsString::from("/var/antctl/services/antnode1"),
                        OsString::from("--log-output-dest"),
                        OsString::from("/var/log/antnode/antnode1"),
                        OsString::from("--rewards-address"),
                        OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                        OsString::from("evm-custom"),
                        OsString::from("--rpc-url"),
                        OsString::from("http://localhost:8545/"),
                        OsString::from("--payment-token-address"),
                        OsString::from("0x5FbDB2315678afecb367f032d93F642f64180aa3"),
                        OsString::from("--data-payments-address"),
                        OsString::from("0x8464135c8F25Da09e49BC8782676a84730C318bC"),
                    ],
                    autostart: true,
                    contents: None,
                    environment: None,
                    label: "antnode1".parse()?,
                    program: current_node_bin.to_path_buf(),
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(100));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: true,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),

            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager
            .upgrade(UpgradeOptions {
                auto_restart: true,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        assert!(service_manager.service.service_data.auto_restart,);

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_retain_the_rewards_address() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--root-dir"),
                        OsString::from("/var/antctl/services/antnode1"),
                        OsString::from("--log-output-dest"),
                        OsString::from("/var/log/antnode/antnode1"),
                        OsString::from("--rewards-address"),
                        OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                        OsString::from("evm-custom"),
                        OsString::from("--rpc-url"),
                        OsString::from("http://localhost:8545/"),
                        OsString::from("--payment-token-address"),
                        OsString::from("0x5FbDB2315678afecb367f032d93F642f64180aa3"),
                        OsString::from("--data-payments-address"),
                        OsString::from("0x8464135c8F25Da09e49BC8782676a84730C318bC"),
                    ],
                    autostart: true,
                    contents: None,
                    environment: None,
                    label: "antnode1".parse()?,
                    program: current_node_bin.to_path_buf(),
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(100));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: true,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),

            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager
            .upgrade(UpgradeOptions {
                auto_restart: true,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        assert!(service_manager.service.service_data.auto_restart,);

        Ok(())
    }

    #[tokio::test]
    async fn upgrade_should_use_dynamic_startup_delay_if_set() -> Result<()> {
        let current_version = "0.1.0";
        let target_version = "0.2.0";

        let tmp_data_dir = assert_fs::TempDir::new()?;
        let current_install_dir = tmp_data_dir.child("antnode_install");
        current_install_dir.create_dir_all()?;

        let current_node_bin = current_install_dir.child("antnode");
        current_node_bin.write_binary(b"fake antnode binary")?;
        let target_node_bin = tmp_data_dir.child("antnode");
        target_node_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        let mut mock_metric_client = MockMetricClient::new();

        // before binary upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(1000));
        mock_service_control
            .expect_stop()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // after binary upgrade
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--root-dir"),
                        OsString::from("/var/antctl/services/antnode1"),
                        OsString::from("--log-output-dest"),
                        OsString::from("/var/log/antnode/antnode1"),
                        OsString::from("--upnp"),
                        OsString::from("--rewards-address"),
                        OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                        OsString::from("evm-arbitrum-one"),
                    ],
                    autostart: false,
                    contents: None,
                    environment: None,
                    label: "antnode1".parse()?,
                    program: current_node_bin.to_path_buf(),
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // after service restart
        mock_service_control
            .expect_start()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(3000))
            .times(1)
            .returning(|_| ());
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf().clone()))
            .times(1)
            .returning(|_| Ok(100));
        mock_metric_client
            .expect_is_node_connected_to_network()
            .times(1)
            .returning(|_| Ok(()));
        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(|| {
                Ok(NodeInfo {
                    pid: 2000,
                    peer_id: PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from("/var/antctl/services/antnode1"),
                    log_path: PathBuf::from("/var/log/antnode/antnode1"),
                    version: target_version.to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::ArbitrumOne,
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: current_node_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: true,
            user: Some("ant".to_string()),
            user_mode: false,
            version: current_version.to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(mock_metric_client))
            .with_connection_timeout(Duration::from_secs(
                DEFAULT_NODE_STARTUP_CONNECTION_TIMEOUT_S,
            ));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager
            .upgrade(UpgradeOptions {
                auto_restart: false,
                env_variables: None,
                force: false,
                start_service: true,
                target_bin_path: target_node_bin.to_path_buf(),
                target_version: Version::parse(target_version).unwrap(),
            })
            .await?;

        Ok(())
    }

    #[tokio::test]
    async fn remove_should_remove_an_added_node() -> Result<()> {
        let temp_dir = assert_fs::TempDir::new()?;
        let log_dir = temp_dir.child("antnode1-logs");
        log_dir.create_dir_all()?;
        let data_dir = temp_dir.child("antnode1-data");
        data_dir.create_dir_all()?;
        let antnode_bin = data_dir.child("antnode");
        antnode_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: data_dir.to_path_buf(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: log_dir.to_path_buf(),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peers_args: InitialPeersConfig::default(),
            peer_id: None,
            pid: None,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: antnode_bin.to_path_buf(),
            status: ServiceStatus::Stopped,
            service_name: "antnode1".to_string(),
            version: "0.98.1".to_string(),
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
        };
        let service = NodeService::new(&mut service_data, Box::new(MockMetricClient::new()));
        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager.remove(false).await?;

        assert_matches!(
            service_manager.service.service_data.status,
            ServiceStatus::Removed
        );
        log_dir.assert(predicate::path::missing());
        data_dir.assert(predicate::path::missing());

        Ok(())
    }

    #[tokio::test]
    async fn remove_should_return_an_error_if_attempting_to_remove_a_running_node() -> Result<()> {
        let mut mock_service_control = MockServiceControl::new();
        mock_service_control
            .expect_get_process_pid()
            .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
            .times(1)
            .returning(|_| Ok(1000));

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(MockMetricClient::new()));
        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        let result = service_manager.remove(false).await;
        match result {
            Ok(_) => panic!("This test should result in an error"),
            Err(e) => assert_eq!(
                "The service(s) is already running: [\"antnode1\"]",
                e.to_string()
            ),
        }

        Ok(())
    }

    #[tokio::test]
    async fn remove_should_return_an_error_for_a_node_that_was_marked_running_but_was_not_actually_running(
    ) -> Result<()> {
        let temp_dir = assert_fs::TempDir::new()?;
        let log_dir = temp_dir.child("antnode1-logs");
        log_dir.create_dir_all()?;
        let data_dir = temp_dir.child("antnode1-data");
        data_dir.create_dir_all()?;
        let antnode_bin = data_dir.child("antnode");
        antnode_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        mock_service_control
            .expect_get_process_pid()
            .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
            .times(1)
            .returning(|_| {
                Err(ServiceError::ServiceProcessNotFound(
                    "Could not find process at '/var/antctl/services/antnode1/antnode'".to_string(),
                ))
            });

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peers_args: InitialPeersConfig::default(),
            pid: Some(1000),
            peer_id: Some(PeerId::from_str(
                "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
            )?),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Running,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(MockMetricClient::new()));
        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        let result = service_manager.remove(false).await;
        match result {
            Ok(_) => panic!("This test should result in an error"),
            Err(e) => assert_eq!(
                "The service status is not as expected. Expected: Running",
                e.to_string()
            ),
        }

        Ok(())
    }

    #[tokio::test]
    async fn remove_should_remove_an_added_node_and_keep_directories() -> Result<()> {
        let temp_dir = assert_fs::TempDir::new()?;
        let log_dir = temp_dir.child("antnode1-logs");
        log_dir.create_dir_all()?;
        let data_dir = temp_dir.child("antnode1-data");
        data_dir.create_dir_all()?;
        let antnode_bin = data_dir.child("antnode");
        antnode_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: data_dir.to_path_buf(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: log_dir.to_path_buf(),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            pid: None,
            peers_args: InitialPeersConfig::default(),
            peer_id: None,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: antnode_bin.to_path_buf(),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Stopped,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(MockMetricClient::new()));
        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager.remove(true).await?;

        assert_matches!(
            service_manager.service.service_data.status,
            ServiceStatus::Removed
        );
        log_dir.assert(predicate::path::is_dir());
        data_dir.assert(predicate::path::is_dir());

        Ok(())
    }

    #[tokio::test]
    async fn remove_should_remove_a_user_mode_service() -> Result<()> {
        let temp_dir = assert_fs::TempDir::new()?;
        let log_dir = temp_dir.child("antnode1-logs");
        log_dir.create_dir_all()?;
        let data_dir = temp_dir.child("antnode1-data");
        data_dir.create_dir_all()?;
        let antnode_bin = data_dir.child("antnode");
        antnode_bin.write_binary(b"fake antnode binary")?;

        let mut mock_service_control = MockServiceControl::new();
        mock_service_control
            .expect_uninstall()
            .with(eq("antnode1"), eq(true))
            .times(1)
            .returning(|_, _| Ok(()));

        let mut service_data = NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: data_dir.to_path_buf(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: log_dir.to_path_buf(),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            pid: None,
            peers_args: InitialPeersConfig::default(),
            peer_id: None,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: antnode_bin.to_path_buf(),
            status: ServiceStatus::Stopped,
            service_name: "antnode1".to_string(),
            upnp: false,
            user: None,
            user_mode: true,
            version: "0.98.1".to_string(),
        };
        let service = NodeService::new(&mut service_data, Box::new(MockMetricClient::new()));
        let mut service_manager = ServiceManager::new(
            service,
            Box::new(mock_service_control),
            VerbosityLevel::Normal,
        );

        service_manager.remove(false).await?;

        assert_matches!(
            service_manager.service.service_data.status,
            ServiceStatus::Removed
        );
        log_dir.assert(predicate::path::missing());
        data_dir.assert(predicate::path::missing());

        Ok(())
    }
}
