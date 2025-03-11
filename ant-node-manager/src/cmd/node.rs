// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#![allow(clippy::too_many_arguments)]

use super::{download_and_get_upgrade_bin_path, print_upgrade_summary};
use crate::{
    add_services::{
        add_node,
        config::{AddNodeServiceOptions, PortRange},
    },
    config::{self, is_running_as_root},
    helpers::{download_and_extract_release, get_bin_version},
    print_banner, refresh_node_registry, status_report, ServiceManager, VerbosityLevel,
};
use ant_bootstrap::InitialPeersConfig;
use ant_evm::{EvmNetwork, RewardsAddress};
use ant_logging::LogFormat;
use ant_releases::{AntReleaseRepoActions, ReleaseType};
use ant_service_management::{
    control::{ServiceControl, ServiceController},
    rpc::RpcClient,
    NodeRegistry, NodeService, ServiceStateActions, ServiceStatus, UpgradeOptions, UpgradeResult,
};
use color_eyre::{eyre::eyre, Help, Result};
use colored::Colorize;
use libp2p_identity::PeerId;
use semver::Version;
use std::{cmp::Ordering, io::Write, net::Ipv4Addr, path::PathBuf, str::FromStr, time::Duration};
use tracing::debug;

/// Returns the added service names
pub async fn add(
    auto_restart: bool,
    auto_set_nat_flags: bool,
    count: Option<u16>,
    data_dir_path: Option<PathBuf>,
    enable_metrics_server: bool,
    env_variables: Option<Vec<(String, String)>>,
    evm_network: Option<EvmNetwork>,
    relay: bool,
    log_dir_path: Option<PathBuf>,
    log_format: Option<LogFormat>,
    max_archived_log_files: Option<usize>,
    max_log_files: Option<usize>,
    metrics_port: Option<PortRange>,
    network_id: Option<u8>,
    node_ip: Option<Ipv4Addr>,
    node_port: Option<PortRange>,
    mut init_peers_config: InitialPeersConfig,
    rewards_address: RewardsAddress,
    rpc_address: Option<Ipv4Addr>,
    rpc_port: Option<PortRange>,
    src_path: Option<PathBuf>,
    no_upnp: bool,
    url: Option<String>,
    user: Option<String>,
    version: Option<String>,
    verbosity: VerbosityLevel,
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

    let mut node_registry = NodeRegistry::load(&config::get_node_registry_path()?)?;
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
        .extend(InitialPeersConfig::read_addr_from_env());
    init_peers_config.bootstrap_cache_dir = bootstrap_cache_dir;

    let options = AddNodeServiceOptions {
        auto_restart,
        auto_set_nat_flags,
        count,
        delete_antnode_src: src_path.is_none(),
        enable_metrics_server,
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
    };
    info!("Adding node service(s)");
    let added_services_names =
        add_node(options, &mut node_registry, &service_manager, verbosity).await?;

    node_registry.save()?;
    debug!("Node registry saved");

    Ok(added_services_names)
}

pub async fn balance(
    peer_ids: Vec<String>,
    service_names: Vec<String>,
    verbosity: VerbosityLevel,
) -> Result<()> {
    if verbosity != VerbosityLevel::Minimal {
        print_banner("Reward Balances");
    }

    let mut node_registry = NodeRegistry::load(&config::get_node_registry_path()?)?;
    refresh_node_registry(
        &mut node_registry,
        &ServiceController {},
        verbosity != VerbosityLevel::Minimal,
        false,
        false,
    )
    .await?;

    let service_indices = get_services_for_ops(&node_registry, peer_ids, service_names)?;
    if service_indices.is_empty() {
        info!("Service indices is empty, cannot obtain the balance");
        // This could be the case if all services are at `Removed` status.
        println!("No balances to display");
        return Ok(());
    }
    debug!("Obtaining balances for {} services", service_indices.len());

    for &index in &service_indices {
        let node = &mut node_registry.nodes[index];
        let rpc_client = RpcClient::from_socket_addr(node.rpc_socket_addr);
        let service = NodeService::new(node, Box::new(rpc_client));
        // TODO: remove this as we have no way to know the reward balance of nodes since EVM payments!
        println!("{}: {}", service.service_data.service_name, 0,);
    }
    Ok(())
}

