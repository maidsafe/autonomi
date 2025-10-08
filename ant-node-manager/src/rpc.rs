// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    ServiceManager, VerbosityLevel, add_services::config::InstallNodeServiceCtxBuilder,
    config::create_owned_dir,
};
use ant_service_management::{
    NodeRegistryManager, NodeService, NodeServiceData, ServiceStatus,
    control::{ServiceControl, ServiceController},
    node::NODE_SERVICE_DATA_SCHEMA_LATEST,
    rpc::RpcClient,
};
use color_eyre::{
    Result,
    eyre::{OptionExt, eyre},
};
use libp2p::PeerId;
use std::sync::Arc;

pub async fn restart_node_service(
    node_registry: NodeRegistryManager,
    peer_id: PeerId,
    retain_peer_id: bool,
) -> Result<()> {
    let nodes_len = node_registry.nodes.read().await.len();
    let mut current_node = None;

    for node in node_registry.nodes.read().await.iter() {
        if node.read().await.peer_id.is_some_and(|id| id == peer_id) {
            current_node = Some(Arc::clone(node));
            break;
        }
    }

    let current_node = current_node.ok_or_else(|| {
        error!("Could not find the provided PeerId: {peer_id:?}");
        eyre!("Could not find the provided PeerId: {peer_id:?}")
    })?;

    let rpc_client = RpcClient::from_socket_addr(current_node.read().await.rpc_socket_addr);
    let service = NodeService::new(Arc::clone(&current_node), Box::new(rpc_client));
    let mut service_manager = ServiceManager::new(
        service,
        Box::new(ServiceController {}),
        VerbosityLevel::Normal,
    );
    service_manager.stop().await?;
    let service_name = current_node.read().await.service_name.clone();

    let service_control = ServiceController {};
    if retain_peer_id {
        debug!("Retaining the peer id: {peer_id:?} for the node: {service_name:?}");
        // reuse the same port and root dir to retain peer id.
        service_control
            .uninstall(&service_name, false)
            .map_err(
                |err| eyre!("Error while uninstalling node {service_name:?} with: {err:?}",),
            )?;
        let current_node_clone = current_node.read().await.clone();
        let install_ctx = InstallNodeServiceCtxBuilder {
            alpha: current_node_clone.alpha,
            antnode_path: current_node_clone.antnode_path.clone(),
            autostart: current_node_clone.auto_restart,
            data_dir_path: current_node_clone.data_dir_path.clone(),
            env_variables: node_registry.environment_variables.read().await.clone(),
            evm_network: current_node_clone.evm_network.clone(),
            relay: current_node_clone.relay,
            init_peers_config: current_node_clone.initial_peers_config.clone(),
            log_dir_path: current_node_clone.log_dir_path.clone(),
            log_format: current_node_clone.log_format,
            max_archived_log_files: current_node_clone.max_archived_log_files,
            max_log_files: current_node_clone.max_log_files,
            metrics_port: None,
            name: current_node_clone.service_name.clone(),
            network_id: current_node_clone.network_id,
            node_ip: current_node_clone.node_ip,
            node_port: current_node_clone.get_antnode_port(),
            no_upnp: current_node_clone.no_upnp,
            reachability_check: current_node_clone.reachability_check,
            rewards_address: current_node_clone.rewards_address,
            rpc_socket_addr: current_node_clone.rpc_socket_addr,
            service_user: current_node_clone.user.clone(),
            write_older_cache_files: current_node_clone.write_older_cache_files,
        }
        .build()?;

        service_control
            .install(install_ctx, false)
            .map_err(|err| eyre!("Error while installing node {service_name:?} with: {err:?}",))?;
        service_manager.start().await?;
    } else {
        let current_node_clone = current_node.read().await.clone();
        debug!("Starting a new node since retain peer id is false.");
        let new_node_number = nodes_len + 1;
        let new_service_name = format!("antnode{new_node_number}");

        // example path "log_dir_path":"/var/log/antnode/antnode18"
        let log_dir_path = {
            let mut log_dir_path = current_node_clone.log_dir_path.clone();
            log_dir_path.pop();
            log_dir_path.join(&new_service_name)
        };
        // example path "data_dir_path":"/var/antctl/services/antnode18"
        let data_dir_path = {
            let mut data_dir_path = current_node_clone.data_dir_path.clone();
            data_dir_path.pop();
            data_dir_path.join(&new_service_name)
        };

        create_owned_dir(
            log_dir_path.clone(),
            current_node_clone.user.as_ref().ok_or_else(|| {
                error!("The user must be set in the RPC context");
                eyre!("The user must be set in the RPC context")
            })?,
        )
        .map_err(|err| {
            error!(
                "Error while creating owned dir for {:?}: {err:?}",
                current_node_clone.user
            );
            eyre!(
                "Error while creating owned dir for {:?}: {err:?}",
                current_node_clone.user
            )
        })?;
        debug!("Created data dir: {data_dir_path:?} for the new node");
        create_owned_dir(
            data_dir_path.clone(),
            current_node_clone
                .user
                .as_ref()
                .ok_or_else(|| eyre!("The user must be set in the RPC context"))?,
        )
        .map_err(|err| {
            eyre!(
                "Error while creating owned dir for {:?}: {err:?}",
                current_node_clone.user
            )
        })?;
        // example path "antnode_path":"/var/antctl/services/antnode18/antnode"
        let antnode_path = {
            debug!("Copying antnode binary");
            let mut antnode_path = current_node_clone.antnode_path.clone();
            let antnode_file_name = antnode_path
                .file_name()
                .ok_or_eyre("Could not get filename from the current node's antnode path")?
                .to_string_lossy()
                .to_string();
            antnode_path.pop();
            antnode_path.pop();

            let antnode_path = antnode_path.join(&new_service_name);
            create_owned_dir(
                data_dir_path.clone(),
                current_node_clone
                    .user
                    .as_ref()
                    .ok_or_else(|| eyre!("The user must be set in the RPC context"))?,
            )
            .map_err(|err| {
                eyre!(
                    "Error while creating owned dir for {:?}: {err:?}",
                    current_node_clone.user
                )
            })?;
            let antnode_path = antnode_path.join(antnode_file_name);

            std::fs::copy(&current_node_clone.antnode_path, &antnode_path).map_err(|err| {
                eyre!(
                    "Failed to copy antnode bin from {:?} to {antnode_path:?} with err: {err}",
                    current_node_clone.antnode_path
                )
            })?;
            antnode_path
        };

        let install_ctx = InstallNodeServiceCtxBuilder {
            alpha: current_node_clone.alpha,
            autostart: current_node_clone.auto_restart,
            data_dir_path: data_dir_path.clone(),
            env_variables: node_registry.environment_variables.read().await.clone(),
            evm_network: current_node_clone.evm_network.clone(),
            relay: current_node_clone.relay,
            init_peers_config: current_node_clone.initial_peers_config.clone(),
            log_dir_path: log_dir_path.clone(),
            log_format: current_node_clone.log_format,
            name: new_service_name.clone(),
            max_archived_log_files: current_node_clone.max_archived_log_files,
            max_log_files: current_node_clone.max_log_files,
            metrics_port: None,
            network_id: current_node_clone.network_id,
            node_ip: current_node_clone.node_ip,
            node_port: None,
            no_upnp: current_node_clone.no_upnp,
            reachability_check: current_node_clone.reachability_check,
            rewards_address: current_node_clone.rewards_address,
            rpc_socket_addr: current_node_clone.rpc_socket_addr,
            antnode_path: antnode_path.clone(),
            service_user: current_node_clone.user.clone(),
            write_older_cache_files: current_node_clone.write_older_cache_files,
        }
        .build()?;
        service_control.install(install_ctx, false).map_err(|err| {
            eyre!("Error while installing node {new_service_name:?} with: {err:?}",)
        })?;

        let node = NodeServiceData {
            alpha: current_node_clone.alpha,
            antnode_path,
            auto_restart: current_node_clone.auto_restart,
            connected_peers: None,
            data_dir_path,
            evm_network: current_node_clone.evm_network,
            relay: current_node_clone.relay,
            initial_peers_config: current_node_clone.initial_peers_config.clone(),
            listen_addr: None,
            log_dir_path,
            log_format: current_node_clone.log_format,
            max_archived_log_files: current_node_clone.max_archived_log_files,
            max_log_files: current_node_clone.max_log_files,
            metrics_port: None,
            network_id: current_node_clone.network_id,
            node_ip: current_node_clone.node_ip,
            node_port: None,
            no_upnp: current_node_clone.no_upnp,
            number: new_node_number as u16,
            peer_id: None,
            pid: None,
            reachability_check: current_node_clone.reachability_check,
            rewards_address: current_node_clone.rewards_address,
            reward_balance: current_node_clone.reward_balance,
            rpc_socket_addr: current_node_clone.rpc_socket_addr,
            schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
            service_name: new_service_name.clone(),
            status: ServiceStatus::Added,
            user: current_node_clone.user.clone(),
            user_mode: false,
            version: current_node_clone.version.clone(),
            write_older_cache_files: current_node_clone.write_older_cache_files,
        };

        let rpc_client = RpcClient::from_socket_addr(node.rpc_socket_addr);
        let service = NodeService::new(Arc::clone(&current_node), Box::new(rpc_client));
        let mut service_manager = ServiceManager::new(
            service,
            Box::new(ServiceController {}),
            VerbosityLevel::Normal,
        );
        service_manager.start().await?;
        node_registry
            .push_node(service_manager.service.service_data.read().await.clone())
            .await;
    };

    Ok(())
}
