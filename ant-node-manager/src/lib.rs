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
pub mod batch_service_manager;
pub mod cmd;
pub mod config;
pub mod error;
pub mod helpers;
pub mod local;

use std::sync::Arc;

pub use {
    batch_service_manager::BatchServiceManager, error::Error, error::Result,
    service_manager::ServiceManager,
};

pub const DEFAULT_NODE_STARTUP_INTERVAL_MS: u64 = 10000;

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

use ant_service_management::NodeRegistryManager;
use ant_service_management::fs::{FileSystemActions, FileSystemClient};
use ant_service_management::metric::MetricsClient;
use ant_service_management::{
    NodeService, ServiceStateActions, ServiceStatus, control::ServiceControl,
};
use colored::Colorize;
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use tracing::debug;

#[tracing::instrument(skip(node_registry, service_control), err)]
pub async fn status_report(
    node_registry: &NodeRegistryManager,
    service_control: Arc<dyn ServiceControl + Send + Sync>,
    detailed_view: bool,
    output_json: bool,
    fail: bool,
    is_local_network: bool,
    save_registry: bool,
) -> Result<()> {
    refresh_node_registry(
        node_registry.clone(),
        Arc::clone(&service_control),
        !output_json,
        is_local_network,
        VerbosityLevel::Normal,
        save_registry,
    )
    .await?;

    if output_json {
        let json = serde_json::to_string_pretty(&node_registry.to_status_summary().await).map_err(
            |err| Error::JsonError {
                reason: format!("Failed to serialize status summary to JSON: {err}"),
            },
        )?;
        println!("{json}");
    } else if detailed_view {
        for node in node_registry.nodes.read().await.iter() {
            let node = node.read().await;
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
            let metrics_socket_addr = format!("http://localhost:{}", node.metrics_port);
            println!("Metrics Socket addr: {metrics_socket_addr}");
            println!("Listen Addresses: {:?}", node.listen_addr);
            println!(
                "PID: {}",
                node.pid.map_or("-".to_string(), |p| p.to_string())
            );
            let critical_failure = if let Some(failure) = &node.last_critical_failure {
                Some(failure.clone())
            } else if node.status == ServiceStatus::Stopped {
                let fs_client = FileSystemClient;
                fs_client
                    .critical_failure(&node.data_dir_path)
                    .ok()
                    .flatten()
            } else {
                None
            };
            if let Some(failure) = critical_failure {
                println!("Failure reason: {} ({})", failure.reason, failure.date_time);
            }
            println!("Data path: {}", node.data_dir_path.to_string_lossy());
            println!("Log path: {}", node.log_dir_path.to_string_lossy());
            println!("Bin path: {}", node.antnode_path.to_string_lossy());
            println!("Connected peers: {}", node.connected_peers);
            println!("Rewards address: {}", node.rewards_address);
            println!();
        }
    } else {
        println!(
            "{:<18} {:<52} {:<7} {:>15} {:<}",
            "Service Name", "Peer ID", "Status", "Connected Peers", "Failure"
        );

        let fs_client = FileSystemClient;
        for node in node_registry.nodes.read().await.iter() {
            let node = node.read().await;

            if node.status == ServiceStatus::Removed {
                continue;
            }
            let peer_id = node.peer_id.map_or("-".to_string(), |p| p.to_string());
            let critical_failure = if let Some(failure) = &node.last_critical_failure {
                failure.reason.clone()
            } else if node.status == ServiceStatus::Stopped {
                fs_client
                    .critical_failure(&node.data_dir_path)
                    .ok()
                    .flatten()
                    .map(|failure| failure.reason)
                    .unwrap_or_else(|| "-".to_string())
            } else {
                "-".to_string()
            };
            println!(
                "{:<18} {:<52} {:<7} {:>15} {:<}",
                node.service_name,
                peer_id,
                format_status(&node.status),
                node.connected_peers,
                critical_failure
            );
        }
    }

    if fail {
        let mut non_running_services = Vec::new();
        for node in node_registry.nodes.read().await.iter() {
            let node = node.read().await;
            if node.status != ServiceStatus::Running {
                non_running_services.push(node.service_name.clone());
            }
        }

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
/// know if any *particular* service is running or not. A full refresh uses the Metrics client to
/// connect to the node's Metrics service to determine things like the number of connected peers.
///
/// For a local network, the node paths are not unique, so we can't use that. We consider the node
/// running if we can connect to its metrics service; otherwise it is considered stopped.
#[tracing::instrument(skip(node_registry, service_control), err)]
pub async fn refresh_node_registry(
    node_registry: NodeRegistryManager,
    service_control: Arc<dyn ServiceControl + Send + Sync>,
    full_refresh: bool,
    is_local_network: bool,
    verbosity: VerbosityLevel,
    save_registry: bool,
) -> Result<()> {
    // This message is useful for users, but needs to be suppressed when a JSON output is
    // requested.

    info!("Refreshing the node registry");
    let pb = if verbosity != VerbosityLevel::Minimal {
        let total_nodes = node_registry.nodes.read().await.len() as u64;
        let pb = ProgressBar::new(total_nodes);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg} {spinner:.green} [{bar:40.cyan/blue}] ({percent}%)")
                .unwrap_or_else(|_e| {
                    // Fallback to default style if template fails
                    ProgressStyle::default_bar()
                })
                .progress_chars("#>-"),
        );
        pb.set_message("Refreshing the node registry");
        Some(pb)
    } else {
        None
    };

    let nodes = {
        let nodes_guard = node_registry.nodes.read().await;
        nodes_guard.clone()
    };

    let mut join_set = tokio::task::JoinSet::new();

    for node in nodes {
        let service_control = Arc::clone(&service_control);
        let pb = pb.clone();
        join_set.spawn(async move {
            let result = async {
                let (service_name, metrics_port) = {
                    let node_guard = node.read().await;
                    (node_guard.service_name.clone(), node_guard.metrics_port)
                };

                let metrics_client = MetricsClient::new(metrics_port);
                let service = NodeService::new(
                    Arc::clone(&node),
                    Box::new(FileSystemClient),
                    Box::new(metrics_client),
                );

                if is_local_network {
                    match service.metrics_action.get_node_metrics().await {
                        Ok(_) => {
                            debug!("Local node {service_name} is running",);
                            service.on_start(None, full_refresh).await?;
                        }
                        Err(_) => {
                            debug!("Failed to retrieve PID for local node {service_name}",);
                            service.on_stop().await?;
                        }
                    }
                } else {
                    match service_control.get_process_pid(&service.bin_path().await) {
                        Ok(pid) => {
                            debug!("{service_name} is running with PID {pid}",);
                            service.on_start(Some(pid), full_refresh).await?;
                        }
                        Err(_) => match service.status().await {
                            ServiceStatus::Added => {
                                debug!(
                                    "{service_name} has not been started since it was installed"
                                );
                            }
                            ServiceStatus::Removed => {
                                debug!("{service_name} has been removed");
                            }
                            _ => {
                                debug!("Failed to retrieve PID for {service_name}");
                                service.on_stop().await?;
                            }
                        },
                    }
                }

                Result::<()>::Ok(())
            }
            .await;

            if let Some(pb) = pb {
                pb.inc(1);
            }

            result
        });
    }

    let mut task_error = None;
    while let Some(join_result) = join_set.join_next().await {
        match join_result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                task_error.get_or_insert(err);
            }
            Err(join_err) => {
                task_error.get_or_insert_with(|| Error::BatchOperationFailed {
                    details: format!("Failed to refresh node registry task: {join_err}"),
                });
            }
        }
    }

    if let Some(pb) = pb {
        pb.finish_and_clear();
    }

    if let Some(err) = task_error {
        return Err(err);
    }

    if save_registry {
        node_registry.save().await?;
    }

    info!("Node registry refresh complete!");

    Ok(())
}

pub fn print_banner(text: &str) {
    let padding = 2;
    let text_width = text.len() + padding * 2;
    let border_chars = 2;
    let total_width = text_width + border_chars;
    let top_bottom = "═".repeat(total_width);

    println!("╔{top_bottom}╗");
    println!("║ {text:^text_width$} ║");
    println!("╚{top_bottom}╝");
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
