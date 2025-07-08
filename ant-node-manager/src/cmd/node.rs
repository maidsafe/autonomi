// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#![allow(clippy::too_many_arguments)]

use super::download_and_get_upgrade_bin_path;
use crate::{
    add_services::{
        add_node,
        config::{AddNodeServiceOptions, PortRange},
    },
    batch_service_manager::BatchServiceManager,
    config::{self, is_running_as_root},
    helpers::{download_and_extract_release, get_bin_version},
    print_banner, refresh_node_registry, status_report, VerbosityLevel,
};
use ant_bootstrap::InitialPeersConfig;
use ant_evm::{EvmNetwork, RewardsAddress};
use ant_logging::LogFormat;
use ant_releases::{AntReleaseRepoActions, ReleaseType};
use ant_service_management::{
    control::{ServiceControl, ServiceController},
    metric::MetricsClient,
    rpc::RpcClient,
    NodeRegistryManager, NodeService, NodeServiceData, ServiceStatus, UpgradeOptions,
};
use color_eyre::{eyre::eyre, Result};
use colored::Colorize;
use libp2p_identity::PeerId;
use semver::Version;
use std::{cmp::Ordering, io::Write, net::Ipv4Addr, path::PathBuf, str::FromStr, sync::Arc};
use tokio::sync::RwLock;
use tracing::debug;

/// Returns the added service names
pub async fn add(
    alpha: bool,
    auto_restart: bool,
    auto_set_nat_flags: bool,
    count: Option<u16>,
    data_dir_path: Option<PathBuf>,
    env_variables: Option<Vec<(String, String)>>,
    evm_network: Option<EvmNetwork>,
    log_dir_path: Option<PathBuf>,
    log_format: Option<LogFormat>,
    max_archived_log_files: Option<usize>,
    max_log_files: Option<usize>,
    metrics_port: Option<PortRange>,
    network_id: Option<u8>,
    node_ip: Option<Ipv4Addr>,
    node_port: Option<PortRange>,
    node_registry: NodeRegistryManager,
    mut init_peers_config: InitialPeersConfig,
    relay: bool,
    reachability_check: bool,
    rewards_address: RewardsAddress,
    rpc_address: Option<Ipv4Addr>,
    rpc_port: Option<PortRange>,
    src_path: Option<PathBuf>,
    no_upnp: bool,
    url: Option<String>,
    user: Option<String>,
    version: Option<String>,
    verbosity: VerbosityLevel,
    write_older_cache_files: bool,
) -> Result<Vec<String>> {
    let user_mode = !is_running_as_root();

    if verbosity != VerbosityLevel::Minimal {
        print_banner("Add Antnode Services");
        println!("{} service(s) to be added", count.unwrap_or(1));
    }

    let service_manager = ServiceController {};
    let service_user = if user_mode {
        None
    } else {
        let service_user = user.unwrap_or_else(|| "ant".to_string());
        service_manager.create_service_user(&service_user)?;
        Some(service_user)
    };

    let service_data_dir_path =
        config::get_service_data_dir_path(data_dir_path, service_user.clone())?;
    let service_log_dir_path =
        config::get_service_log_dir_path(ReleaseType::AntNode, log_dir_path, service_user.clone())?;
    let bootstrap_cache_dir = if let Some(user) = &service_user {
        Some(config::get_bootstrap_cache_owner_path(user)?)
    } else {
        None
    };

    let release_repo = <dyn AntReleaseRepoActions>::default_config();

    let (antnode_src_path, version) = if let Some(path) = src_path.clone() {
        let version = get_bin_version(&path)?;
        (path, version)
    } else {
        download_and_extract_release(
            ReleaseType::AntNode,
            url.clone(),
            version,
            &*release_repo,
            verbosity,
            None,
        )
        .await?
    };

    debug!("Parsing peers from PeersArgs");

    init_peers_config
        .addrs
        .extend(InitialPeersConfig::read_bootstrap_addr_from_env());
    init_peers_config.bootstrap_cache_dir = bootstrap_cache_dir;

    let options = AddNodeServiceOptions {
        alpha,
        auto_restart,
        auto_set_nat_flags,
        count,
        delete_antnode_src: src_path.is_none(),
        evm_network: evm_network.unwrap_or(EvmNetwork::ArbitrumOne),
        env_variables,
        relay,
        log_format,
        max_archived_log_files,
        max_log_files,
        metrics_port,
        network_id,
        node_ip,
        node_port,
        init_peers_config,
        reachability_check,
        rewards_address,
        rpc_address,
        rpc_port,
        antnode_src_path,
        antnode_dir_path: service_data_dir_path.clone(),
        service_data_dir_path,
        service_log_dir_path,
        no_upnp,
        user: service_user,
        user_mode,
        version,
        write_older_cache_files,
    };
    info!("Adding node service(s)");
    let added_services_names =
        add_node(options, node_registry.clone(), &service_manager, verbosity).await?;

    node_registry.save().await?;
    debug!("Node registry saved");

    Ok(added_services_names)
}

