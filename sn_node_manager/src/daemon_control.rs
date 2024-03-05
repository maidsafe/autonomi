// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{node_control, service::ServiceControl, VerbosityLevel};
use color_eyre::Result;
use libp2p::Multiaddr;
use service_manager::{ServiceInstallCtx, ServiceLabel};
use sn_node_rpc_client::RpcActions;
use sn_protocol::node_registry::Node;
use std::{ffi::OsString, net::Ipv4Addr, path::PathBuf};

pub fn run(
    address: Ipv4Addr,
    port: u16,
    daemon_path: PathBuf,
    service_control: &dyn ServiceControl,
    _verbosity: VerbosityLevel,
) -> Result<()> {
    let service_name: ServiceLabel = "safenode-manager-daemon".parse()?;

    let install_ctx = ServiceInstallCtx {
        label: service_name.clone(),
        program: daemon_path,
        args: vec![
            OsString::from("--port"),
            OsString::from(port.to_string()),
            OsString::from("--address"),
            OsString::from(address.to_string()),
        ],
        contents: None,
        username: None,
        working_directory: None,
        environment: None,
    };
    service_control.install(install_ctx)?;
    service_control.start(&service_name.to_string())?;

    Ok(())
}

pub async fn restart_safenode(
    node: &mut Node,
    rpc_client: &dyn RpcActions,
    bootstrap_peers: Vec<Multiaddr>,
    env_variables: Option<Vec<(String, String)>>,
    service_control: &dyn ServiceControl,
) -> Result<()> {
    node_control::stop(node, service_control).await?;

    service_control.uninstall(&node.service_name.clone())?;
    let install_ctx = node_control::InstallNodeServiceCtxBuilder {
        local: node.local,
        data_dir_path: node.data_dir_path.clone(),
        genesis: node.genesis,
        name: node.service_name.clone(),
        node_port: node.get_safenode_port(),
        bootstrap_peers,
        rpc_socket_addr: node.rpc_socket_addr,
        log_dir_path: node.log_dir_path.clone(),
        safenode_path: node.safenode_path.clone(),
        service_user: node.user.clone(),
        env_variables,
    }
    .build()?;
    service_control.install(install_ctx)?;

    node_control::start(node, service_control, rpc_client, VerbosityLevel::Normal).await?;
    Ok(())
}
