// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_bootstrap::InitialPeersConfig;
use ant_evm::{EvmNetwork, RewardsAddress};
use ant_logging::LogFormat;
use ant_service_management::node::push_arguments_from_initial_peers_config;
use color_eyre::{eyre::eyre, Result};
use service_manager::{ServiceInstallCtx, ServiceLabel};
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
    pub antnode_path: PathBuf,
    pub autostart: bool,
    pub data_dir_path: PathBuf,
    pub env_variables: Option<Vec<(String, String)>>,
    pub evm_network: EvmNetwork,
    pub home_network: bool,
    pub log_dir_path: PathBuf,
    pub log_format: Option<LogFormat>,
    pub name: String,
    pub network_id: Option<u8>,
    pub max_archived_log_files: Option<usize>,
    pub max_log_files: Option<usize>,
    pub metrics_port: Option<u16>,
    pub node_ip: Option<Ipv4Addr>,
    pub node_port: Option<u16>,
    pub init_peers_config: InitialPeersConfig,
    pub rewards_address: RewardsAddress,
    pub rpc_socket_addr: SocketAddr,
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

        push_arguments_from_initial_peers_config(&self.init_peers_config, &mut args);
        if let Some(id) = self.network_id {
            args.push(OsString::from("--network-id"));
            args.push(OsString::from(id.to_string()));
        }
        if self.home_network {
            args.push(OsString::from("--home-network"));
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
        if let Some(log_files) = self.max_archived_log_files {
            args.push(OsString::from("--max-archived-log-files"));
            args.push(OsString::from(log_files.to_string()));
        }
        if let Some(log_files) = self.max_log_files {
            args.push(OsString::from("--max-log-files"));
            args.push(OsString::from(log_files.to_string()));
        }

        args.push(OsString::from("--rewards-address"));
        args.push(OsString::from(self.rewards_address.to_string()));

        args.push(OsString::from(self.evm_network.to_string()));
        if let EvmNetwork::Custom(custom_network) = &self.evm_network {
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
            autostart: self.autostart,
            contents: None,
            environment: self.env_variables,
            label: label.clone(),
            program: self.antnode_path.to_path_buf(),
            username: self.service_user.clone(),
            working_directory: None,
            disable_restart_on_failure: true,
        })
    }
}

pub struct AddNodeServiceOptions {
    pub antnode_dir_path: PathBuf,
    pub antnode_src_path: PathBuf,
    pub auto_restart: bool,
    pub auto_set_nat_flags: bool,
    pub count: Option<u16>,
    pub delete_antnode_src: bool,
    pub enable_metrics_server: bool,
    pub env_variables: Option<Vec<(String, String)>>,
    pub evm_network: EvmNetwork,
    pub init_peers_config: InitialPeersConfig,
    pub log_format: Option<LogFormat>,
    pub max_archived_log_files: Option<usize>,
    pub max_log_files: Option<usize>,
    pub metrics_port: Option<PortRange>,
    pub network_id: Option<u8>,
    pub node_ip: Option<Ipv4Addr>,
    pub node_port: Option<PortRange>,
    pub relay: bool,
    pub rewards_address: RewardsAddress,
    pub rpc_address: Option<Ipv4Addr>,
    pub rpc_port: Option<PortRange>,
    pub service_data_dir_path: PathBuf,
    pub service_log_dir_path: PathBuf,
    pub no_upnp: bool,
    pub user: Option<String>,
    pub user_mode: bool,
    pub version: String,
}

#[derive(Debug, PartialEq)]
pub struct InstallAuditorServiceCtxBuilder {
    pub auditor_path: PathBuf,
    pub beta_encryption_key: Option<String>,
    pub env_variables: Option<Vec<(String, String)>>,
    pub log_dir_path: PathBuf,
    pub name: String,
    pub service_user: String,
}

