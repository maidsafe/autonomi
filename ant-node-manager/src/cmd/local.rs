// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#![allow(clippy::too_many_arguments)]

use super::get_bin_path;
use crate::{
    add_services::config::PortRange,
    local::{kill_network, run_network, LocalNetworkOptions},
    print_banner, status_report, VerbosityLevel,
};
use ant_bootstrap::InitialPeersConfig;
use ant_evm::{EvmNetwork, RewardsAddress};
use ant_logging::LogFormat;
use ant_releases::{AntReleaseRepoActions, ReleaseType};
use ant_service_management::{
    control::ServiceController, get_local_node_registry_path, NodeRegistry,
};
use color_eyre::{eyre::eyre, Help, Report, Result};
use std::{path::PathBuf, sync::Arc};

pub async fn join(
    build: bool,
    count: u16,
    enable_metrics_server: bool,
    interval: u64,
    metrics_port: Option<PortRange>,
    node_path: Option<PathBuf>,
    node_port: Option<PortRange>,
    node_version: Option<String>,
    log_format: Option<LogFormat>,
    _peers_args: InitialPeersConfig,
    rewards_address: RewardsAddress,
    evm_network: Option<EvmNetwork>,
    skip_validation: bool,
    verbosity: VerbosityLevel,
) -> Result<(), Report> {
    if verbosity != VerbosityLevel::Minimal {
        print_banner("Joining Local Network");
    }
    info!("Joining local network");

    if (enable_metrics_server || metrics_port.is_some()) && !cfg!(feature = "open-metrics") && build
    {
        return Err(eyre!(
        "Metrics server is not available. Please enable the open-metrics feature flag. Run the command with the --features open-metrics"
    ));
    }

    let local_node_reg_path = &get_local_node_registry_path()?;
    let mut local_node_registry = NodeRegistry::load(local_node_reg_path)?;

    let release_repo = <dyn AntReleaseRepoActions>::default_config();

    let antnode_bin_path = get_bin_path(
        build,
        node_path,
        ReleaseType::AntNode,
        node_version,
        &*release_repo,
        verbosity,
    )
    .await?;

    let options = LocalNetworkOptions {
        antnode_bin_path,
        enable_metrics_server,
        interval,
        join: true,
        metrics_port,
        node_count: count,
        node_port,
        peers: None,
        skip_validation,
        log_format,
        rewards_address,
        evm_network,
    };
    run_network(options, &mut local_node_registry, &ServiceController {}).await?;
    Ok(())
}

pub fn kill(keep_directories: bool, verbosity: VerbosityLevel) -> Result<()> {
    let local_reg_path = &get_local_node_registry_path()?;
    let local_node_registry = NodeRegistry::load(local_reg_path)?;
    if local_node_registry.nodes.is_empty() {
        info!("No local network is currently running, cannot kill it");
        println!("No local network is currently running");
    } else {
        if verbosity != VerbosityLevel::Minimal {
            print_banner("Killing Local Network");
        }
        info!("Kill local network");
        kill_network(&local_node_registry, keep_directories)?;
        std::fs::remove_file(local_reg_path)?;
    }
    Ok(())
}

pub async fn run(
    build: bool,
    clean: bool,
    count: u16,
    enable_metrics_server: bool,
    interval: u64,
    metrics_port: Option<PortRange>,
    node_path: Option<PathBuf>,
    node_port: Option<PortRange>,
    node_version: Option<String>,
    log_format: Option<LogFormat>,
    rewards_address: RewardsAddress,
    evm_network: Option<EvmNetwork>,
    skip_validation: bool,
    verbosity: VerbosityLevel,
) -> Result<(), Report> {
    if (enable_metrics_server || metrics_port.is_some()) && !cfg!(feature = "open-metrics") && build
    {
        return Err(eyre!(
        "Metrics server is not available. Please enable the open-metrics feature flag. Run the command with the --features open-metrics"
    ));
    }

    // In the clean case, the node registry must be loaded *after* the existing network has
    // been killed, which clears it out.
    let local_node_reg_path = &get_local_node_registry_path()?;
    let mut local_node_registry: NodeRegistry = if clean {
        debug!(
            "Clean set to true, removing client, node dir, local registry and killing the network."
        );
        let client_data_path = dirs_next::data_dir()
            .ok_or_else(|| eyre!("Could not obtain user's data directory"))?
            .join("autonomi")
            .join("client");
        if client_data_path.is_dir() {
            std::fs::remove_dir_all(client_data_path)?;
        }
        if local_node_reg_path.exists() {
            std::fs::remove_file(local_node_reg_path)?;
        }
        kill(false, verbosity)?;
        NodeRegistry::load(local_node_reg_path)?
    } else {
        let local_node_registry = NodeRegistry::load(local_node_reg_path)?;
        if !local_node_registry.nodes.is_empty() {
            error!("A local network is already running, cannot run a new one");
            return Err(eyre!("A local network is already running")
                .suggestion("Use the kill command to destroy the network then try again"));
        }
        local_node_registry
    };

    if verbosity != VerbosityLevel::Minimal {
        print_banner("Launching Local Network");
    }
    info!("Launching local network");

    let release_repo = <dyn AntReleaseRepoActions>::default_config();

    let antnode_bin_path = get_bin_path(
        build,
        node_path,
        ReleaseType::AntNode,
        node_version,
        &*release_repo,
        verbosity,
    )
    .await?;

    let options = LocalNetworkOptions {
        antnode_bin_path,
        enable_metrics_server,
        join: false,
        interval,
        metrics_port,
        node_port,
        node_count: count,
        peers: None,
        skip_validation,
        log_format,
        rewards_address,
        evm_network,
    };
    run_network(options, &mut local_node_registry, &ServiceController {}).await?;

    local_node_registry.save()?;
    Ok(())
}

pub async fn status(details: bool, fail: bool, json: bool) -> Result<()> {
    let mut local_node_registry = NodeRegistry::load(&get_local_node_registry_path()?)?;
    if !json {
        print_banner("Local Network");
    }
    status_report(
        &mut local_node_registry,
        Arc::new(ServiceController {}),
        details,
        json,
        fail,
        true,
    )
    .await?;
    local_node_registry.save()?;
    Ok(())
}
