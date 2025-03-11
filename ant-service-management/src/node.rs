// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{error::Result, rpc::RpcActions, ServiceStateActions, ServiceStatus, UpgradeOptions};
use ant_bootstrap::InitialPeersConfig;
use ant_evm::{AttoTokens, EvmNetwork, RewardsAddress};
use ant_logging::LogFormat;
use ant_protocol::get_port_from_multiaddr;
use async_trait::async_trait;
use libp2p::{multiaddr::Protocol, Multiaddr, PeerId};
use serde::{de::Error as DeError, Deserialize, Deserializer, Serialize, Serializer};
use service_manager::{ServiceInstallCtx, ServiceLabel};
use std::{
    ffi::OsString,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    str::FromStr,
    time::Duration,
};

pub struct NodeService<'a> {
    pub service_data: &'a mut NodeServiceData,
    pub rpc_actions: Box<dyn RpcActions + Send>,
    /// Used to enable dynamic startup delay based on the time it takes for a node to connect to the network.
    pub connection_timeout: Option<Duration>,
}

impl<'a> NodeService<'a> {
    pub fn new(
        service_data: &'a mut NodeServiceData,
        rpc_actions: Box<dyn RpcActions + Send>,
    ) -> NodeService<'a> {
        NodeService {
            rpc_actions,
            service_data,
            connection_timeout: None,
        }
    }

    /// Set the max time to wait for the node to connect to the network.
    /// If not set, we do not perform a dynamic startup delay.
    pub fn with_connection_timeout(mut self, connection_timeout: Duration) -> NodeService<'a> {
        self.connection_timeout = Some(connection_timeout);
        self
    }
}