pub async fn balance(
    peer_ids: Vec<String>,
    node_registry: NodeRegistryManager,
    service_names: Vec<String>,
    verbosity: VerbosityLevel,
) -> Result<()> {
    if verbosity != VerbosityLevel::Minimal {
        print_banner("Reward Balances");
    }

    refresh_node_registry(
        node_registry.clone(),
        &ServiceController {},
        verbosity != VerbosityLevel::Minimal,
        false,
        verbosity,
    )
    .await?;

    let services_for_ops = get_services_for_ops(&node_registry, peer_ids, service_names).await?;
    if services_for_ops.is_empty() {
        info!("Services for ops is empty, cannot obtain the balance");
        // This could be the case if all services are at `Removed` status.
        println!("No balances to display");
        return Ok(());
    }
    debug!("Obtaining balances for {} services", services_for_ops.len());

    for node in services_for_ops {
        let node = node.read().await;
        // TODO: remove this as we have no way to know the reward balance of nodes since EVM payments!
        println!("{}: {}", node.service_name, 0,);
    }
    Ok(())
}

pub async fn remove(
    keep_directories: bool,
    peer_ids: Vec<String>,
    node_registry: NodeRegistryManager,
    service_names: Vec<String>,
    verbosity: VerbosityLevel,
) -> Result<()> {
    if verbosity != VerbosityLevel::Minimal {
        print_banner("Remove Antnode Services");
    }
    info!("Removing antnode services with keep_dirs=({keep_directories}) for: {peer_ids:?}, {service_names:?}");

    refresh_node_registry(
        node_registry.clone(),
        &ServiceController {},
        verbosity != VerbosityLevel::Minimal,
        false,
        verbosity,
    )
    .await?;

    let services_for_ops = get_services_for_ops(&node_registry, peer_ids, service_names).await?;
    if services_for_ops.is_empty() {
        info!("Services for ops is empty, no services were eligible for removal");
        // This could be the case if all services are at `Removed` status.
        if verbosity != VerbosityLevel::Minimal {
            println!("No services were eligible for removal");
        }
        return Ok(());
    }

    let batch_manager =
        get_batch_manager_from_service_data(node_registry.clone(), services_for_ops, verbosity)
            .await;
    batch_manager.remove_all(keep_directories).await
}

pub async fn reset(
    force: bool,
    node_registry: NodeRegistryManager,
    verbosity: VerbosityLevel,
) -> Result<()> {
    if verbosity != VerbosityLevel::Minimal {
        print_banner("Reset Antnode Services");
    }
    info!("Resetting all antnode services, with force={force}");

    if !force {
        println!("WARNING: all antnode services, data, and logs will be removed.");
        println!("Do you wish to proceed? [y/n]");
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if input.trim().to_lowercase() != "y" {
            println!("Reset aborted");
            return Ok(());
        }
    }

    stop(None, node_registry.clone(), vec![], vec![], verbosity).await?;
    remove(false, vec![], node_registry, vec![], verbosity).await?;

    // Due the possibility of repeated runs of the `reset` command, we need to check for the
    // existence of this file before attempting to delete it, since `remove_file` will return an
    // error if the file doesn't exist. On Windows this has been observed to happen.
    let node_registry_path = config::get_node_registry_path()?;
    if node_registry_path.exists() {
        info!("Removing node registry file: {node_registry_path:?}");
        std::fs::remove_file(node_registry_path)?;
    }

    Ok(())
}

