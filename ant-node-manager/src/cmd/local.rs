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
    VerbosityLevel,
    add_services::config::PortRange,
    local::{LocalNetworkOptions, kill_network, run_network},
    print_banner, status_report,
};
use ant_bootstrap::InitialPeersConfig;
use ant_evm::{EvmNetwork, RewardsAddress};
use ant_logging::LogFormat;
use ant_releases::{AntReleaseRepoActions, ReleaseType};
use ant_service_management::{
    NodeRegistryManager, control::ServiceController, get_local_node_registry_path,
};
use color_eyre::{Help, Report, Result, eyre::eyre};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    sync::Arc,
};
use sysinfo::System;
use tokio::time::{Duration, sleep};

#[tracing::instrument(
    skip(
        metrics_port,
        node_path,
        node_port,
        _peers_args,
        log_format,
        rpc_port,
        rewards_address,
        evm_network,
        node_version
    ),
    err
)]
pub async fn join(
    build: bool,
    count: u16,
    interval: u64,
    metrics_port: Option<PortRange>,
    node_path: Option<PathBuf>,
    node_port: Option<PortRange>,
    node_version: Option<String>,
    log_format: Option<LogFormat>,
    _peers_args: InitialPeersConfig,
    rpc_port: Option<PortRange>,
    rewards_address: RewardsAddress,
    evm_network: EvmNetwork,
    skip_validation: bool,
    verbosity: VerbosityLevel,
) -> Result<(), Report> {
    if verbosity != VerbosityLevel::Minimal {
        print_banner("Joining Local Network");
    }
    info!("Joining local network");

    if metrics_port.is_some() && !cfg!(feature = "open-metrics") && build {
        return Err(eyre!(
            "Metrics server is not available. Please enable the open-metrics feature flag. Run the command with the --features open-metrics"
        ));
    }

    let local_node_reg_path = &get_local_node_registry_path()?;
    let local_node_registry = NodeRegistryManager::load(local_node_reg_path).await?;

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
        interval,
        join: true,
        metrics_port,
        node_count: count,
        node_port,
        peers: None,
        rpc_port,
        skip_validation,
        log_format,
        rewards_address,
        evm_network,
    };

    // Ensure EVM testnet is running before starting the local network
    ensure_evm_testnet_running(build, verbosity).await?;

    run_network(options, local_node_registry, &ServiceController {}).await?;
    Ok(())
}

#[tracing::instrument(err)]
pub async fn kill(keep_directories: bool, verbosity: VerbosityLevel) -> Result<()> {
    let local_reg_path = &get_local_node_registry_path()?;
    let local_node_registry = NodeRegistryManager::load(local_reg_path).await?;
    if local_node_registry.nodes.read().await.is_empty() {
        info!("No local network is currently running, cannot kill it");
        println!("No local network is currently running");
    } else {
        if verbosity != VerbosityLevel::Minimal {
            print_banner("Killing Local Network");
        }
        info!("Kill local network");
        kill_network(local_node_registry, keep_directories).await?;
        std::fs::remove_file(local_reg_path)?;
    }

    Ok(())
}

