// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::error::{Error, Result};
use ant_protocol::antnode_proto::{
    NetworkInfoRequest, NodeInfoRequest, RecordAddressesRequest, RestartRequest, StopRequest,
    UpdateLogLevelRequest, UpdateRequest, ant_node_client::AntNodeClient,
};
use async_trait::async_trait;
use libp2p::{Multiaddr, PeerId, kad::RecordKey};
use std::{net::SocketAddr, path::PathBuf, str::FromStr};
use tokio::time::Duration;
use tonic::Request;
use tracing::error;

#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub pid: u32,
    pub peer_id: PeerId,
    pub log_path: PathBuf,
    pub data_path: PathBuf,
    pub version: String,
    pub uptime: Duration,
    pub wallet_balance: u64,
}

#[derive(Debug, Clone)]
pub struct NetworkInfo {
    pub connected_peers: Vec<PeerId>,
    pub listeners: Vec<Multiaddr>,
}

#[derive(Debug, Clone)]
pub struct RecordAddress {
    pub key: RecordKey,
}

#[async_trait]
pub trait RpcActions: Sync {
    async fn node_info(&self) -> Result<NodeInfo>;
    async fn network_info(&self) -> Result<NetworkInfo>;
    async fn record_addresses(&self) -> Result<Vec<RecordAddress>>;
    async fn node_restart(&self, delay_millis: u64, retain_peer_id: bool) -> Result<()>;
    async fn node_stop(&self, delay_millis: u64) -> Result<()>;
    async fn node_update(&self, delay_millis: u64) -> Result<()>;
    async fn is_node_connected_to_network(&self, timeout: Duration) -> Result<()>;
    async fn update_log_level(&self, log_levels: String) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct RpcClient {
    endpoint: String,
    max_attempts: u8,
    retry_delay: Duration,
}

impl RpcClient {
    const MAX_CONNECTION_RETRY_ATTEMPTS: u8 = 5;
    const CONNECTION_RETRY_DELAY_SEC: Duration = Duration::from_secs(1);

    pub fn new(endpoint: &str) -> Self {
        Self {
            endpoint: endpoint.to_string(),
            max_attempts: Self::MAX_CONNECTION_RETRY_ATTEMPTS,
            retry_delay: Self::CONNECTION_RETRY_DELAY_SEC,
        }
    }

    pub fn from_socket_addr(socket: SocketAddr) -> Self {
        let endpoint = format!("https://{socket}");
        Self::new(&endpoint)
    }

    /// Set the maximum number of retry attempts when connecting to the RPC endpoint. Default is 5.
    pub fn set_max_attempts(&mut self, max_retry_attempts: u8) {
        self.max_attempts = max_retry_attempts;
    }

    /// Set the delay between retry attempts when connecting to the RPC endpoint. Default is 1 second.
    pub fn set_retry_delay(&mut self, retry_delay: Duration) {
        self.retry_delay = retry_delay;
    }

    // Connect to the RPC endpoint with retry
    async fn connect_with_retry(&self) -> Result<AntNodeClient<tonic::transport::Channel>> {
        let mut attempts = 0;
        loop {
            debug!(
                "Attempting connection to node RPC endpoint at {}...",
                self.endpoint
            );
            match AntNodeClient::connect(self.endpoint.clone()).await {
                Ok(rpc_client) => {
                    debug!("Connection successful");
                    break Ok(rpc_client);
                }
                Err(_) => {
                    attempts += 1;
                    tokio::time::sleep(self.retry_delay).await;
                    if attempts >= self.max_attempts {
                        return Err(Error::RpcConnectionError(self.endpoint.clone()));
                    }
                    error!(
                        "Could not connect to RPC endpoint {:?}. Retrying {attempts}/{}",
                        self.endpoint, self.max_attempts
                    );
                }
            }
        }
    }
}

#[async_trait]
impl RpcActions for RpcClient {
    async fn node_info(&self) -> Result<NodeInfo> {
        let mut client = self.connect_with_retry().await?;
        let response = client
            .node_info(Request::new(NodeInfoRequest {}))
            .await
            .map_err(|e| {
                error!("Could not obtain node info through RPC: {e:?}");
                Error::RpcNodeInfoError(e.to_string())
            })?;
        let node_info_resp = response.get_ref();
        let peer_id = PeerId::from_bytes(&node_info_resp.peer_id)?;
        let node_info = NodeInfo {
            pid: node_info_resp.pid,
            peer_id,
            log_path: PathBuf::from(node_info_resp.log_dir.clone()),
            data_path: PathBuf::from(node_info_resp.data_dir.clone()),
            version: node_info_resp.bin_version.clone(),
            uptime: Duration::from_secs(node_info_resp.uptime_secs),
            wallet_balance: node_info_resp.wallet_balance,
        };
        Ok(node_info)
    }
    async fn network_info(&self) -> Result<NetworkInfo> {
        let mut client = self.connect_with_retry().await?;
        let response = client
            .network_info(Request::new(NetworkInfoRequest {}))
            .await
            .map_err(|e| {
                error!("Could not obtain network info through RPC: {e:?}");
                Error::RpcNodeInfoError(e.to_string())
            })?;
        let network_info = response.get_ref();

        let mut connected_peers = Vec::new();
        for bytes in network_info.connected_peers.iter() {
            let peer_id = PeerId::from_bytes(bytes)?;
            connected_peers.push(peer_id);
        }

        let mut listeners = Vec::new();
        for multiaddr_str in network_info.listeners.iter() {
            let multiaddr = Multiaddr::from_str(multiaddr_str)?;
            listeners.push(multiaddr);
        }

        Ok(NetworkInfo {
            connected_peers,
            listeners,
        })
    }