pub async fn start(
    fixed_interval: u64,
    node_registry: NodeRegistryManager,
    peer_ids: Vec<String>,
    service_names: Vec<String>,
    verbosity: VerbosityLevel,
) -> Result<()> {
    if verbosity != VerbosityLevel::Minimal {
        print_banner("Start Antnode Services (Batch Mode)");
    }
    info!("Starting antnode services in batch for: {peer_ids:?}, {service_names:?}");

    refresh_node_registry(
        node_registry.clone(),
        &ServiceController {},
        verbosity != VerbosityLevel::Minimal,
        false,
        verbosity,
    )
    .await?;

    let services_for_ops = get_services_for_ops(&node_registry, peer_ids, service_names).await?;
    if services_for_ops.is_empty() {
        info!("No services are eligible to be started");
        if verbosity != VerbosityLevel::Minimal {
            println!("No services were eligible to be started");
        }
        return Ok(());
    }
    let batch_manager =
        get_batch_manager_from_service_data(node_registry.clone(), services_for_ops, verbosity)
            .await;
    batch_manager.start_all(fixed_interval).await
}

pub async fn status(
    details: bool,
    fail: bool,
    json: bool,
    node_registry: NodeRegistryManager,
) -> Result<()> {
    if !node_registry.nodes.read().await.is_empty() {
        if !json && !details {
            print_banner("Antnode Services");
        }
        status_report(
            &node_registry,
            &ServiceController {},
            details,
            json,
            fail,
            false,
        )
        .await?;
        node_registry.save().await?;
    }
    Ok(())
}

pub async fn stop(
    interval: Option<u64>,
    node_registry: NodeRegistryManager,
    peer_ids: Vec<String>,
    service_names: Vec<String>,
    verbosity: VerbosityLevel,
) -> Result<()> {
    if verbosity != VerbosityLevel::Minimal {
        print_banner("Stop Antnode Services");
    }
    info!("Stopping antnode services for: {peer_ids:?}, {service_names:?}");

    refresh_node_registry(
        node_registry.clone(),
        &ServiceController {},
        verbosity != VerbosityLevel::Minimal,
        false,
        verbosity,
    )
    .await?;

    let services_for_ops = get_services_for_ops(&node_registry, peer_ids, service_names).await?;
    if services_for_ops.is_empty() {
        info!("No services are eligible to be stopped");
        // This could be the case if all services are at `Removed` status.
        if verbosity != VerbosityLevel::Minimal {
            println!("No services were eligible to be stopped");
        }
        return Ok(());
    }

    let batch_manager =
        get_batch_manager_from_service_data(node_registry.clone(), services_for_ops, verbosity)
            .await;
    batch_manager.stop_all(interval).await
}

pub async fn upgrade(
    do_not_start: bool,
    custom_bin_path: Option<PathBuf>,
    force: bool,
    fixed_interval: u64,
    node_registry: NodeRegistryManager,
    peer_ids: Vec<String>,
    provided_env_variables: Option<Vec<(String, String)>>,
    service_names: Vec<String>,
    url: Option<String>,
    version: Option<String>,
    verbosity: VerbosityLevel,
) -> Result<()> {
    // In the case of a custom binary, we want to force the use of it. Regardless of its version
    // number, the user has probably built it for some special case. They may have not used the
    // `--force` flag; if they didn't, we can just do that for them here.
    let use_force = force || custom_bin_path.is_some();

    if verbosity != VerbosityLevel::Minimal {
        print_banner("Upgrade Antnode Services");
    }
    info!(
        "Upgrading antnode services with use_force={use_force} for: {peer_ids:?}, {service_names:?}"
    );

    let (upgrade_bin_path, target_version) = download_and_get_upgrade_bin_path(
        custom_bin_path.clone(),
        ReleaseType::AntNode,
        url,
        version,
        verbosity,
    )
    .await?;

    refresh_node_registry(
        node_registry.clone(),
        &ServiceController {},
        verbosity != VerbosityLevel::Minimal,
        false,
        verbosity,
    )
    .await?;

    if let Some(node) = node_registry.nodes.read().await.first() {
        let node = node.read().await;
        debug!("listen addresses for nodes[0]: {:?}", node.listen_addr);
    } else {
        debug!("There are no nodes currently added or active");
    }

    if !use_force {
        let mut node_versions = Vec::new();

        for node in node_registry.nodes.read().await.iter() {
            let node = node.read().await;
            let version = Version::parse(&node.version)
                .map_err(|_| eyre!("Failed to parse Version for node {}", node.service_name))?;
            node_versions.push(version);
        }

        let any_nodes_need_upgraded = node_versions
            .iter()
            .any(|current_version| current_version < &target_version);
        if !any_nodes_need_upgraded {
            info!("All nodes are at the latest version, no upgrade required.");
            if verbosity != VerbosityLevel::Minimal {
                println!("{} All nodes are at the latest version", "✓".green());
            }
            return Ok(());
        }
    }
    let env_variables = if provided_env_variables.is_some() {
        provided_env_variables.clone()
    } else {
        node_registry.environment_variables.read().await.clone()
    };
    let options = UpgradeOptions {
        auto_restart: false,
        env_variables: env_variables.clone(),
        force: use_force,
        start_service: !do_not_start,
        target_bin_path: upgrade_bin_path.clone(),
        target_version: target_version.clone(),
    };

    let services_for_ops = get_services_for_ops(&node_registry, peer_ids, service_names).await?;
    trace!("services_for_ops len: {}", services_for_ops.len());

    let batch_manager =
        get_batch_manager_from_service_data(node_registry.clone(), services_for_ops, verbosity)
            .await;
    batch_manager.upgrade_all(options, fixed_interval).await
}

