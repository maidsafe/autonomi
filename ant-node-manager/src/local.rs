// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::add_services::config::PortRange;
use crate::helpers::{
    check_port_availability, get_bin_version, get_start_port_if_applicable, increment_port_option,
};

use ant_bootstrap::InitialPeersConfig;
use ant_evm::{EvmNetwork, RewardsAddress};
use ant_logging::LogFormat;
use ant_service_management::metric::{MetricActions, MetricClient};
use ant_service_management::{
    control::ServiceControl, NodeRegistry, NodeServiceData, ServiceStatus,
};
use color_eyre::eyre::OptionExt;
use color_eyre::{eyre::eyre, Result};
use colored::Colorize;
use libp2p::{Multiaddr, PeerId};
#[cfg(test)]
use mockall::automock;
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    str::FromStr,
};
use sysinfo::{Pid, System};

#[cfg_attr(test, automock)]
pub trait Launcher {
    fn get_antnode_path(&self) -> PathBuf;
    #[allow(clippy::too_many_arguments)]
    fn launch_node(
        &self,
        first: bool,
        log_format: Option<LogFormat>,
        metrics_port: Option<u16>,
        node_port: Option<u16>,
        rewards_address: RewardsAddress,
        evm_network: Option<EvmNetwork>,
    ) -> Result<()>;
    fn wait(&self, delay: u64);
}

#[derive(Default)]
pub struct LocalSafeLauncher {
    pub antnode_bin_path: PathBuf,
}

impl Launcher for LocalSafeLauncher {
    fn get_antnode_path(&self) -> PathBuf {
        self.antnode_bin_path.clone()
    }

    fn launch_node(
        &self,
        first: bool,
        log_format: Option<LogFormat>,
        metrics_port: Option<u16>,
        node_port: Option<u16>,
        rewards_address: RewardsAddress,
        evm_network: Option<EvmNetwork>,
    ) -> Result<()> {
        let mut args = Vec::new();

        if first {
            args.push("--first".to_string())
        }

        if let Some(log_format) = log_format {
            args.push("--log-format".to_string());
            args.push(log_format.as_str().to_string());
        }

        if let Some(metrics_port) = metrics_port {
            args.push("--metrics-server-port".to_string());
            args.push(metrics_port.to_string());
        }

        if let Some(node_port) = node_port {
            args.push("--port".to_string());
            args.push(node_port.to_string());
        }

        args.push("--local".to_string());

        args.push("--rewards-address".to_string());
        args.push(rewards_address.to_string());

        if let Some(network) = evm_network {
            args.push(format!("evm-{}", network.identifier()));

            if let EvmNetwork::Custom(custom) = network {
                args.push("--rpc-url".to_string());
                args.push(custom.rpc_url_http.to_string());
                args.push("--payment-token-address".to_string());
                args.push(custom.payment_token_address.to_string());
                args.push("--data-payments-address".to_string());
                args.push(custom.data_payments_address.to_string());
            }
        }

        Command::new(self.antnode_bin_path.clone())
            .args(args)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .inspect_err(|err| error!("Error while spawning node process: {err:?}"))?;

        Ok(())
    }

    /// Provide a delay for the service to start or stop.
    ///
    /// This is wrapped mainly just for unit testing.
    fn wait(&self, delay: u64) {
        std::thread::sleep(std::time::Duration::from_millis(delay));
    }
}