    async fn record_addresses(&self) -> Result<Vec<RecordAddress>> {
        let mut client = self.connect_with_retry().await?;
        let response = client
            .record_addresses(Request::new(RecordAddressesRequest {}))
            .await
            .map_err(|e| {
                error!("Could not obtain record addresses through RPC: {e:?}");
                Error::RpcRecordAddressError(e.to_string())
            })?;
        let mut record_addresses = vec![];
        for bytes in response.get_ref().addresses.iter() {
            let key = libp2p::kad::RecordKey::from(bytes.clone());
            record_addresses.push(RecordAddress { key });
        }
        Ok(record_addresses)
    }

    async fn node_restart(&self, delay_millis: u64, retain_peer_id: bool) -> Result<()> {
        let mut client = self.connect_with_retry().await?;
        let _response = client
            .restart(Request::new(RestartRequest {
                delay_millis,
                retain_peer_id,
            }))
            .await
            .map_err(|e| {
                error!("Could not restart node through RPC: {e:?}");
                Error::RpcNodeRestartError(e.to_string())
            })?;
        Ok(())
    }

    async fn node_stop(&self, delay_millis: u64) -> Result<()> {
        let mut client = self.connect_with_retry().await?;
        let _response = client
            .stop(Request::new(StopRequest { delay_millis }))
            .await
            .map_err(|e| {
                error!("Could not restart node through RPC: {e:?}");
                Error::RpcNodeStopError(e.to_string())
            })?;
        Ok(())
    }

    async fn node_update(&self, delay_millis: u64) -> Result<()> {
        let mut client = self.connect_with_retry().await?;
        let _response = client
            .update(Request::new(UpdateRequest { delay_millis }))
            .await
            .map_err(|e| {
                error!("Could not update node through RPC: {e:?}");
                Error::RpcNodeUpdateError(e.to_string())
            })?;
        Ok(())
    }

    async fn is_node_connected_to_network(&self, timeout: Duration) -> Result<()> {
        let max_attempts = std::cmp::max(1, timeout.as_secs() / self.retry_delay.as_secs());
        trace!(
            "RPC conneciton max attempts set to: {max_attempts} with retry_delay of {:?}",
            self.retry_delay
        );
        let mut attempts = 0;
        loop {
            debug!(
                "Attempting connection to node RPC endpoint at {}...",
                self.endpoint
            );
            if let Ok(mut client) = AntNodeClient::connect(self.endpoint.clone()).await {
                debug!("Connection to RPC successful");
                if client
                    .network_info(Request::new(NetworkInfoRequest {}))
                    .await
                    .is_ok()
                {
                    return Ok(());
                } else {
                    error!(
                        "Could not obtain NetworkInfo through RPC. Retrying {attempts}/{max_attempts}"
                    );
                }
            } else {
                error!(
                    "Could not connect to RPC endpoint {:?}. Retrying {attempts}/{max_attempts}",
                    self.endpoint
                );
            }

            attempts += 1;
            tokio::time::sleep(self.retry_delay).await;
            if attempts >= max_attempts {
                return Err(Error::RpcConnectionError(self.endpoint.clone()));
            }
        }
    }

    async fn update_log_level(&self, log_levels: String) -> Result<()> {
        let mut client = self.connect_with_retry().await?;
        let _response = client
            .update_log_level(Request::new(UpdateLogLevelRequest {
                log_level: log_levels,
            }))
            .await
            .map_err(|e| {
                error!("Could not update node through RPC: {e:?}");
                Error::RpcNodeUpdateError(e.to_string())
            })?;
        Ok(())
    }
}