#[async_trait]
impl ServiceStateActions for NodeService<'_> {
    fn bin_path(&self) -> PathBuf {
        self.service_data.antnode_path.clone()
    }

    fn build_upgrade_install_context(&self, options: UpgradeOptions) -> Result<ServiceInstallCtx> {
        let label: ServiceLabel = self.service_data.service_name.parse()?;
        let mut args = vec![
            OsString::from("--rpc"),
            OsString::from(self.service_data.rpc_socket_addr.to_string()),
            OsString::from("--root-dir"),
            OsString::from(
                self.service_data
                    .data_dir_path
                    .to_string_lossy()
                    .to_string(),
            ),
            OsString::from("--log-output-dest"),
            OsString::from(self.service_data.log_dir_path.to_string_lossy().to_string()),
        ];

        push_arguments_from_initial_peers_config(&self.service_data.peers_args, &mut args);
        if let Some(log_fmt) = self.service_data.log_format {
            args.push(OsString::from("--log-format"));
            args.push(OsString::from(log_fmt.as_str()));
        }
        if let Some(id) = self.service_data.network_id {
            args.push(OsString::from("--network-id"));
            args.push(OsString::from(id.to_string()));
        }
        if self.service_data.upnp {
            args.push(OsString::from("--upnp"));
        }
        if self.service_data.home_network {
            args.push(OsString::from("--home-network"));
        }

        if let Some(node_ip) = self.service_data.node_ip {
            args.push(OsString::from("--ip"));
            args.push(OsString::from(node_ip.to_string()));
        }

        if let Some(node_port) = self.service_data.node_port {
            args.push(OsString::from("--port"));
            args.push(OsString::from(node_port.to_string()));
        }
        if let Some(metrics_port) = self.service_data.metrics_port {
            args.push(OsString::from("--metrics-server-port"));
            args.push(OsString::from(metrics_port.to_string()));
        }
        if let Some(max_archived_log_files) = self.service_data.max_archived_log_files {
            args.push(OsString::from("--max-archived-log-files"));
            args.push(OsString::from(max_archived_log_files.to_string()));
        }
        if let Some(max_log_files) = self.service_data.max_log_files {
            args.push(OsString::from("--max-log-files"));
            args.push(OsString::from(max_log_files.to_string()));
        }

        args.push(OsString::from("--rewards-address"));
        args.push(OsString::from(
            self.service_data.rewards_address.to_string(),
        ));

        args.push(OsString::from(self.service_data.evm_network.to_string()));
        if let EvmNetwork::Custom(custom_network) = &self.service_data.evm_network {
            args.push(OsString::from("--rpc-url"));
            args.push(OsString::from(custom_network.rpc_url_http.to_string()));
            args.push(OsString::from("--payment-token-address"));
            args.push(OsString::from(
                custom_network.payment_token_address.to_string(),
            ));
            args.push(OsString::from("--data-payments-address"));
            args.push(OsString::from(
                custom_network.data_payments_address.to_string(),
            ));
        }

        Ok(ServiceInstallCtx {
            args,
            autostart: options.auto_restart,
            contents: None,
            environment: options.env_variables,
            label: label.clone(),
            program: self.service_data.antnode_path.to_path_buf(),
            username: self.service_data.user.clone(),
            working_directory: None,
            disable_restart_on_failure: true,
        })
    }

    fn data_dir_path(&self) -> PathBuf {
        self.service_data.data_dir_path.clone()
    }

    fn is_user_mode(&self) -> bool {
        self.service_data.user_mode
    }

    fn log_dir_path(&self) -> PathBuf {
        self.service_data.log_dir_path.clone()
    }

    fn name(&self) -> String {
        self.service_data.service_name.clone()
    }

    fn pid(&self) -> Option<u32> {
        self.service_data.pid
    }

    fn on_remove(&mut self) {
        self.service_data.status = ServiceStatus::Removed;
    }

    async fn on_start(&mut self, pid: Option<u32>, full_refresh: bool) -> Result<()> {
        let (connected_peers, pid, peer_id) = if full_refresh {
            debug!(
                "Performing full refresh for {}",
                self.service_data.service_name
            );
            if let Some(connection_timeout) = self.connection_timeout {
                debug!(
                    "Performing dynamic startup delay for {}",
                    self.service_data.service_name
                );
                self.rpc_actions
                    .is_node_connected_to_network(connection_timeout)
                    .await?;
            }

            let node_info = self
                .rpc_actions
                .node_info()
                .await
                .inspect_err(|err| error!("Error obtaining node_info via RPC: {err:?}"))?;
            let network_info = self
                .rpc_actions
                .network_info()
                .await
                .inspect_err(|err| error!("Error obtaining network_info via RPC: {err:?}"))?;

            self.service_data.listen_addr = Some(
                network_info
                    .listeners
                    .iter()
                    .cloned()
                    .map(|addr| addr.with(Protocol::P2p(node_info.peer_id)))
                    .collect(),
            );
            for addr in &network_info.listeners {
                if let Some(port) = get_port_from_multiaddr(addr) {
                    debug!(
                        "Found antnode port for {}: {port}",
                        self.service_data.service_name
                    );
                    self.service_data.node_port = Some(port);
                    break;
                }
            }

            if self.service_data.node_port.is_none() {
                error!("Could not find antnode port");
                error!("This will cause the node to have a different port during upgrade");
            }

            (
                Some(network_info.connected_peers),
                pid,
                Some(node_info.peer_id),
            )
        } else {
            debug!(
                "Performing partial refresh for {}",
                self.service_data.service_name
            );
            debug!("Previously assigned data will be used");
            (
                self.service_data.connected_peers.clone(),
                pid,
                self.service_data.peer_id,
            )
        };

        self.service_data.connected_peers = connected_peers;
        self.service_data.peer_id = peer_id;
        self.service_data.pid = pid;
        self.service_data.status = ServiceStatus::Running;
        Ok(())
    }

    async fn on_stop(&mut self) -> Result<()> {
        debug!("Marking {} as stopped", self.service_data.service_name);
        self.service_data.pid = None;
        self.service_data.status = ServiceStatus::Stopped;
        self.service_data.connected_peers = None;
        Ok(())
    }

    fn set_version(&mut self, version: &str) {
        self.service_data.version = version.to_string();
    }

    fn status(&self) -> ServiceStatus {
        self.service_data.status.clone()
    }

    fn version(&self) -> String {
        self.service_data.version.clone()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeServiceData {
    pub antnode_path: PathBuf,
    #[serde(default)]
    pub auto_restart: bool,
    #[serde(
        serialize_with = "serialize_connected_peers",
        deserialize_with = "deserialize_connected_peers"
    )]
    pub connected_peers: Option<Vec<PeerId>>,
    pub data_dir_path: PathBuf,
    #[serde(default)]
    pub evm_network: EvmNetwork,
    pub home_network: bool,
    pub listen_addr: Option<Vec<Multiaddr>>,
    pub log_dir_path: PathBuf,
    pub log_format: Option<LogFormat>,
    pub max_archived_log_files: Option<usize>,
    pub max_log_files: Option<usize>,
    #[serde(default)]
    pub metrics_port: Option<u16>,
    pub network_id: Option<u8>,
    #[serde(default)]
    pub node_ip: Option<Ipv4Addr>,
    #[serde(default)]
    pub node_port: Option<u16>,
    pub number: u16,
    #[serde(
        serialize_with = "serialize_peer_id",
        deserialize_with = "deserialize_peer_id"
    )]
    pub peer_id: Option<PeerId>,
    pub peers_args: InitialPeersConfig,
    pub pid: Option<u32>,
    #[serde(default)]
    pub rewards_address: RewardsAddress,
    pub reward_balance: Option<AttoTokens>,
    pub rpc_socket_addr: SocketAddr,
    pub service_name: String,
    pub status: ServiceStatus,
    #[serde(default = "default_upnp")]
    pub upnp: bool,
    pub user: Option<String>,
    pub user_mode: bool,
    pub version: String,
}