#[tracing::instrument(
    skip(
        metrics_port,
        node_path,
        node_port,
        node_version,
        log_format,
        rpc_port,
        rewards_address,
        evm_network
    ),
    err
)]
pub async fn run(
    build: bool,
    clean: bool,
    count: u16,
    interval: u64,
    metrics_port: Option<PortRange>,
    node_path: Option<PathBuf>,
    node_port: Option<PortRange>,
    node_version: Option<String>,
    log_format: Option<LogFormat>,
    rpc_port: Option<PortRange>,
    rewards_address: RewardsAddress,
    evm_network: EvmNetwork,
    skip_validation: bool,
    verbosity: VerbosityLevel,
) -> Result<(), Report> {
    if metrics_port.is_some() && !cfg!(feature = "open-metrics") && build {
        return Err(eyre!(
            "Metrics server is not available. Please enable the open-metrics feature flag. Run the command with the --features open-metrics"
        ));
    }

    // In the clean case, the node registry must be loaded *after* the existing network has
    // been killed, which clears it out.
    let local_node_reg_path = &get_local_node_registry_path()?;
    let local_node_registry: NodeRegistryManager = if clean {
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
        kill(false, verbosity).await?;
        NodeRegistryManager::load(local_node_reg_path).await?
    } else {
        let local_node_registry = NodeRegistryManager::load(local_node_reg_path).await?;
        if !local_node_registry.nodes.read().await.is_empty() {
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

    // Ensure EVM testnet is running before starting the local network
    ensure_evm_testnet_running(build, verbosity).await?;

    let options = LocalNetworkOptions {
        antnode_bin_path,
        join: false,
        interval,
        metrics_port,
        node_port,
        node_count: count,
        peers: None,
        rpc_port,
        skip_validation,
        log_format,
        rewards_address,
        evm_network,
    };
    run_network(options, local_node_registry.clone(), &ServiceController {}).await?;

    local_node_registry.save().await?;
    Ok(())
}

/// Get the path to the evm-testnet binary, building it if necessary
fn get_evm_testnet_bin_path(build: bool, verbosity: VerbosityLevel) -> Result<PathBuf> {
    if build {
        // Build the evm-testnet binary from source
        if verbosity != VerbosityLevel::Minimal {
            print_banner("Building evm-testnet binary");
        }

        let mut cmd = Command::new("cargo");
        cmd.args(["build", "--release", "--bin", "evm-testnet"])
            .stdout(if verbosity == VerbosityLevel::Minimal {
                Stdio::null()
            } else {
                Stdio::inherit()
            })
            .stderr(if verbosity == VerbosityLevel::Minimal {
                Stdio::null()
            } else {
                Stdio::inherit()
            });

        let output = cmd.output()?;
        if !output.status.success() {
            return Err(eyre!("Failed to build evm-testnet binary"));
        }

        let target_dir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".to_string());
        Ok(PathBuf::from(target_dir)
            .join("release")
            .join("evm-testnet"))
    } else {
        // Try to find evm-testnet in PATH
        match which::which("evm-testnet") {
            Ok(path) => Ok(path),
            Err(_) => {
                // Fallback to building from source
                if verbosity != VerbosityLevel::Minimal {
                    println!("evm-testnet not found in PATH, building from source...");
                }

                let mut cmd = Command::new("cargo");
                cmd.args(["build", "--release", "--bin", "evm-testnet"])
                    .stdout(if verbosity == VerbosityLevel::Minimal {
                        Stdio::null()
                    } else {
                        Stdio::inherit()
                    })
                    .stderr(if verbosity == VerbosityLevel::Minimal {
                        Stdio::null()
                    } else {
                        Stdio::inherit()
                    });

                let output = cmd.output()?;
                if !output.status.success() {
                    return Err(eyre!("Failed to build evm-testnet binary"));
                }

                let target_dir =
                    std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".to_string());
                Ok(PathBuf::from(target_dir)
                    .join("release")
                    .join("evm-testnet"))
            }
        }
    }
}

/// Spawn the evm-testnet binary as a child process
#[tracing::instrument(err)]
async fn spawn_evm_testnet(build: bool, verbosity: VerbosityLevel) -> Result<()> {
    let evm_testnet_path = get_evm_testnet_bin_path(build, verbosity)?;

    if verbosity != VerbosityLevel::Minimal {
        print_banner("Starting EVM testnet");
    }

    let mut cmd = Command::new(&evm_testnet_path);
    cmd.stdout(if verbosity == VerbosityLevel::Minimal {
        Stdio::null()
    } else {
        Stdio::inherit()
    })
    .stderr(if verbosity == VerbosityLevel::Minimal {
        Stdio::null()
    } else {
        Stdio::inherit()
    });

    let _ = cmd.spawn()?;

    // Wait a moment for the testnet to start up
    sleep(Duration::from_millis(2000)).await;

    Ok(())
}

/// Check if EVM testnet is already running by checking the process list
fn check_evm_testnet_running() -> bool {
    let mut system = System::new_all();
    system.refresh_all();

    // Look for evm-testnet or anvil processes
    for process in system.processes().values() {
        let process_name = process.name().to_lowercase();
        if process_name.contains("evm-testnet") || process_name.contains("anvil") {
            return true;
        }
    }
    false
}

/// Ensure an EVM testnet is running, starting one if necessary
#[tracing::instrument(err)]
async fn ensure_evm_testnet_running(build: bool, verbosity: VerbosityLevel) -> Result<()> {
    if check_evm_testnet_running() {
        if verbosity != VerbosityLevel::Minimal {
            println!("EVM testnet is already running");
        }
        return Ok(());
    }

    spawn_evm_testnet(build, verbosity).await?;

    // Wait for the testnet to be fully ready
    let mut attempts = 0;
    while !check_evm_testnet_running() && attempts < 30 {
        sleep(Duration::from_millis(1000)).await;
        attempts += 1;
    }

    if !check_evm_testnet_running() {
        return Err(eyre!(
            "Failed to start EVM testnet - not responding after 30 seconds"
        ));
    }

    if verbosity != VerbosityLevel::Minimal {
        println!("EVM testnet started successfully");
    }

    Ok(())
}

#[tracing::instrument(err)]
pub async fn status(details: bool, fail: bool, json: bool) -> Result<()> {
    let local_node_registry = NodeRegistryManager::load(&get_local_node_registry_path()?).await?;
    if !json {
        print_banner("Local Network");
    }
    status_report(
        &local_node_registry,
        Arc::new(ServiceController {}),
        details,
        json,
        fail,
        true,
        true,
    )
    .await?;
    Ok(())
}