pub async fn remove(
    keep_directories: bool,
    peer_ids: Vec<String>,
    service_names: Vec<String>,
    verbosity: VerbosityLevel,
) -> Result<()> {
    if verbosity != VerbosityLevel::Minimal {
        print_banner("Remove Antnode Services");
    }
    info!("Removing antnode services with keep_dirs=({keep_directories}) for: {peer_ids:?}, {service_names:?}");

    let mut node_registry = NodeRegistry::load(&config::get_node_registry_path()?)?;
    refresh_node_registry(
        &mut node_registry,
        &ServiceController {},
        verbosity != VerbosityLevel::Minimal,
        false,
        false,
    )
    .await?;

    let service_indices = get_services_for_ops(&node_registry, peer_ids, service_names)?;
    if service_indices.is_empty() {
        info!("Service indices is empty, no services were eligible for removal");
        // This could be the case if all services are at `Removed` status.
        if verbosity != VerbosityLevel::Minimal {
            println!("No services were eligible for removal");
        }
        return Ok(());
    }

    let mut failed_services = Vec::new();
    for &index in &service_indices {
        let node = &mut node_registry.nodes[index];
        let rpc_client = RpcClient::from_socket_addr(node.rpc_socket_addr);
        let service = NodeService::new(node, Box::new(rpc_client));
        let mut service_manager =
            ServiceManager::new(service, Box::new(ServiceController {}), verbosity);
        match service_manager.remove(keep_directories).await {
            Ok(()) => {
                debug!("Removed service {}", node.service_name);
                node_registry.save()?;
            }
            Err(err) => {
                error!("Failed to remove service {}: {err}", node.service_name);
                failed_services.push((node.service_name.clone(), err.to_string()))
            }
        }
    }

    summarise_any_failed_ops(failed_services, "remove", verbosity)
}

pub async fn reset(force: bool, verbosity: VerbosityLevel) -> Result<()> {
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

    stop(None, vec![], vec![], verbosity).await?;
    remove(false, vec![], vec![], verbosity).await?;

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
    connection_timeout_s: u64,
    fixed_interval: Option<u64>,
    peer_ids: Vec<String>,
    service_names: Vec<String>,
    verbosity: VerbosityLevel,
) -> Result<()> {
    if verbosity != VerbosityLevel::Minimal {
        print_banner("Start Antnode Services");
    }
    info!("Starting antnode services for: {peer_ids:?}, {service_names:?}");

    let mut node_registry = NodeRegistry::load(&config::get_node_registry_path()?)?;
    refresh_node_registry(
        &mut node_registry,
        &ServiceController {},
        verbosity != VerbosityLevel::Minimal,
        false,
        false,
    )
    .await?;

    let service_indices = get_services_for_ops(&node_registry, peer_ids, service_names)?;
    if service_indices.is_empty() {
        info!("No services are eligible to be started");
        // This could be the case if all services are at `Removed` status.
        if verbosity != VerbosityLevel::Minimal {
            println!("No services were eligible to be started");
        }
        return Ok(());
    }

    let mut failed_services = Vec::new();
    for &index in &service_indices {
        let node = &mut node_registry.nodes[index];
        let rpc_client = RpcClient::from_socket_addr(node.rpc_socket_addr);

        let service = NodeService::new(node, Box::new(rpc_client));

        // set dynamic startup delay if fixed_interval is not set
        let service = if fixed_interval.is_none() {
            service.with_connection_timeout(Duration::from_secs(connection_timeout_s))
        } else {
            service
        };

        let mut service_manager =
            ServiceManager::new(service, Box::new(ServiceController {}), verbosity);
        if service_manager.service.status() != ServiceStatus::Running {
            // It would be possible here to check if the service *is* running and then just
            // continue without applying the delay. The reason for not doing so is because when
            // `start` is called below, the user will get a message to say the service was already
            // started, which I think is useful behaviour to retain.
            if let Some(interval) = fixed_interval {
                debug!("Sleeping for {} milliseconds", interval);
                std::thread::sleep(std::time::Duration::from_millis(interval));
            }
        }
        match service_manager.start().await {
            Ok(start_duration) => {
                debug!(
                    "Started service {} in {start_duration:?}",
                    node.service_name
                );

                node_registry.save()?;
            }
            Err(err) => {
                error!("Failed to start service {}: {err}", node.service_name);
                failed_services.push((node.service_name.clone(), err.to_string()))
            }
        }
    }

    summarise_any_failed_ops(failed_services, "start", verbosity)
}