pub fn kill_network(node_registry: &NodeRegistry, keep_directories: bool) -> Result<()> {
    let mut system = System::new_all();
    system.refresh_all();

    // It's possible that the faucet was not spun up because the network failed the validation
    // process. If it wasn't running, we obviously don't need to do anything.
    if let Some(faucet) = &node_registry.faucet {
        // If we're here, the faucet was spun up. However, it's possible for the process to have
        // died since then. In that case, we don't need to do anything.
        // I think the use of `unwrap` is justified here, because for a local network, if the
        // faucet is not `None`, the pid also must have a value.
        if let Some(process) = system.process(Pid::from(faucet.pid.unwrap() as usize)) {
            process.kill();
            debug!("Faucet has been killed");
            println!("{} Killed faucet", "✓".green());
        }
    }

    let faucet_data_path = dirs_next::data_dir()
        .ok_or_else(|| eyre!("Could not obtain user's data directory"))?
        .join("autonomi")
        .join("test_faucet");
    if faucet_data_path.is_dir() {
        std::fs::remove_dir_all(faucet_data_path)?;
        debug!("Removed faucet data directory");
    }
    let genesis_data_path = dirs_next::data_dir()
        .ok_or_else(|| eyre!("Could not obtain user's data directory"))?
        .join("autonomi")
        .join("test_genesis");
    if genesis_data_path.is_dir() {
        debug!("Removed genesis data directory");
        std::fs::remove_dir_all(genesis_data_path)?;
    }

    for node in node_registry.nodes.iter() {
        println!("{}:", node.service_name);
        // If the PID is not set it means the `status` command ran and determined the node was
        // already dead anyway, so we don't need to do anything.
        if let Some(pid) = node.pid {
            // It could be possible that None would be returned here, if the process had already
            // died, but the `status` command had not ran. In that case, we don't need to do
            // anything anyway.
            if let Some(process) = system.process(Pid::from(pid as usize)) {
                process.kill();
                debug!("Killed node: {} ({})", node.service_name, pid);
                println!("  {} Killed process", "✓".green());
            }
        }

        if !keep_directories {
            // At this point we don't allow path overrides, so deleting the data directory will clear
            // the log directory also.
            if let Err(e) = std::fs::remove_dir_all(&node.data_dir_path) {
                error!("Failed to remove node data directory: {:?}", e);
                println!(
                    "  {} Failed to remove {}: {e}",
                    "✗".red(),
                    node.data_dir_path.to_string_lossy()
                );
            } else {
                debug!("Removed node data directory: {:?}", node.data_dir_path);
                println!(
                    "  {} Removed {}",
                    "✓".green(),
                    node.data_dir_path.to_string_lossy()
                );
            }
        }
    }

    Ok(())
}

pub struct LocalNetworkOptions {
    pub antnode_bin_path: PathBuf,
    pub enable_metrics_server: bool,
    pub join: bool,
    pub interval: u64,
    pub metrics_port: Option<PortRange>,
    pub node_port: Option<PortRange>,
    pub node_count: u16,
    pub peers: Option<Vec<Multiaddr>>,
    pub skip_validation: bool,
    pub log_format: Option<LogFormat>,
    pub rewards_address: RewardsAddress,
    pub evm_network: Option<EvmNetwork>,
}

pub async fn run_network(
    options: LocalNetworkOptions,
    node_registry: &mut NodeRegistry,
    service_control: &dyn ServiceControl,
) -> Result<()> {
    info!("Running local network");

    // Check port availability when joining a local network.
    if let Some(port_range) = &options.node_port {
        port_range.validate(options.node_count)?;
        check_port_availability(port_range, &node_registry.nodes)?;
    }

    if let Some(port_range) = &options.metrics_port {
        port_range.validate(options.node_count)?;
        check_port_availability(port_range, &node_registry.nodes)?;
    }

    let launcher = LocalSafeLauncher {
        antnode_bin_path: options.antnode_bin_path.to_path_buf(),
    };

    let mut node_port = get_start_port_if_applicable(options.node_port);
    let mut metrics_port = get_start_port_if_applicable(options.metrics_port);

    // Start the bootstrap node if it doesnt exist.
    let (bootstrap_peers, start) = if options.join {
        if let Some(peers) = options.peers {
            (peers, 1)
        } else {
            let peer = node_registry
                .nodes
                .iter()
                .find_map(|n| n.listen_addr.clone())
                .ok_or_eyre("Unable to obtain a peer to connect to")?;
            (peer, 1)
        }
    } else {
        let metrics_free_port = if let Some(port) = metrics_port {
            port
        } else {
            service_control.get_available_port()?
        };

        let metric_client = MetricClient::new(metrics_free_port);

        let number = (node_registry.nodes.len() as u16) + 1;
        let node = run_node(
            RunNodeOptions {
                first: true,
                metrics_port: Some(metrics_free_port),
                node_port,
                interval: options.interval,
                log_format: options.log_format,
                number,
                rewards_address: options.rewards_address,
                evm_network: options.evm_network.clone(),
                version: get_bin_version(&launcher.get_antnode_path())?,
            },
            &launcher,
            &metric_client,
        )
        .await?;
        node_registry.nodes.push(node.clone());
        let bootstrap_peers = node
            .listen_addr
            .ok_or_eyre("The listen address was not set")?;
        node_port = increment_port_option(node_port);
        metrics_port = increment_port_option(metrics_port);
        (bootstrap_peers, 2)
    };
    node_registry.save()?;

    for _ in start..=options.node_count {
        let metrics_free_port = if let Some(port) = metrics_port {
            port
        } else {
            service_control.get_available_port()?
        };

        let metric_client = MetricClient::new(metrics_free_port);

        let number = (node_registry.nodes.len() as u16) + 1;
        let node = run_node(
            RunNodeOptions {
                first: false,
                metrics_port: Some(metrics_free_port),
                node_port,
                interval: options.interval,
                log_format: options.log_format,
                number,
                rewards_address: options.rewards_address,
                evm_network: options.evm_network.clone(),
                version: get_bin_version(&launcher.get_antnode_path())?,
            },
            &launcher,
            &metric_client,
        )
        .await?;
        node_registry.nodes.push(node);

        // We save the node registry for each launch because it's possible any node can fail to
        // launch, or maybe the validation will fail. In the error case, we will want to use the
        // `kill` command for the nodes that we did spin up. The `kill` command works on the basis
        // of what's in the node registry.
        node_registry.save()?;

        node_port = increment_port_option(node_port);
        metrics_port = increment_port_option(metrics_port);
    }

    if !options.skip_validation {
        debug!("Waiting for 10 seconds before validating the network...");
        println!("Waiting for 10 seconds before validating the network...");
        std::thread::sleep(std::time::Duration::from_secs(10));
        validate_network(node_registry, bootstrap_peers.clone()).await?;
    }

    Ok(())
}