impl InstallAuditorServiceCtxBuilder {
    pub fn build(self) -> Result<ServiceInstallCtx> {
        let mut args = vec![
            OsString::from("--log-output-dest"),
            OsString::from(self.log_dir_path.to_string_lossy().to_string()),
        ];

        if let Some(beta_encryption_key) = self.beta_encryption_key {
            args.push(OsString::from("--beta-encryption-key"));
            args.push(OsString::from(beta_encryption_key));
        }

        Ok(ServiceInstallCtx {
            args,
            autostart: true,
            contents: None,
            environment: self.env_variables,
            label: self.name.parse()?,
            program: self.auditor_path.to_path_buf(),
            username: Some(self.service_user.to_string()),
            working_directory: None,
            disable_restart_on_failure: false,
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct InstallFaucetServiceCtxBuilder {
    pub env_variables: Option<Vec<(String, String)>>,
    pub faucet_path: PathBuf,
    pub local: bool,
    pub log_dir_path: PathBuf,
    pub name: String,
    pub service_user: String,
}

impl InstallFaucetServiceCtxBuilder {
    pub fn build(self) -> Result<ServiceInstallCtx> {
        let mut args = vec![
            OsString::from("--log-output-dest"),
            OsString::from(self.log_dir_path.to_string_lossy().to_string()),
        ];

        args.push(OsString::from("server"));

        Ok(ServiceInstallCtx {
            args,
            autostart: true,
            contents: None,
            environment: self.env_variables,
            label: self.name.parse()?,
            program: self.faucet_path.to_path_buf(),
            username: Some(self.service_user.to_string()),
            working_directory: None,
            disable_restart_on_failure: false,
        })
    }
}

pub struct AddAuditorServiceOptions {
    pub auditor_install_bin_path: PathBuf,
    pub auditor_src_bin_path: PathBuf,
    pub beta_encryption_key: Option<String>,
    pub env_variables: Option<Vec<(String, String)>>,
    pub service_log_dir_path: PathBuf,
    pub user: String,
    pub version: String,
}

pub struct AddFaucetServiceOptions {
    pub env_variables: Option<Vec<(String, String)>>,
    pub faucet_install_bin_path: PathBuf,
    pub faucet_src_bin_path: PathBuf,
    pub local: bool,
    pub service_data_dir_path: PathBuf,
    pub service_log_dir_path: PathBuf,
    pub user: String,
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

#[cfg(test)]
mod tests {
    use super::*;
    use ant_evm::{CustomNetwork, RewardsAddress};
    use std::net::{IpAddr, Ipv4Addr};

    fn create_default_builder() -> InstallNodeServiceCtxBuilder {
        InstallNodeServiceCtxBuilder {
            antnode_path: PathBuf::from("/bin/antnode"),
            autostart: true,
            data_dir_path: PathBuf::from("/data"),
            env_variables: None,
            evm_network: EvmNetwork::ArbitrumOne,
            home_network: false,
            log_dir_path: PathBuf::from("/logs"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            name: "test-node".to_string(),
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")
                .unwrap(),
            rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080),
            service_user: None,
            upnp: false,
        }
    }

    fn create_custom_evm_network_builder() -> InstallNodeServiceCtxBuilder {
        InstallNodeServiceCtxBuilder {
            autostart: true,
            data_dir_path: PathBuf::from("/data"),
            env_variables: None,
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse().unwrap(),
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )
                .unwrap(),
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )
                .unwrap(),
            }),
            home_network: false,
            log_dir_path: PathBuf::from("/logs"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            name: "test-node".to_string(),
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")
                .unwrap(),
            rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080),
            antnode_path: PathBuf::from("/bin/antnode"),
            service_user: None,
            upnp: false,
        }
    }

    fn create_builder_with_all_options_enabled() -> InstallNodeServiceCtxBuilder {
        InstallNodeServiceCtxBuilder {
            autostart: true,
            data_dir_path: PathBuf::from("/data"),
            env_variables: None,
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse().unwrap(),
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )
                .unwrap(),
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )
                .unwrap(),
            }),
            home_network: false,
            log_dir_path: PathBuf::from("/logs"),
            log_format: None,
            max_archived_log_files: Some(10),
            max_log_files: Some(10),
            metrics_port: None,
            name: "test-node".to_string(),
            network_id: Some(5),
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")
                .unwrap(),
            rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080),
            antnode_path: PathBuf::from("/bin/antnode"),
            service_user: None,
            upnp: false,
        }
    }

    #[test]
    fn build_should_assign_expected_values_when_mandatory_options_are_provided() {
        let builder = create_default_builder();
        let result = builder.build().unwrap();

        assert_eq!(result.label.to_string(), "test-node");
        assert_eq!(result.program, PathBuf::from("/bin/antnode"));
        assert!(result.autostart);
        assert_eq!(result.username, None);
        assert_eq!(result.working_directory, None);

        let expected_args = vec![
            "--rpc",
            "127.0.0.1:8080",
            "--root-dir",
            "/data",
            "--log-output-dest",
            "/logs",
            "--rewards-address",
            "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            "evm-arbitrum-one",
        ];
        assert_eq!(
            result
                .args
                .iter()
                .map(|os| os.to_str().unwrap())
                .collect::<Vec<_>>(),
            expected_args
        );
    }

    #[test]
    fn build_should_assign_expected_values_when_a_custom_evm_network_is_provided() {
        let builder = create_custom_evm_network_builder();
        let result = builder.build().unwrap();

        assert_eq!(result.label.to_string(), "test-node");
        assert_eq!(result.program, PathBuf::from("/bin/antnode"));
        assert!(result.autostart);
        assert_eq!(result.username, None);
        assert_eq!(result.working_directory, None);

        let expected_args = vec![
            "--rpc",
            "127.0.0.1:8080",
            "--root-dir",
            "/data",
            "--log-output-dest",
            "/logs",
            "--rewards-address",
            "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            "evm-custom",
            "--rpc-url",
            "http://localhost:8545/",
            "--payment-token-address",
            "0x5FbDB2315678afecb367f032d93F642f64180aa3",
            "--data-payments-address",
            "0x8464135c8F25Da09e49BC8782676a84730C318bC",
        ];
        assert_eq!(
            result
                .args
                .iter()
                .map(|os| os.to_str().unwrap())
                .collect::<Vec<_>>(),
            expected_args
        );
    }

    #[test]
    fn build_should_assign_expected_values_when_all_options_are_enabled() {
        let mut builder = create_builder_with_all_options_enabled();
        builder.home_network = true;
        builder.log_format = Some(LogFormat::Json);
        builder.upnp = true;
        builder.node_ip = Some(Ipv4Addr::new(192, 168, 1, 1));
        builder.node_port = Some(12345);
        builder.metrics_port = Some(9090);
        builder.init_peers_config.addrs = vec![
            "/ip4/127.0.0.1/tcp/8080".parse().unwrap(),
            "/ip4/192.168.1.1/tcp/8081".parse().unwrap(),
        ];
        builder.init_peers_config.first = true;
        builder.init_peers_config.local = true;
        builder.init_peers_config.network_contacts_url =
            vec!["http://localhost:8080".parse().unwrap()];
        builder.init_peers_config.ignore_cache = true;
        builder.init_peers_config.disable_mainnet_contacts = true;
        builder.service_user = Some("antnode-user".to_string());

        let result = builder.build().unwrap();

        let expected_args = vec![
            "--rpc",
            "127.0.0.1:8080",
            "--root-dir",
            "/data",
            "--log-output-dest",
            "/logs",
            "--first",
            "--local",
            "--peer",
            "/ip4/127.0.0.1/tcp/8080,/ip4/192.168.1.1/tcp/8081",
            "--network-contacts-url",
            "http://localhost:8080",
            "--testnet",
            "--ignore-cache",
            "--network-id",
            "5",
            "--home-network",
            "--log-format",
            "json",
            "--upnp",
            "--ip",
            "192.168.1.1",
            "--port",
            "12345",
            "--metrics-server-port",
            "9090",
            "--max-archived-log-files",
            "10",
            "--max-log-files",
            "10",
            "--rewards-address",
            "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            "evm-custom",
            "--rpc-url",
            "http://localhost:8545/",
            "--payment-token-address",
            "0x5FbDB2315678afecb367f032d93F642f64180aa3",
            "--data-payments-address",
            "0x8464135c8F25Da09e49BC8782676a84730C318bC",
        ];
        assert_eq!(
            result
                .args
                .iter()
                .map(|os| os.to_str().unwrap())
                .collect::<Vec<_>>(),
            expected_args
        );
        assert_eq!(result.username, Some("antnode-user".to_string()));
    }

    #[test]
    fn build_should_assign_expected_values_when_environment_variables_are_provided() {
        let mut builder = create_default_builder();
        builder.env_variables = Some(vec![
            ("VAR1".to_string(), "value1".to_string()),
            ("VAR2".to_string(), "value2".to_string()),
        ]);

        let result = builder.build().unwrap();

        assert_eq!(
            result.environment,
            Some(vec![
                ("VAR1".to_string(), "value1".to_string()),
                ("VAR2".to_string(), "value2".to_string()),
            ])
        );
    }
}