/// Ensure n nodes are running by stopping nodes or by adding and starting nodes if required.
///
/// The arguments here are mostly mirror those used in `add`.
pub async fn maintain_n_running_nodes(
    alpha: bool,
    auto_restart: bool,
    auto_set_nat_flags: bool,
    max_nodes_to_run: u16,
    data_dir_path: Option<PathBuf>,
    env_variables: Option<Vec<(String, String)>>,
    evm_network: Option<EvmNetwork>,
    log_dir_path: Option<PathBuf>,
    log_format: Option<LogFormat>,
    max_archived_log_files: Option<usize>,
    max_log_files: Option<usize>,
    metrics_port: Option<PortRange>,
    network_id: Option<u8>,
    node_ip: Option<Ipv4Addr>,
    node_port: Option<PortRange>,
    node_registry: NodeRegistryManager,
    peers_args: InitialPeersConfig,
    reachability_check: bool,
    relay: bool,
    rewards_address: RewardsAddress,
    rpc_address: Option<Ipv4Addr>,
    rpc_port: Option<PortRange>,
    src_path: Option<PathBuf>,
    url: Option<String>,
    no_upnp: bool,
    user: Option<String>,
    version: Option<String>,
    verbosity: VerbosityLevel,
    start_node_interval: u64,
    write_older_cache_files: bool,
) -> Result<()> {
    let mut running_nodes = Vec::new();

    for node in node_registry.nodes.read().await.iter() {
        let node = node.read().await;
        if node.status == ServiceStatus::Running {
            running_nodes.push(node.service_name.clone());
        }
    }

    let running_count = running_nodes.len();
    let target_count = max_nodes_to_run as usize;

    info!(
        "Current running nodes: {}, Target: {}",
        running_count, target_count
    );

    match running_count.cmp(&target_count) {
        Ordering::Greater => {
            let to_stop_count = running_count - target_count;
            let services_to_stop = running_nodes
                .into_iter()
                .rev() // Stop the oldest nodes first
                .take(to_stop_count)
                .collect::<Vec<_>>();

            info!(
                "Stopping {} excess nodes: {:?}",
                to_stop_count, services_to_stop
            );
            stop(
                None,
                node_registry.clone(),
                vec![],
                services_to_stop,
                verbosity,
            )
            .await?;
        }
        Ordering::Less => {
            let to_start_count = target_count - running_count;
            let mut inactive_nodes = Vec::new();
            for node in node_registry.nodes.read().await.iter() {
                let node = node.read().await;
                if node.status == ServiceStatus::Stopped || node.status == ServiceStatus::Added {
                    inactive_nodes.push(node.service_name.clone());
                }
            }

            info!("Inactive nodes available: {}", inactive_nodes.len());

            if to_start_count <= inactive_nodes.len() {
                let nodes_to_start = inactive_nodes.into_iter().take(to_start_count).collect();
                info!(
                    "Starting {} existing inactive nodes: {:?}",
                    to_start_count, nodes_to_start
                );
                start(
                    start_node_interval,
                    node_registry.clone(),
                    vec![],
                    nodes_to_start,
                    verbosity,
                )
                .await?;
            } else {
                let to_add_count = to_start_count - inactive_nodes.len();
                info!(
                    "Adding {} new nodes and starting all {} inactive nodes",
                    to_add_count,
                    inactive_nodes.len()
                );

                let ports_to_use = match node_port {
                    Some(PortRange::Single(port)) => vec![port],
                    Some(PortRange::Range(start, end)) => {
                        (start..=end).take(to_add_count).collect()
                    }
                    None => vec![],
                };

                for (i, port) in ports_to_use.into_iter().enumerate() {
                    let added_service = add(
                        alpha,
                        auto_restart,
                        auto_set_nat_flags,
                        Some(1),
                        data_dir_path.clone(),
                        env_variables.clone(),
                        evm_network.clone(),
                        log_dir_path.clone(),
                        log_format,
                        max_archived_log_files,
                        max_log_files,
                        metrics_port.clone(),
                        network_id,
                        node_ip,
                        Some(PortRange::Single(port)),
                        node_registry.clone(),
                        peers_args.clone(),
                        relay,
                        reachability_check,
                        rewards_address,
                        rpc_address,
                        rpc_port.clone(),
                        src_path.clone(),
                        no_upnp,
                        url.clone(),
                        user.clone(),
                        version.clone(),
                        verbosity,
                        write_older_cache_files,
                    )
                    .await?;

                    if i == 0 {
                        start(
                            start_node_interval,
                            node_registry.clone(),
                            vec![],
                            added_service,
                            verbosity,
                        )
                        .await?;
                    }
                }

                if !inactive_nodes.is_empty() {
                    start(
                        start_node_interval,
                        node_registry.clone(),
                        vec![],
                        inactive_nodes,
                        verbosity,
                    )
                    .await?;
                }
            }
        }
        Ordering::Equal => {
            info!(
                "Current node count ({}) matches target ({}). No action needed.",
                running_count, target_count
            );
        }
    }

    // Verify final state
    let mut final_running_count = 0;
    for node in node_registry.nodes.read().await.iter() {
        let node_read = node.read().await;
        if node_read.status == ServiceStatus::Running {
            final_running_count += 1;
        }
    }

    if final_running_count != target_count {
        warn!("Failed to reach target node count. Expected {target_count}, but got {final_running_count}");
    }

    Ok(())
}

