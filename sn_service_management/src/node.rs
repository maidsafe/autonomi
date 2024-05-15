// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{error::Result, rpc::RpcActions, ServiceStateActions, ServiceStatus, UpgradeOptions};
use async_trait::async_trait;
use libp2p::{multiaddr::Protocol, Multiaddr, PeerId};
use serde::{de::Error as DeError, Deserialize, Deserializer, Serialize, Serializer};
use service_manager::{ServiceInstallCtx, ServiceLabel};
use sn_protocol::get_port_from_multiaddr;
use sn_transfers::NanoTokens;
use std::{ffi::OsString, net::SocketAddr, path::PathBuf, str::FromStr};

pub struct NodeService<'a> {
    pub service_data: &'a mut NodeServiceData,
    pub rpc_actions: Box<dyn RpcActions + Send>,
}

impl<'a> NodeService<'a> {
    pub fn new(
        service_data: &'a mut NodeServiceData,
        rpc_actions: Box<dyn RpcActions + Send>,
    ) -> NodeService<'a> {
        NodeService {
            rpc_actions,
            service_data,
        }
    }
}

#[async_trait]
impl<'a> ServiceStateActions for NodeService<'a> {
    fn bin_path(&self) -> PathBuf {
        self.service_data.safenode_path.clone()
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

        if self.service_data.genesis {
            args.push(OsString::from("--first"));
        }
        if self.service_data.local {
            args.push(OsString::from("--local"));
        }
        if let Some(node_port) = self.service_data.get_safenode_port() {
            args.push(OsString::from("--port"));
            args.push(OsString::from(node_port.to_string()));
        }

        if !options.bootstrap_peers.is_empty() {
            let peers_str = options
                .bootstrap_peers
                .iter()
                .map(|peer| peer.to_string())
                .collect::<Vec<_>>()
                .join(",");
            args.push(OsString::from("--peer"));
            args.push(OsString::from(peers_str));
        }

        Ok(ServiceInstallCtx {
            label: label.clone(),
            program: self.service_data.safenode_path.to_path_buf(),
            args,
            contents: None,
            username: self.service_data.user.clone(),
            working_directory: None,
            environment: options.env_variables,
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

    async fn on_start(&mut self) -> Result<()> {
        let node_info = self.rpc_actions.node_info().await?;
        let network_info = self.rpc_actions.network_info().await?;
        self.service_data.listen_addr = Some(
            network_info
                .listeners
                .into_iter()
                .map(|addr| addr.with(Protocol::P2p(node_info.peer_id)))
                .collect(),
        );
        self.service_data.pid = Some(node_info.pid);
        self.service_data.peer_id = Some(node_info.peer_id);
        self.service_data.status = ServiceStatus::Running;
        Ok(())
    }

    async fn on_stop(&mut self) -> Result<()> {
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
    #[serde(
        serialize_with = "serialize_connected_peers",
        deserialize_with = "deserialize_connected_peers"
    )]
    pub connected_peers: Option<Vec<PeerId>>,
    pub data_dir_path: PathBuf,
    pub genesis: bool,
    pub home_network: bool,
    pub listen_addr: Option<Vec<Multiaddr>>,
    pub local: bool,
    pub log_dir_path: PathBuf,
    pub number: u16,
    #[serde(
        serialize_with = "serialize_peer_id",
        deserialize_with = "deserialize_peer_id"
    )]
    pub peer_id: Option<PeerId>,
    pub pid: Option<u32>,
    pub reward_balance: Option<NanoTokens>,
    pub rpc_socket_addr: SocketAddr,
    pub safenode_path: PathBuf,
    pub service_name: String,
    pub status: ServiceStatus,
    pub user: Option<String>,
    pub user_mode: bool,
    pub version: String,
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
    pub fn get_safenode_port(&self) -> Option<u16> {
        // assuming the listening addr contains /ip4/127.0.0.1/udp/56215/quic-v1/p2p/<peer_id>
        if let Some(multi_addrs) = &self.listen_addr {
            for addr in multi_addrs {
                if let Some(port) = get_port_from_multiaddr(addr) {
                    return Some(port);
                }
            }
        }
        None
    }
}