fn default_upnp() -> bool {
    false
}

fn serialize_peer_id<S>(value: &Option<PeerId>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    if let Some(peer_id) = value {
        return serializer.serialize_str(&peer_id.to_string());
    }
    serializer.serialize_none()
}

fn deserialize_peer_id<'de, D>(deserializer: D) -> Result<Option<PeerId>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    if let Some(peer_id_str) = s {
        PeerId::from_str(&peer_id_str)
            .map(Some)
            .map_err(DeError::custom)
    } else {
        Ok(None)
    }
}

fn serialize_connected_peers<S>(
    connected_peers: &Option<Vec<PeerId>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match connected_peers {
        Some(peers) => {
            let peer_strs: Vec<String> = peers.iter().map(|p| p.to_string()).collect();
            serializer.serialize_some(&peer_strs)
        }
        None => serializer.serialize_none(),
    }
}

fn deserialize_connected_peers<'de, D>(deserializer: D) -> Result<Option<Vec<PeerId>>, D::Error>
where
    D: Deserializer<'de>,
{
    let vec: Option<Vec<String>> = Option::deserialize(deserializer)?;
    match vec {
        Some(peer_strs) => {
            let peers: Result<Vec<PeerId>, _> = peer_strs
                .into_iter()
                .map(|s| PeerId::from_str(&s).map_err(DeError::custom))
                .collect();
            peers.map(Some)
        }
        None => Ok(None),
    }
}

impl NodeServiceData {
    /// Returns the UDP port from our node's listen address.
    pub fn get_antnode_port(&self) -> Option<u16> {
        // assuming the listening addr contains /ip4/127.0.0.1/udp/56215/quic-v1/p2p/<peer_id>
        if let Some(multi_addrs) = &self.listen_addr {
            println!("Listening addresses are defined");
            for addr in multi_addrs {
                if let Some(port) = get_port_from_multiaddr(addr) {
                    println!("Found port: {}", port);
                    return Some(port);
                }
            }
        }
        None
    }

    /// Returns an optional critical failure of the node.
    pub fn get_critical_failure(&self) -> Option<(chrono::DateTime<chrono::Utc>, String)> {
        const CRITICAL_FAILURE_LOG_FILE: &str = "critical_failure.log";

        let log_path = self.log_dir_path.join(CRITICAL_FAILURE_LOG_FILE);

        if let Ok(content) = std::fs::read_to_string(log_path) {
            if let Some((timestamp, message)) = content.split_once(']') {
                let timestamp_trimmed = timestamp.trim_start_matches('[').trim();
                if let Ok(datetime) = timestamp_trimmed.parse::<chrono::DateTime<chrono::Utc>>() {
                    let message_trimmed = message
                        .trim()
                        .trim_start_matches("Node terminated due to: ");
                    return Some((datetime, message_trimmed.to_string()));
                }
            }
        }

        None
    }
}

/// Pushes arguments from the `InitialPeersConfig` struct to the provided `args` vector.
pub fn push_arguments_from_initial_peers_config(
    init_peers_config: &InitialPeersConfig,
    args: &mut Vec<OsString>,
) {
    if init_peers_config.first {
        args.push(OsString::from("--first"));
    }
    if init_peers_config.local {
        args.push(OsString::from("--local"));
    }
    if !init_peers_config.addrs.is_empty() {
        let peers_str = init_peers_config
            .addrs
            .iter()
            .map(|peer| peer.to_string())
            .collect::<Vec<_>>()
            .join(",");
        args.push(OsString::from("--peer"));
        args.push(OsString::from(peers_str));
    }
    if !init_peers_config.network_contacts_url.is_empty() {
        args.push(OsString::from("--network-contacts-url"));
        args.push(OsString::from(
            init_peers_config
                .network_contacts_url
                .iter()
                .map(|url| url.to_string())
                .collect::<Vec<_>>()
                .join(","),
        ));
    }
    if init_peers_config.disable_mainnet_contacts {
        args.push(OsString::from("--testnet"));
    }
    if init_peers_config.ignore_cache {
        args.push(OsString::from("--ignore-cache"));
    }
    if let Some(path) = &init_peers_config.bootstrap_cache_dir {
        args.push(OsString::from("--bootstrap-cache-dir"));
        args.push(OsString::from(path.to_string_lossy().to_string()));
    }
}