pub struct RunNodeOptions {
    pub first: bool,
    pub interval: u64,
    pub log_format: Option<LogFormat>,
    pub metrics_port: Option<u16>,
    pub node_port: Option<u16>,
    pub number: u16,
    pub rewards_address: RewardsAddress,
    pub evm_network: Option<EvmNetwork>,
    pub version: String,
}

pub async fn run_node(
    run_options: RunNodeOptions,
    launcher: &dyn Launcher,
    metric_client: &dyn MetricActions,
) -> Result<NodeServiceData> {
    info!("Launching node {}...", run_options.number);
    println!("Launching node {}...", run_options.number);
    launcher.launch_node(
        run_options.first,
        run_options.log_format,
        run_options.metrics_port,
        run_options.node_port,
        run_options.rewards_address,
        run_options.evm_network.clone(),
    )?;
    launcher.wait(run_options.interval);

    let node_info = metric_client.node_info().await?;
    let peer_id = node_info.peer_id;
    let network_info = metric_client.network_info().await?;
    let connected_peers = Some(network_info.connected_peers);
    let listen_addrs = network_info.listeners.into_iter().collect();

    Ok(NodeServiceData {
        antnode_path: launcher.get_antnode_path(),
        auto_restart: false,
        connected_peers,
        data_dir_path: node_info.data_path,
        evm_network: run_options.evm_network.unwrap_or(EvmNetwork::ArbitrumOne),
        home_network: false,
        listen_addr: Some(listen_addrs),
        log_dir_path: node_info.log_path,
        log_format: run_options.log_format,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: run_options.metrics_port,
        network_id: None,
        node_ip: None,
        node_port: run_options.node_port,
        number: run_options.number,
        peer_id: Some(peer_id),
        peers_args: InitialPeersConfig {
            first: run_options.first,
            addrs: vec![],
            network_contacts_url: vec![],
            local: true,
            disable_mainnet_contacts: true,
            ignore_cache: true,
            bootstrap_cache_dir: None,
        },
        pid: Some(node_info.pid),
        rewards_address: run_options.rewards_address,
        reward_balance: None,
        rpc_socket_addr: None,
        status: ServiceStatus::Running,
        service_name: format!("antnode-local{}", run_options.number),
        upnp: false,
        user: None,
        user_mode: false,
        version: run_options.version.to_string(),
    })
}

//
// Private Helpers
//

