// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod node_service_data;
mod node_service_data_v0;
mod node_service_data_v1;
mod node_service_data_v2;
mod node_service_data_v3;

// Re-export types
pub use node_service_data::{NODE_SERVICE_DATA_SCHEMA_LATEST, NodeServiceData};

use crate::{
    ServiceStateActions, ServiceStatus, UpgradeOptions, control::ServiceControl, error::Result,
    metric::MetricsAction, rpc::RpcActions,
};
use ant_bootstrap::InitialPeersConfig;
use ant_evm::EvmNetwork;
use ant_protocol::get_port_from_multiaddr;
use libp2p::multiaddr::Protocol;
use service_manager::{ServiceInstallCtx, ServiceLabel};
use std::{ffi::OsString, path::PathBuf, sync::Arc};
use tokio::sync::RwLock;
use tonic::async_trait;

pub struct NodeService {
    pub service_data: Arc<RwLock<NodeServiceData>>,
    pub metrics_action: Box<dyn MetricsAction + Send>,
    pub rpc_actions: Box<dyn RpcActions + Send>,
}

impl NodeService {
    pub fn new(
        service_data: Arc<RwLock<NodeServiceData>>,
        rpc_actions: Box<dyn RpcActions + Send>,
        metrics_action: Box<dyn MetricsAction + Send>,
    ) -> NodeService {
        NodeService {
            rpc_actions,
            metrics_action,
            service_data,
        }
    }
}

#[async_trait]
impl ServiceStateActions for NodeService {
    async fn bin_path(&self) -> PathBuf {
        self.service_data.read().await.antnode_path.clone()
    }

