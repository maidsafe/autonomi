// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use color_eyre::{eyre::eyre, Result};
use libp2p::Multiaddr;
use service_manager::{ServiceInstallCtx, ServiceLabel};
use sn_logging::LogFormat;
use std::{
    ffi::OsString,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    str::FromStr,
};

#[derive(Clone, Debug)]
pub enum PortRange {
    Single(u16),
    Range(u16, u16),
}

impl PortRange {
    pub fn parse(s: &str) -> Result<Self> {
        if let Ok(port) = u16::from_str(s) {
            Ok(Self::Single(port))
        } else {
            let parts: Vec<&str> = s.split('-').collect();
            if parts.len() != 2 {
                return Err(eyre!("Port range must be in the format 'start-end'"));
            }
            let start = parts[0].parse::<u16>()?;
            let end = parts[1].parse::<u16>()?;
            if start >= end {
                return Err(eyre!("End port must be greater than start port"));
            }
            Ok(Self::Range(start, end))
        }
    }

    /// Validate the port range against a count to make sure the correct number of ports are provided.
    pub fn validate(&self, count: u16) -> Result<()> {
        match self {
            Self::Single(_) => {
                if count != 1 {
                    error!("The count ({count}) does not match the number of ports (1)");
                    return Err(eyre!(
                        "The count ({count}) does not match the number of ports (1)"
                    ));
                }
            }
            Self::Range(start, end) => {
                let port_count = end - start + 1;
                if count != port_count {
                    error!("The count ({count}) does not match the number of ports ({port_count})");
                    return Err(eyre!(
                        "The count ({count}) does not match the number of ports ({port_count})"
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq)]
pub struct InstallNodeServiceCtxBuilder {
    pub autostart: bool,
    pub bootstrap_peers: Vec<Multiaddr>,
    pub data_dir_path: PathBuf,
    pub env_variables: Option<Vec<(String, String)>>,
    pub genesis: bool,
    pub home_network: bool,
    pub local: bool,
    pub log_dir_path: PathBuf,
    pub log_format: Option<LogFormat>,
    pub name: String,
    pub metrics_port: Option<u16>,
    pub node_ip: Option<Ipv4Addr>,
    pub node_port: Option<u16>,
    pub owner: Option<String>,
    pub rpc_socket_addr: SocketAddr,
    pub safenode_path: PathBuf,
    pub service_user: Option<String>,
    pub upnp: bool,
}

impl InstallNodeServiceCtxBuilder {
    pub fn build(self) -> Result<ServiceInstallCtx> {
        let label: ServiceLabel = self.name.parse()?;
        let mut args = vec![
            OsString::from("--rpc"),
            OsString::from(self.rpc_socket_addr.to_string()),
            OsString::from("--root-dir"),
            OsString::from(self.data_dir_path.to_string_lossy().to_string()),
            OsString::from("--log-output-dest"),
            OsString::from(self.log_dir_path.to_string_lossy().to_string()),
        ];

        if self.genesis {
            args.push(OsString::from("--first"));
        }
        if self.home_network {
            args.push(OsString::from("--home-network"));
        }
        if self.local {
            args.push(OsString::from("--local"));
        }
        if let Some(log_format) = self.log_format {
            args.push(OsString::from("--log-format"));
            args.push(OsString::from(log_format.as_str()));
        }
        if self.upnp {
            args.push(OsString::from("--upnp"));
        }
        if let Some(node_ip) = self.node_ip {
            args.push(OsString::from("--ip"));
            args.push(OsString::from(node_ip.to_string()));
        }
        if let Some(node_port) = self.node_port {
            args.push(OsString::from("--port"));
            args.push(OsString::from(node_port.to_string()));
        }
        if let Some(metrics_port) = self.metrics_port {
            args.push(OsString::from("--metrics-server-port"));
            args.push(OsString::from(metrics_port.to_string()));
        }
        if let Some(owner) = self.owner {
            args.push(OsString::from("--owner"));
            args.push(OsString::from(owner));
        }

        if !self.bootstrap_peers.is_empty() {
            let peers_str = self
                .bootstrap_peers
                .iter()
                .map(|peer| peer.to_string())
                .collect::<Vec<_>>()
                .join(",");
            args.push(OsString::from("--peer"));
            args.push(OsString::from(peers_str));
        }

        Ok(ServiceInstallCtx {
            args,
            autostart: self.autostart,
            contents: None,
            environment: self.env_variables,
            label: label.clone(),
            program: self.safenode_path.to_path_buf(),
            username: self.service_user.clone(),
            working_directory: None,
        })
    }
}

pub struct AddNodeServiceOptions {
    pub auto_restart: bool,
    pub auto_set_nat_flags: bool,
    pub bootstrap_peers: Vec<Multiaddr>,
    pub count: Option<u16>,
    pub delete_safenode_src: bool,
    pub enable_metrics_server: bool,
    pub env_variables: Option<Vec<(String, String)>>,
    pub genesis: bool,
    pub home_network: bool,
    pub local: bool,
    pub log_format: Option<LogFormat>,
    pub metrics_port: Option<PortRange>,
    pub owner: Option<String>,
    pub node_ip: Option<Ipv4Addr>,
    pub node_port: Option<PortRange>,
    pub rpc_address: Option<Ipv4Addr>,
    pub rpc_port: Option<PortRange>,
    pub safenode_src_path: PathBuf,
    pub safenode_dir_path: PathBuf,
    pub service_data_dir_path: PathBuf,
    pub service_log_dir_path: PathBuf,
    pub upnp: bool,
    pub user: Option<String>,
    pub user_mode: bool,
    pub version: String,
}

pub struct AddDaemonServiceOptions {
    pub address: Ipv4Addr,
    pub env_variables: Option<Vec<(String, String)>>,
    pub daemon_install_bin_path: PathBuf,
    pub daemon_src_bin_path: PathBuf,
    pub port: u16,
    pub user: String,
    pub version: String,
}