pub async fn status(details: bool, fail: bool, json: bool) -> Result<()> {
    let mut node_registry = NodeRegistry::load(&config::get_node_registry_path()?)?;
    if !node_registry.nodes.is_empty() {
        if !json && !details {
            print_banner("Antnode Services");
        }
        status_report(
            &mut node_registry,
            &ServiceController {},
            details,
            json,
            fail,
            false,
        )
        .await?;
        node_registry.save()?;
    }
    Ok(())
}

pub async fn stop(
    interval: Option<u64>,
    peer_ids: Vec<String>,
    service_names: Vec<String>,
    verbosity: VerbosityLevel,
) -> Result<()> {
    if verbosity != VerbosityLevel::Minimal {
        print_banner("Stop Antnode Services");
    }
    info!("Stopping antnode services for: {peer_ids:?}, {service_names:?}");

    let mut node_registry = NodeRegistry::load(&config::get_node_registry_path()?)?;
    refresh_node_registry(
        &mut node_registry,
        &ServiceController {},
        verbosity != VerbosityLevel::Minimal,
        false,
        false,
    )
    .await?;

    let service_indices = get_services_for_ops(&node_registry, peer_ids, service_names)?;
    if service_indices.is_empty() {
        info!("Service indices is empty, no services were eligible to be stopped");
        // This could be the case if all services are at `Removed` status.
        if verbosity != VerbosityLevel::Minimal {
            println!("No services were eligible to be stopped");
        }
        return Ok(());
    }

    let mut failed_services = Vec::new();
    for &index in &service_indices {
        let node = &mut node_registry.nodes[index];
        let rpc_client = RpcClient::from_socket_addr(node.rpc_socket_addr);
        let service = NodeService::new(node, Box::new(rpc_client));
        let mut service_manager =
            ServiceManager::new(service, Box::new(ServiceController {}), verbosity);

        if service_manager.service.status() == ServiceStatus::Running {
            if let Some(interval) = interval {
                debug!("Sleeping for {} milliseconds", interval);
                std::thread::sleep(std::time::Duration::from_millis(interval));
            }
        }
        match service_manager.stop().await {
            Ok(()) => {
                debug!("Stopped service {}", node.service_name);
                node_registry.save()?;
            }
            Err(err) => {
                error!("Failed to stop service {}: {err}", node.service_name);
                failed_services.push((node.service_name.clone(), err.to_string()))
            }
        }
    }

    summarise_any_failed_ops(failed_services, "stop", verbosity)
}