    async fn build_upgrade_install_context(
        &self,
        options: UpgradeOptions,
    ) -> Result<ServiceInstallCtx> {
        let service_data = self.service_data.read().await;
        let label: ServiceLabel = service_data.service_name.parse()?;
        let mut args = vec![
            OsString::from("--rpc"),
            OsString::from(service_data.rpc_socket_addr.to_string()),
            OsString::from("--root-dir"),
            OsString::from(service_data.data_dir_path.to_string_lossy().to_string()),
            OsString::from("--log-output-dest"),
            OsString::from(service_data.log_dir_path.to_string_lossy().to_string()),
        ];

        push_arguments_from_initial_peers_config(&service_data.initial_peers_config, &mut args);
        if let Some(log_fmt) = service_data.log_format {
            args.push(OsString::from("--log-format"));
            args.push(OsString::from(log_fmt.as_str()));
        }
        if let Some(id) = service_data.network_id {
            args.push(OsString::from("--network-id"));
            args.push(OsString::from(id.to_string()));
        }
        if service_data.reachability_check {
            args.push(OsString::from("--reachability-check"));
        }
        if service_data.no_upnp {
            args.push(OsString::from("--no-upnp"));
        }
        if service_data.relay {
            args.push(OsString::from("--relay"));
        }

        if service_data.alpha {
            args.push(OsString::from("--alpha"));
        }

        if let Some(node_ip) = service_data.node_ip {
            args.push(OsString::from("--ip"));
            args.push(OsString::from(node_ip.to_string()));
        }

        if let Some(node_port) = service_data.node_port {
            args.push(OsString::from("--port"));
            args.push(OsString::from(node_port.to_string()));
        }

        if let Some(metrics_port) = service_data.metrics_port {
            args.push(OsString::from("--metrics-server-port"));
            args.push(OsString::from(metrics_port.to_string()));
        } else {
            error!(
                "Metrics port not available during upgrade_install_context. Make sure to call ServiceStateActions::set_metrics_port_if_not_set before building context"
            );
        }

        if let Some(max_archived_log_files) = service_data.max_archived_log_files {
            args.push(OsString::from("--max-archived-log-files"));
            args.push(OsString::from(max_archived_log_files.to_string()));
        }
        if let Some(max_log_files) = service_data.max_log_files {
            args.push(OsString::from("--max-log-files"));
            args.push(OsString::from(max_log_files.to_string()));
        }

        args.push(OsString::from("--rewards-address"));
        args.push(OsString::from(service_data.rewards_address.to_string()));

        if service_data.write_older_cache_files {
            args.push(OsString::from("--write-older-cache-files"));
        }

        args.push(OsString::from(service_data.evm_network.to_string()));
        if let EvmNetwork::Custom(custom_network) = &service_data.evm_network {
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
            program: service_data.antnode_path.to_path_buf(),
            username: service_data.user.clone(),
            working_directory: None,
            disable_restart_on_failure: true,
        })
    }

    async fn data_dir_path(&self) -> PathBuf {
        self.service_data.read().await.data_dir_path.clone()
    }

    async fn is_user_mode(&self) -> bool {
        self.service_data.read().await.user_mode
    }

    async fn log_dir_path(&self) -> PathBuf {
        self.service_data.read().await.log_dir_path.clone()
    }

    async fn name(&self) -> String {
        self.service_data.read().await.service_name.clone()
    }

    async fn pid(&self) -> Option<u32> {
        self.service_data.read().await.pid
    }

    async fn on_remove(&self) {
        self.service_data.write().await.status = ServiceStatus::Removed;
    }

    async fn on_start(&self, pid: Option<u32>, full_refresh: bool) -> Result<()> {
        let service_name = self.service_data.read().await.service_name.clone();
        let (connected_peers, pid, peer_id) = if full_refresh {
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

            self.service_data.write().await.listen_addr = Some(
                network_info
                    .listeners
                    .iter()
                    .cloned()
                    .map(|addr| addr.with(Protocol::P2p(node_info.peer_id)))
                    .collect(),
            );
            for addr in &network_info.listeners {
                if let Some(port) = get_port_from_multiaddr(addr) {
                    debug!("Found antnode port for {service_name}: {port}");
                    self.service_data.write().await.node_port = Some(port);
                    break;
                }
            }

            if self.service_data.read().await.node_port.is_none() {
                error!("Could not find antnode port");
                error!("This will cause the node to have a different port during upgrade");
            }

            (
                Some(network_info.connected_peers),
                Some(node_info.pid),
                Some(node_info.peer_id),
            )
        } else {
            debug!("Performing partial refresh for {service_name}");
            debug!("Previously assigned data will be used");
            (
                self.service_data.read().await.connected_peers.clone(),
                pid,
                self.service_data.read().await.peer_id,
            )
        };

        self.service_data.write().await.connected_peers = connected_peers;
        self.service_data.write().await.peer_id = peer_id;
        self.service_data.write().await.pid = pid;
        self.service_data.write().await.status = ServiceStatus::Running;
        Ok(())
    }

    async fn wait_until_started(&self) -> Result<()> {
        let service_name = self.service_data.read().await.service_name.clone();
        info!("Waiting for {service_name} to complete reachability check");
        self.metrics_action
            .wait_until_reachability_check_completes(None)
            .await?;

        info!(
            "Reachability check completed for {service_name}. Now waiting for the node to connect to the network"
        );

        self.rpc_actions
            .wait_until_node_connects_to_network(None)
            .await?;
        info!("{service_name} is now connected to the network");
        Ok(())
    }

    async fn on_stop(&self) -> Result<()> {
        let mut service_data = self.service_data.write().await;
        debug!("Marking {} as stopped", service_data.service_name);
        service_data.pid = None;
        service_data.status = ServiceStatus::Stopped;
        service_data.connected_peers = None;
        Ok(())
    }

    async fn set_version(&self, version: &str) {
        self.service_data.write().await.version = version.to_string();
    }

    async fn status(&self) -> ServiceStatus {
        self.service_data.read().await.status.clone()
    }

    async fn version(&self) -> String {
        self.service_data.read().await.version.clone()
    }

    async fn set_metrics_port_if_not_set(
        &self,
        service_control: &dyn ServiceControl,
    ) -> Result<()> {
        if self.service_data.read().await.metrics_port.is_none() {
            info!(
                "Setting port for {} as it does not have any",
                self.service_data.read().await.service_name
            );
            let port = service_control.get_available_port()?;
            self.service_data.write().await.metrics_port = Some(port);
        }

        Ok(())
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
    if init_peers_config.ignore_cache {
        args.push(OsString::from("--ignore-cache"));
    }
    if let Some(path) = &init_peers_config.bootstrap_cache_dir {
        args.push(OsString::from("--bootstrap-cache-dir"));
        args.push(OsString::from(path.to_string_lossy().to_string()));
    }
}