async fn validate_network(node_registry: &mut NodeRegistry, peers: Vec<Multiaddr>) -> Result<()> {
    let mut all_peers = node_registry
        .nodes
        .iter()
        .map(|n| n.peer_id.ok_or_eyre("The PeerId was not set"))
        .collect::<Result<Vec<PeerId>>>()?;
    // The additional peers are peers being managed outwith the node manager. This only applies
    // when we've joined a network not being managed by the node manager. Otherwise, this list will
    // be empty.
    let additional_peers = peers
        .into_iter()
        .filter_map(|addr| {
            addr.to_string()
                .rsplit('/')
                .next()
                .and_then(|id_str| PeerId::from_str(id_str).ok())
        })
        .collect::<Vec<PeerId>>();
    all_peers.extend(additional_peers);

    for node in node_registry.nodes.iter() {
        let metric_client =
            MetricClient::new(node.metrics_port.ok_or_eyre("Metrics port not set")?);
        let net_info = metric_client.network_info().await?;
        let peers = net_info.connected_peers;
        let peer_id = node.peer_id.ok_or_eyre("The PeerId was not set")?;
        debug!("Node {peer_id} has {} peers", peers.len());
        println!("Node {peer_id} has {} peers", peers.len());

        // Look for peers that are not supposed to be present in the network. This can happen if
        // the node has connected to peers on other networks.
        let invalid_peers: Vec<PeerId> = peers
            .iter()
            .filter(|peer| !all_peers.contains(peer))
            .cloned()
            .collect();
        if !invalid_peers.is_empty() {
            for invalid_peer in invalid_peers.iter() {
                println!("Invalid peer found: {}", invalid_peer);
            }
            error!("Network validation failed: {invalid_peers:?}");
            return Err(eyre!("Network validation failed",));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ant_evm::utils::dummy_address;
    use ant_service_management::{
        error::Result as MetricResult,
        metric::{NetworkInfo, NodeInfo},
    };
    use async_trait::async_trait;
    use libp2p_identity::PeerId;
    use mockall::mock;
    use mockall::predicate::*;
    use std::str::FromStr;

    mock! {
        pub MetricClient {}
        #[async_trait]
        impl MetricActions for MetricClient {
            async fn node_info(&self) -> MetricResult<NodeInfo>;
            async fn network_info(&self) -> MetricResult<NetworkInfo>;
            async fn is_node_connected_to_network(&self, timeout: std::time::Duration) -> MetricResult<()>;
        }
    }

    #[tokio::test]
    async fn run_node_should_launch_the_genesis_node() -> Result<()> {
        let mut mock_launcher = MockLauncher::new();
        let mut mock_metric_client = MockMetricClient::new();
        let rewards_address = dummy_address();

        let peer_id = PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?;
        mock_launcher
            .expect_launch_node()
            .with(
                eq(true),
                eq(None),
                eq(None),
                eq(None),
                eq(rewards_address),
                eq(None),
            )
            .times(1)
            .returning(|_, _, _, _, _, _| Ok(()));
        mock_launcher
            .expect_wait()
            .with(eq(100))
            .times(1)
            .returning(|_| ());
        mock_launcher
            .expect_get_antnode_path()
            .times(1)
            .returning(|| PathBuf::from("/usr/local/bin/antnode"));

        mock_metric_client
            .expect_node_info()
            .times(1)
            .returning(move || {
                Ok(NodeInfo {
                    pid: 1000,
                    peer_id,
                    data_path: PathBuf::from(format!("~/.local/share/autonomi/{peer_id}")),
                    log_path: PathBuf::from(format!("~/.local/share/autonomi/{peer_id}/logs")),
                    version: "0.100.12".to_string(),
                    uptime: std::time::Duration::from_secs(1), // the service was just started
                    wallet_balance: 0,
                })
            });
        mock_metric_client
            .expect_network_info()
            .times(1)
            .returning(move || {
                Ok(NetworkInfo {
                    connected_peers: Vec::new(),
                    listeners: Vec::new(),
                })
            });

        let node = run_node(
            RunNodeOptions {
                first: true,
                interval: 100,
                log_format: None,
                metrics_port: None,
                node_port: None,
                number: 1,
                rewards_address,
                evm_network: None,
                version: "0.100.12".to_string(),
            },
            &mock_launcher,
            &mock_metric_client,
        )
        .await?;

        assert!(node.peers_args.first);
        assert_eq!(node.version, "0.100.12");
        assert_eq!(node.service_name, "antnode-local1");
        assert_eq!(
            node.data_dir_path,
            PathBuf::from(format!("~/.local/share/autonomi/{peer_id}"))
        );
        assert_eq!(
            node.log_dir_path,
            PathBuf::from(format!("~/.local/share/autonomi/{peer_id}/logs"))
        );
        assert_eq!(node.number, 1);
        assert_eq!(node.pid, Some(1000));
        assert_eq!(node.status, ServiceStatus::Running);
        assert_eq!(node.antnode_path, PathBuf::from("/usr/local/bin/antnode"));

        Ok(())
    }
}