pub async fn upgrade(
    connection_timeout_s: u64,
    do_not_start: bool,
    custom_bin_path: Option<PathBuf>,
    force: bool,
    fixed_interval: Option<u64>,
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

    let mut node_registry = NodeRegistry::load(&config::get_node_registry_path()?)?;
    refresh_node_registry(
        &mut node_registry,
        &ServiceController {},
        verbosity != VerbosityLevel::Minimal,
        false,
        false,
    )
    .await?;

    if let Some(node) = node_registry.nodes.first() {
        debug!("listen addresses for nodes[0]: {:?}", node.listen_addr);
    } else {
        debug!("There are no nodes currently added or active");
    }

    if !use_force {
        let node_versions = node_registry
            .nodes
            .iter()
            .map(|n| Version::parse(&n.version).map_err(|_| eyre!("Failed to parse Version")))
            .collect::<Result<Vec<Version>>>()?;
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

    let service_indices = get_services_for_ops(&node_registry, peer_ids, service_names)?;
    trace!("service_indices len: {}", service_indices.len());
    let mut upgrade_summary = Vec::new();

    for &index in &service_indices {
        let node = &mut node_registry.nodes[index];
        let env_variables = if provided_env_variables.is_some() {
            &provided_env_variables
        } else {
            &node_registry.environment_variables
        };
        let options = UpgradeOptions {
            auto_restart: false,
            env_variables: env_variables.clone(),
            force: use_force,
            start_service: !do_not_start,
            target_bin_path: upgrade_bin_path.clone(),
            target_version: target_version.clone(),
        };
        let service_name = node.service_name.clone();

        let rpc_client = RpcClient::from_socket_addr(node.rpc_socket_addr);
        let service = NodeService::new(node, Box::new(rpc_client));
        // set dynamic startup delay if fixed_interval is not set
        let service = if fixed_interval.is_none() {
            service.with_connection_timeout(Duration::from_secs(connection_timeout_s))
        } else {
            service
        };

        let mut service_manager =
            ServiceManager::new(service, Box::new(ServiceController {}), verbosity);

        match service_manager.upgrade(options).await {
            Ok(upgrade_result) => {
                info!("Service: {service_name} has been upgraded, result: {upgrade_result:?}",);
                if upgrade_result != UpgradeResult::NotRequired {
                    // It doesn't seem useful to apply the interval if there was no upgrade
                    // required for the previous service.
                    if let Some(interval) = fixed_interval {
                        debug!("Sleeping for {interval} milliseconds",);
                        std::thread::sleep(std::time::Duration::from_millis(interval));
                    }
                }
                upgrade_summary.push((
                    service_manager.service.service_data.service_name.clone(),
                    upgrade_result,
                ));
                node_registry.save()?;
            }
            Err(err) => {
                error!("Error upgrading service {service_name}: {err}");
                upgrade_summary.push((
                    node.service_name.clone(),
                    UpgradeResult::Error(format!("Error: {err}")),
                ));
                node_registry.save()?;
            }
        }
    }

    if verbosity != VerbosityLevel::Minimal {
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

/// Ensure n nodes are running by stopping nodes or by adding and starting nodes if required.
///
/// The arguments here are mostly mirror those used in `add`.
pub async fn maintain_n_running_nodes(
    auto_restart: bool,
    auto_set_nat_flags: bool,
    connection_timeout_s: u64,
    max_nodes_to_run: u16,
    data_dir_path: Option<PathBuf>,
    enable_metrics_server: bool,
    env_variables: Option<Vec<(String, String)>>,
    evm_network: Option<EvmNetwork>,
    home_network: bool,
    log_dir_path: Option<PathBuf>,
    log_format: Option<LogFormat>,
    max_archived_log_files: Option<usize>,
    max_log_files: Option<usize>,
    metrics_port: Option<PortRange>,
    network_id: Option<u8>,
    node_ip: Option<Ipv4Addr>,
    node_port: Option<PortRange>,
    peers_args: InitialPeersConfig,
    rewards_address: RewardsAddress,
    rpc_address: Option<Ipv4Addr>,
    rpc_port: Option<PortRange>,
    src_path: Option<PathBuf>,
    url: Option<String>,
    upnp: bool,
    user: Option<String>,
    version: Option<String>,
    verbosity: VerbosityLevel,
    start_node_interval: Option<u64>,
) -> Result<()> {
    let node_registry = NodeRegistry::load(&config::get_node_registry_path()?)?;
    let running_nodes = node_registry
        .nodes
        .iter()
        .filter(|node| node.status == ServiceStatus::Running)
        .map(|node| node.service_name.clone())
        .collect::<Vec<_>>();

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
            stop(None, vec![], services_to_stop, verbosity).await?;
        }
        Ordering::Less => {
            let to_start_count = target_count - running_count;
            let inactive_nodes = node_registry
                .nodes
                .iter()
                .filter(|node| {
                    node.status == ServiceStatus::Stopped || node.status == ServiceStatus::Added
                })
                .map(|node| node.service_name.clone())
                .collect::<Vec<_>>();

            info!("Inactive nodes available: {}", inactive_nodes.len());

            if to_start_count <= inactive_nodes.len() {
                let nodes_to_start = inactive_nodes.into_iter().take(to_start_count).collect();
                info!(
                    "Starting {} existing inactive nodes: {:?}",
                    to_start_count, nodes_to_start
                );
                start(
                    connection_timeout_s,
                    start_node_interval,
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
                        auto_restart,
                        auto_set_nat_flags,
                        Some(1),
                        data_dir_path.clone(),
                        enable_metrics_server,
                        env_variables.clone(),
                        evm_network.clone(),
                        home_network,
                        log_dir_path.clone(),
                        log_format,
                        max_archived_log_files,
                        max_log_files,
                        metrics_port.clone(),
                        network_id,
                        node_ip,
                        Some(PortRange::Single(port)),
                        peers_args.clone(),
                        rewards_address,
                        rpc_address,
                        rpc_port.clone(),
                        src_path.clone(),
                        upnp,
                        url.clone(),
                        user.clone(),
                        version.clone(),
                        verbosity,
                    )
                    .await?;

                    if i == 0 {
                        start(
                            connection_timeout_s,
                            start_node_interval,
                            vec![],
                            added_service,
                            verbosity,
                        )
                        .await?;
                    }
                }

                if !inactive_nodes.is_empty() {
                    start(
                        connection_timeout_s,
                        start_node_interval,
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
    let final_node_registry = NodeRegistry::load(&config::get_node_registry_path()?)?;
    let final_running_count = final_node_registry
        .nodes
        .iter()
        .filter(|node| node.status == ServiceStatus::Running)
        .count();

    info!("Final running node count: {}", final_running_count);
    if final_running_count != target_count {
        warn!(
            "Failed to reach target node count. Expected {}, but got {}",
            target_count, final_running_count
        );
    }

    Ok(())
}

fn get_services_for_ops(
    node_registry: &NodeRegistry,
    peer_ids: Vec<String>,
    service_names: Vec<String>,
) -> Result<Vec<usize>> {
    let mut service_indices = Vec::new();

    if service_names.is_empty() && peer_ids.is_empty() {
        for node in node_registry.nodes.iter() {
            if let Some(index) = node_registry.nodes.iter().position(|x| {
                x.service_name == node.service_name && x.status != ServiceStatus::Removed
            }) {
                service_indices.push(index);
            }
        }
    } else {
        for name in &service_names {
            if let Some(index) = node_registry
                .nodes
                .iter()
                .position(|x| x.service_name == *name && x.status != ServiceStatus::Removed)
            {
                service_indices.push(index);
            } else {
                error!("No service named '{name}'");
                return Err(eyre!(format!("No service named '{name}'")));
            }
        }

        for peer_id_str in &peer_ids {
            let peer_id = PeerId::from_str(peer_id_str)
                .inspect_err(|err| error!("Error parsing PeerId: {err:?}"))?;
            if let Some(index) = node_registry
                .nodes
                .iter()
                .position(|x| x.peer_id == Some(peer_id) && x.status != ServiceStatus::Removed)
            {
                service_indices.push(index);
            } else {
                error!("Could not find node with peer id: '{peer_id:?}'");
                return Err(eyre!(format!(
                    "Could not find node with peer ID '{peer_id}'",
                )));
            }
        }
    }

    Ok(service_indices)
}

fn summarise_any_failed_ops(
    failed_services: Vec<(String, String)>,
    verb: &str,
    verbosity: VerbosityLevel,
) -> Result<()> {
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
