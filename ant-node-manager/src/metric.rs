// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    add_services::config::InstallNodeServiceCtxBuilder, config::create_owned_dir, error::Error,
    ServiceManager, VerbosityLevel,
};
use ant_service_management::{
    control::{ServiceControl, ServiceController},
    metric::MetricClient,
    NodeRegistry, NodeService, NodeServiceData, ServiceStatus,
};
use color_eyre::{
    eyre::{eyre, OptionExt},
    Result,
};
use libp2p::PeerId;

pub async fn restart_node_service(
    node_registry: &mut NodeRegistry,
    peer_id: PeerId,
    retain_peer_id: bool,
) -> Result<()> {
    let nodes_len = node_registry.nodes.len();
    let current_node_mut = node_registry
        .nodes
        .iter_mut()
        .find(|node| node.peer_id.is_some_and(|id| id == peer_id))
        .ok_or_eyre({
            error!("Could not find the provided PeerId: {peer_id:?}");
            format!("Could not find the provided PeerId: {peer_id:?}")
        })?;
    let current_node_clone = current_node_mut.clone();

    let metrics_port = current_node_mut
        .metrics_port
        .ok_or(Error::MetricPortEmpty)?;
    let metric_client = MetricClient::new(metrics_port);
    let service = NodeService::new(current_node_mut, Box::new(metric_client));

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(ServiceController {}),
        VerbosityLevel::Normal,
    );
    service_manager.stop().await?;

    let service_control = ServiceController {};
    if retain_peer_id {
        debug!(
            "Retaining the peer id: {peer_id:?} for the node: {:?}",
            current_node_clone.service_name
        );
        // reuse the same port and root dir to retain peer id.
        service_control
            .uninstall(&current_node_clone.service_name, false)
            .map_err(|err| {
                eyre!(
                    "Error while uninstalling node {:?} with: {err:?}",
                    current_node_clone.service_name
                )
            })?;
        let install_ctx = InstallNodeServiceCtxBuilder {
            antnode_path: current_node_clone.antnode_path.clone(),
            autostart: current_node_clone.auto_restart,
            data_dir_path: current_node_clone.data_dir_path.clone(),
            env_variables: node_registry.environment_variables.clone(),
            evm_network: current_node_clone.evm_network.clone(),
            home_network: current_node_clone.home_network,
            log_dir_path: current_node_clone.log_dir_path.clone(),
            log_format: current_node_clone.log_format,
            max_archived_log_files: current_node_clone.max_archived_log_files,
            max_log_files: current_node_clone.max_log_files,
            metrics_port: None,
            name: current_node_clone.service_name.clone(),
            network_id: current_node_clone.network_id,
            node_ip: current_node_clone.node_ip,
            node_port: current_node_clone.get_antnode_port(),
            init_peers_config: current_node_clone.peers_args.clone(),
            rewards_address: current_node_clone.rewards_address,
            service_user: current_node_clone.user.clone(),
            upnp: current_node_clone.upnp,
        }
        .build()?;
        service_control.install(install_ctx, false).map_err(|err| {
            eyre!(
                "Error while installing node {:?} with: {err:?}",
                current_node_clone.service_name
            )
        })?;
        service_manager.start().await?;
    } else {
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
                error!("The user must be set in the Metric context");
                eyre!("The user must be set in the Metric context")
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
                .ok_or_else(|| eyre!("The user must be set in the Metric context"))?,
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
                    .ok_or_else(|| eyre!("The user must be set in the Metric context"))?,
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
            autostart: current_node_clone.auto_restart,
            data_dir_path: data_dir_path.clone(),
            env_variables: node_registry.environment_variables.clone(),
            evm_network: current_node_clone.evm_network.clone(),
            home_network: current_node_clone.home_network,
            log_dir_path: log_dir_path.clone(),
            log_format: current_node_clone.log_format,
            name: new_service_name.clone(),
            max_archived_log_files: current_node_clone.max_archived_log_files,
            max_log_files: current_node_clone.max_log_files,
            metrics_port: None,
            network_id: current_node_clone.network_id,
            node_ip: current_node_clone.node_ip,
            node_port: None,
            init_peers_config: current_node_clone.peers_args.clone(),
            rewards_address: current_node_clone.rewards_address,
            antnode_path: antnode_path.clone(),
            service_user: current_node_clone.user.clone(),
            upnp: current_node_clone.upnp,
        }
        .build()?;
        service_control.install(install_ctx, false).map_err(|err| {
            eyre!("Error while installing node {new_service_name:?} with: {err:?}",)
        })?;

        let mut node = NodeServiceData {
            antnode_path,
            auto_restart: current_node_clone.auto_restart,
            connected_peers: None,
            data_dir_path,
            evm_network: current_node_clone.evm_network,
            home_network: current_node_clone.home_network,
            listen_addr: None,
            log_dir_path,
            log_format: current_node_clone.log_format,
            max_archived_log_files: current_node_clone.max_archived_log_files,
            max_log_files: current_node_clone.max_log_files,
            metrics_port: None,
            network_id: current_node_clone.network_id,
            node_ip: current_node_clone.node_ip,
            node_port: None,
            number: new_node_number as u16,
            peer_id: None,
            peers_args: current_node_clone.peers_args.clone(),
            pid: None,
            rewards_address: current_node_clone.rewards_address,
            reward_balance: current_node_clone.reward_balance,
            rpc_socket_addr: current_node_clone.rpc_socket_addr,
            service_name: new_service_name.clone(),
            status: ServiceStatus::Added,
            upnp: current_node_clone.upnp,
            user: current_node_clone.user.clone(),
            user_mode: false,
            version: current_node_clone.version.clone(),
        };

        let metrics_port = node.metrics_port.ok_or(Error::MetricPortEmpty)?;
        let metric_client = MetricClient::new(metrics_port);
        let service = NodeService::new(&mut node, Box::new(metric_client));

        let mut service_manager = ServiceManager::new(
            service,
            Box::new(ServiceController {}),
            VerbosityLevel::Normal,
        );
        service_manager.start().await?;
        node_registry
            .nodes
            .push(service_manager.service.service_data.clone());
    };

    Ok(())
}