async fn get_services_for_ops(
    node_registry: &NodeRegistryManager,
    peer_ids: Vec<String>,
    service_names: Vec<String>,
) -> Result<Vec<Arc<RwLock<NodeServiceData>>>> {
    let mut services = Vec::new();

    if service_names.is_empty() && peer_ids.is_empty() {
        for node in node_registry.nodes.read().await.iter() {
            if node.read().await.status != ServiceStatus::Removed {
                services.push(node.clone());
            }
        }
    } else {
        for name in &service_names {
            let mut found_service_with_name = false;
            for node in node_registry.nodes.read().await.iter() {
                let node_read = node.read().await;
                if node_read.service_name == *name && node_read.status != ServiceStatus::Removed {
                    {
                        services.push(node.clone());
                        found_service_with_name = true;
                        break;
                    }
                }
            }

            if !found_service_with_name {
                error!("No service named '{name}'");
                return Err(eyre!(format!("No service named '{name}'")));
            }
        }

        for peer_id_str in &peer_ids {
            let mut found_service_with_peer_id = false;
            let given_peer_id = PeerId::from_str(peer_id_str)
                .map_err(|_| eyre!(format!("Error parsing PeerId: '{peer_id_str}'")))?;
            for node in node_registry.nodes.read().await.iter() {
                let node_read = node.read().await;
                if let Some(peer_id) = node_read.peer_id {
                    if peer_id == given_peer_id && node_read.status != ServiceStatus::Removed {
                        services.push(node.clone());
                        found_service_with_peer_id = true;
                        break;
                    }
                }
            }
            if !found_service_with_peer_id {
                error!("Could not find node with peer id: '{given_peer_id:?}'");
                return Err(eyre!(format!(
                    "Could not find node with peer ID '{given_peer_id}'",
                )));
            }
        }
    }

    Ok(services)
}

async fn get_batch_manager_from_service_data(
    node_registry: NodeRegistryManager,

    service_data: Vec<Arc<RwLock<NodeServiceData>>>,
    verbosity: VerbosityLevel,
) -> BatchServiceManager<NodeService> {
    let mut services = Vec::new();
    for node in service_data {
        let rpc_client = RpcClient::from_socket_addr(node.read().await.rpc_socket_addr);
        let metrics_client = MetricsClient::new(
            node.read()
                .await
                .metrics_port
                .expect("TEMP: metrics port is required"),
        );
        let service =
            NodeService::new(node.clone(), Box::new(rpc_client), Box::new(metrics_client));
        services.push(service);
    }
    BatchServiceManager::new(
        services,
        Box::new(ServiceController {}),
        node_registry,
        verbosity,
    )
    .await
}
