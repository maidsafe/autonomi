// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    add_services::{
        add_auditor, add_daemon, add_faucet, add_node,
        config::{
            AddAuditorServiceOptions, AddDaemonServiceOptions, AddFaucetServiceOptions,
            AddNodeServiceOptions, InstallNodeServiceCtxBuilder, PortRange,
        },
    },
    VerbosityLevel,
};
use ant_bootstrap::InitialPeersConfig;
use ant_evm::{AttoTokens, CustomNetwork, EvmNetwork, RewardsAddress};
use ant_service_management::{auditor::AuditorServiceData, control::ServiceControl};
use ant_service_management::{error::Result as ServiceControlResult, NatDetectionStatus};
use ant_service_management::{
    DaemonServiceData, FaucetServiceData, NodeRegistry, NodeServiceData, ServiceStatus,
};
use assert_fs::prelude::*;
use assert_matches::assert_matches;
use color_eyre::Result;
use mockall::{mock, predicate::*, Sequence};
use predicates::prelude::*;
use service_manager::ServiceInstallCtx;
use std::{
    ffi::OsString,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    str::FromStr,
};

#[cfg(not(target_os = "windows"))]
const ANTNODE_FILE_NAME: &str = "antnode";
#[cfg(target_os = "windows")]
const ANTNODE_FILE_NAME: &str = "antnode.exe";
#[cfg(not(target_os = "windows"))]
const AUDITOR_FILE_NAME: &str = "sn_auditor";
#[cfg(target_os = "windows")]
const AUDITOR_FILE_NAME: &str = "sn_auditor.exe";
#[cfg(not(target_os = "windows"))]
const FAUCET_FILE_NAME: &str = "faucet";
#[cfg(target_os = "windows")]
const FAUCET_FILE_NAME: &str = "faucet.exe";
#[cfg(not(target_os = "windows"))]
const DAEMON_FILE_NAME: &str = "antctld";
#[cfg(target_os = "windows")]
const DAEMON_FILE_NAME: &str = "antctld.exe";

mock! {
    pub ServiceControl {}
    impl ServiceControl for ServiceControl {
        fn create_service_user(&self, username: &str) -> ServiceControlResult<()>;
        fn get_available_port(&self) -> ServiceControlResult<u16>;
        fn install(&self, install_ctx: ServiceInstallCtx, user_mode: bool) -> ServiceControlResult<()>;
        fn get_process_pid(&self, bin_path: &Path) -> ServiceControlResult<u32>;
        fn start(&self, service_name: &str, user_mode: bool) -> ServiceControlResult<()>;
        fn stop(&self, service_name: &str, user_mode: bool) -> ServiceControlResult<()>;
        fn uninstall(&self, service_name: &str, user_mode: bool) -> ServiceControlResult<()>;
        fn wait(&self, delay: u64);
    }
}

#[cfg(target_os = "windows")]
fn get_username() -> String {
    std::env::var("USERNAME").expect("Failed to get username")
}

#[cfg(not(target_os = "windows"))]
fn get_username() -> String {
    std::env::var("USER").expect("Failed to get username")
}

#[tokio::test]
async fn add_genesis_node_should_use_latest_version_and_add_one_service() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };

    let mut mock_service_control = MockServiceControl::new();
    let mut seq = Sequence::new();
    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(8081))
        .in_sequence(&mut seq);

    let init_peers_config = InitialPeersConfig {
        first: true,
        addrs: vec![],
        network_contacts_url: vec![],
        local: false,
        disable_mainnet_contacts: false,
        ignore_cache: false,
        bootstrap_cache_dir: None,
    };

    let install_ctx = InstallNodeServiceCtxBuilder {
        autostart: false,
        data_dir_path: node_data_dir.to_path_buf().join("antnode1"),
        env_variables: None,
        evm_network: EvmNetwork::Custom(CustomNetwork {
            rpc_url_http: "http://localhost:8545".parse()?,
            payment_token_address: RewardsAddress::from_str(
                "0x5FbDB2315678afecb367f032d93F642f64180aa3",
            )?,
            data_payments_address: RewardsAddress::from_str(
                "0x8464135c8F25Da09e49BC8782676a84730C318bC",
            )?,
        }),
        home_network: false,
        log_dir_path: node_logs_dir.to_path_buf().join("antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: Some(8081),
        name: "antnode1".to_string(),
        network_id: None,
        node_ip: None,
        node_port: None,
        init_peers_config: init_peers_config.clone(),
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        upnp: false,
    }
    .build()?;
    mock_service_control
        .expect_install()
        .times(1)
        .with(eq(install_ctx), eq(false))
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: None,
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config,
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());

    node_reg_path.assert(predicates::path::is_file());
    assert_eq!(node_registry.nodes.len(), 1);
    assert!(node_registry.nodes[0].peers_args.first);
    assert_eq!(node_registry.nodes[0].version, latest_version);
    assert_eq!(node_registry.nodes[0].service_name, "antnode1");
    assert_eq!(node_registry.nodes[0].user, Some(get_username()));
    assert_eq!(node_registry.nodes[0].number, 1);
    assert_eq!(
        node_registry.nodes[0].log_dir_path,
        node_logs_dir.to_path_buf().join("antnode1")
    );
    assert_eq!(
        node_registry.nodes[0].data_dir_path,
        node_data_dir.to_path_buf().join("antnode1")
    );
    assert_matches!(node_registry.nodes[0].status, ServiceStatus::Added);
    assert_eq!(
        node_registry.nodes[0].evm_network,
        EvmNetwork::Custom(CustomNetwork {
            rpc_url_http: "http://localhost:8545".parse()?,
            payment_token_address: RewardsAddress::from_str(
                "0x5FbDB2315678afecb367f032d93F642f64180aa3"
            )?,
            data_payments_address: RewardsAddress::from_str(
                "0x8464135c8F25Da09e49BC8782676a84730C318bC"
            )?,
        })
    );
    assert_eq!(
        node_registry.nodes[0].rewards_address,
        RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?
    );

    Ok(())
}

#[tokio::test]
async fn add_genesis_node_should_return_an_error_if_there_is_already_a_genesis_node() -> Result<()>
{
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mock_service_control = MockServiceControl::new();

    let latest_version = "0.96.4";

    let init_peers_config = InitialPeersConfig {
        first: true,
        addrs: vec![],
        network_contacts_url: vec![],
        local: false,
        disable_mainnet_contacts: false,
        ignore_cache: false,
        bootstrap_cache_dir: None,
    };
    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: None,
            peers_args: init_peers_config.clone(),
            pid: None,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            status: ServiceStatus::Added,
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            service_name: "antnode1".to_string(),
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: latest_version.to_string(),
        }],
        environment_variables: None,
        daemon: None,
    };

    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("antnode1");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let result = add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: None,
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config,
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await;

    assert_eq!(
        Err("A genesis node already exists".to_string()),
        result.map_err(|e| e.to_string())
    );

    Ok(())
}

#[tokio::test]
async fn add_genesis_node_should_return_an_error_if_count_is_greater_than_1() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };
    let init_peers_config = InitialPeersConfig {
        first: true,
        addrs: vec![],
        network_contacts_url: vec![],
        local: false,
        disable_mainnet_contacts: false,
        ignore_cache: false,
        bootstrap_cache_dir: None,
    };

    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("antnode1");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let result = add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: Some(3),
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config,
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await;

    assert_eq!(
        Err("A genesis node can only be added as a single node".to_string()),
        result.map_err(|e| e.to_string())
    );

    Ok(())
}

#[tokio::test]
async fn add_node_should_use_latest_version_and_add_three_services() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };

    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let mut seq = Sequence::new();

    // Expected calls for first installation
    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(8081))
        .in_sequence(&mut seq);

    let install_ctx = InstallNodeServiceCtxBuilder {
        autostart: false,
        data_dir_path: node_data_dir.to_path_buf().join("antnode1"),
        env_variables: None,
        evm_network: EvmNetwork::Custom(CustomNetwork {
            rpc_url_http: "http://localhost:8545".parse()?,
            payment_token_address: RewardsAddress::from_str(
                "0x5FbDB2315678afecb367f032d93F642f64180aa3",
            )?,
            data_payments_address: RewardsAddress::from_str(
                "0x8464135c8F25Da09e49BC8782676a84730C318bC",
            )?,
        }),
        home_network: false,
        log_dir_path: node_logs_dir.to_path_buf().join("antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: Some(8081),
        network_id: None,
        name: "antnode1".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        service_user: Some(get_username()),
        upnp: false,
    }
    .build()?;

    mock_service_control
        .expect_install()
        .times(1)
        .with(eq(install_ctx), eq(false))
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    // Expected calls for second installation
    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(8083))
        .in_sequence(&mut seq);
    let install_ctx = InstallNodeServiceCtxBuilder {
        autostart: false,
        data_dir_path: node_data_dir.to_path_buf().join("antnode2"),
        env_variables: None,
        evm_network: EvmNetwork::Custom(CustomNetwork {
            rpc_url_http: "http://localhost:8545".parse()?,
            payment_token_address: RewardsAddress::from_str(
                "0x5FbDB2315678afecb367f032d93F642f64180aa3",
            )?,
            data_payments_address: RewardsAddress::from_str(
                "0x8464135c8F25Da09e49BC8782676a84730C318bC",
            )?,
        }),
        home_network: false,
        log_dir_path: node_logs_dir.to_path_buf().join("antnode2"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: Some(8083),
        network_id: None,
        name: "antnode2".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode2")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        upnp: false,
    }
    .build()?;

    mock_service_control
        .expect_install()
        .times(1)
        .with(eq(install_ctx), eq(false))
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    // Expected calls for third installation
    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(8085))
        .in_sequence(&mut seq);
    let install_ctx = InstallNodeServiceCtxBuilder {
        autostart: false,
        data_dir_path: node_data_dir.to_path_buf().join("antnode3"),
        env_variables: None,
        evm_network: EvmNetwork::Custom(CustomNetwork {
            rpc_url_http: "http://localhost:8545".parse()?,
            payment_token_address: RewardsAddress::from_str(
                "0x5FbDB2315678afecb367f032d93F642f64180aa3",
            )?,
            data_payments_address: RewardsAddress::from_str(
                "0x8464135c8F25Da09e49BC8782676a84730C318bC",
            )?,
        }),
        home_network: false,
        log_format: None,
        log_dir_path: node_logs_dir.to_path_buf().join("antnode3"),
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: Some(8085),
        network_id: None,
        name: "antnode3".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode3")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        upnp: false,
    }
    .build()?;

    mock_service_control
        .expect_install()
        .times(1)
        .with(eq(install_ctx), eq(false))
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: Some(3),
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    assert_eq!(node_registry.nodes.len(), 3);
    assert_eq!(node_registry.nodes[0].version, latest_version);
    assert_eq!(node_registry.nodes[0].service_name, "antnode1");
    assert_eq!(node_registry.nodes[0].user, Some(get_username()));
    assert_eq!(node_registry.nodes[0].number, 1);
    assert_eq!(node_registry.nodes[0].rpc_socket_addr, None);
    assert_eq!(
        node_registry.nodes[0].log_dir_path,
        node_logs_dir.to_path_buf().join("antnode1")
    );
    assert_eq!(
        node_registry.nodes[0].data_dir_path,
        node_data_dir.to_path_buf().join("antnode1")
    );
    assert_matches!(node_registry.nodes[0].status, ServiceStatus::Added);
    assert_eq!(node_registry.nodes[1].version, latest_version);
    assert_eq!(node_registry.nodes[1].service_name, "antnode2");
    assert_eq!(node_registry.nodes[1].user, Some(get_username()));
    assert_eq!(node_registry.nodes[1].number, 2);
    assert_eq!(
        node_registry.nodes[1].log_dir_path,
        node_logs_dir.to_path_buf().join("antnode2")
    );
    assert_eq!(
        node_registry.nodes[1].data_dir_path,
        node_data_dir.to_path_buf().join("antnode2")
    );
    assert_matches!(node_registry.nodes[1].status, ServiceStatus::Added);
    assert_eq!(node_registry.nodes[2].version, latest_version);
    assert_eq!(node_registry.nodes[2].service_name, "antnode3");
    assert_eq!(node_registry.nodes[2].user, Some(get_username()));
    assert_eq!(node_registry.nodes[2].number, 3);
    assert_eq!(
        node_registry.nodes[2].log_dir_path,
        node_logs_dir.to_path_buf().join("antnode3")
    );
    assert_eq!(
        node_registry.nodes[2].data_dir_path,
        node_data_dir.to_path_buf().join("antnode3")
    );
    assert_matches!(node_registry.nodes[2].status, ServiceStatus::Added);

    Ok(())
}

#[tokio::test]
async fn add_node_should_update_the_environment_variables_inside_node_registry() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let env_variables = Some(vec![
        ("ANT_LOG".to_owned(), "all".to_owned()),
        ("RUST_LOG".to_owned(), "libp2p=debug".to_owned()),
    ]);

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let mut seq = Sequence::new();

    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(12001))
        .in_sequence(&mut seq);
    let install_ctx = InstallNodeServiceCtxBuilder {
        autostart: false,
        data_dir_path: node_data_dir.to_path_buf().join("antnode1"),
        env_variables: env_variables.clone(),
        evm_network: EvmNetwork::Custom(CustomNetwork {
            rpc_url_http: "http://localhost:8545".parse()?,
            payment_token_address: RewardsAddress::from_str(
                "0x5FbDB2315678afecb367f032d93F642f64180aa3",
            )?,
            data_payments_address: RewardsAddress::from_str(
                "0x8464135c8F25Da09e49BC8782676a84730C318bC",
            )?,
        }),
        home_network: false,
        log_dir_path: node_logs_dir.to_path_buf().join("antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: Some(12001),
        network_id: None,
        name: "antnode1".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        upnp: false,
    }
    .build()?;
    mock_service_control
        .expect_install()
        .times(1)
        .with(eq(install_ctx), eq(false))
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: None,
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: env_variables.clone(),
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());

    assert_eq!(node_registry.environment_variables, env_variables);

    assert_eq!(node_registry.nodes.len(), 1);
    assert_eq!(node_registry.nodes[0].version, latest_version);
    assert_eq!(node_registry.nodes[0].service_name, "antnode1");
    assert_eq!(node_registry.nodes[0].user, Some(get_username()));
    assert_eq!(node_registry.nodes[0].number, 1);
    assert_eq!(
        node_registry.nodes[0].log_dir_path,
        node_logs_dir.to_path_buf().join("antnode1")
    );
    assert_eq!(
        node_registry.nodes[0].data_dir_path,
        node_data_dir.to_path_buf().join("antnode1")
    );
    assert_matches!(node_registry.nodes[0].status, ServiceStatus::Added);

    Ok(())
}

#[tokio::test]
async fn add_new_node_should_add_another_service() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let latest_version = "0.96.4";
    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: None,
            peers_args: InitialPeersConfig::default(),
            pid: None,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Added,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: latest_version.to_string(),
        }],
        environment_variables: None,
        daemon: None,
    };
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("antnode1");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let mut seq = Sequence::new();
    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(8083))
        .in_sequence(&mut seq);
    let install_ctx = InstallNodeServiceCtxBuilder {
        autostart: false,
        data_dir_path: node_data_dir.to_path_buf().join("antnode2"),
        env_variables: None,
        evm_network: EvmNetwork::Custom(CustomNetwork {
            rpc_url_http: "http://localhost:8545".parse()?,
            payment_token_address: RewardsAddress::from_str(
                "0x5FbDB2315678afecb367f032d93F642f64180aa3",
            )?,
            data_payments_address: RewardsAddress::from_str(
                "0x8464135c8F25Da09e49BC8782676a84730C318bC",
            )?,
        }),
        home_network: false,
        log_dir_path: node_logs_dir.to_path_buf().join("antnode2"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: Some(8083),
        network_id: None,
        name: "antnode2".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode2")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        upnp: false,
    }
    .build()?;

    mock_service_control
        .expect_install()
        .times(1)
        .with(eq(install_ctx), eq(false))
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: None,
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            antnode_dir_path: temp_dir.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    assert_eq!(node_registry.nodes.len(), 2);
    assert_eq!(node_registry.nodes[1].version, latest_version);
    assert_eq!(node_registry.nodes[1].service_name, "antnode2");
    assert_eq!(node_registry.nodes[1].user, Some(get_username()));
    assert_eq!(
        node_registry.nodes[1].log_dir_path,
        node_logs_dir.to_path_buf().join("antnode2")
    );
    assert_eq!(
        node_registry.nodes[1].data_dir_path,
        node_data_dir.to_path_buf().join("antnode2")
    );
    assert_matches!(node_registry.nodes[0].status, ServiceStatus::Added);
    assert!(!node_registry.nodes[0].auto_restart);

    Ok(())
}

#[tokio::test]
async fn add_node_should_create_service_file_with_first_arg() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let peers_args = InitialPeersConfig {
        first: true,
        addrs: vec![],
        network_contacts_url: vec![],
        local: false,
        disable_mainnet_contacts: false,
        ignore_cache: false,
        bootstrap_cache_dir: None,
    };

    let mut seq = Sequence::new();

    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(12001))
        .in_sequence(&mut seq);

    mock_service_control
        .expect_install()
        .times(1)
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--root-dir"),
                    OsString::from(
                        node_data_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--log-output-dest"),
                    OsString::from(
                        node_logs_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--first"),
                    OsString::from("--metrics-server-port"),
                    OsString::from("12001"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-custom"),
                    OsString::from("--rpc-url"),
                    OsString::from("http://localhost:8545/"),
                    OsString::from("--payment-token-address"),
                    OsString::from("0x5FbDB2315678afecb367f032d93F642f64180aa3"),
                    OsString::from("--data-payments-address"),
                    OsString::from("0x8464135c8F25Da09e49BC8782676a84730C318bC"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: node_data_dir
                    .to_path_buf()
                    .join("antnode1")
                    .join(ANTNODE_FILE_NAME),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: None,
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: peers_args.clone(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());
    assert_eq!(node_registry.nodes.len(), 1);
    assert_eq!(node_registry.nodes[0].version, latest_version);
    assert_eq!(node_registry.nodes[0].peers_args, peers_args);
    assert!(node_registry.nodes[0].peers_args.first);

    Ok(())
}

#[tokio::test]
async fn add_node_should_create_service_file_with_peers_args() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let peers_args = InitialPeersConfig {
        first: false,
        addrs: vec![
            "/ip4/127.0.0.1/tcp/8080/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
                .parse()?,
        ],
        network_contacts_url: vec![],
        local: false,
        disable_mainnet_contacts: false,
        ignore_cache: false,
        bootstrap_cache_dir: None,
    };

    let mut seq = Sequence::new();

    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(12001))
        .in_sequence(&mut seq);

    mock_service_control
        .expect_install()
        .times(1)
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--root-dir"),
                    OsString::from(
                        node_data_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--log-output-dest"),
                    OsString::from(
                        node_logs_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--peer"),
                    OsString::from(
                        "/ip4/127.0.0.1/tcp/8080/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"),
                    OsString::from("--metrics-server-port"),
                    OsString::from("12001"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-custom"),
                    OsString::from("--rpc-url"),
                    OsString::from("http://localhost:8545/"),
                    OsString::from("--payment-token-address"),
                    OsString::from("0x5FbDB2315678afecb367f032d93F642f64180aa3"),
                    OsString::from("--data-payments-address"),
                    OsString::from("0x8464135c8F25Da09e49BC8782676a84730C318bC"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: node_data_dir
                    .to_path_buf()
                    .join("antnode1")
                    .join(ANTNODE_FILE_NAME),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: None,
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: peers_args.clone(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());
    assert_eq!(node_registry.nodes.len(), 1);
    assert_eq!(node_registry.nodes[0].version, latest_version);
    assert_eq!(node_registry.nodes[0].peers_args, peers_args);
    assert_eq!(node_registry.nodes[0].peers_args.addrs.len(), 1);

    Ok(())
}

#[tokio::test]
async fn add_node_should_create_service_file_with_local_arg() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let peers_args = InitialPeersConfig {
        first: false,
        addrs: vec![],
        network_contacts_url: vec![],
        local: true,
        disable_mainnet_contacts: false,
        ignore_cache: false,
        bootstrap_cache_dir: None,
    };

    let mut seq = Sequence::new();

    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(12001))
        .in_sequence(&mut seq);

    mock_service_control
        .expect_install()
        .times(1)
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--root-dir"),
                    OsString::from(
                        node_data_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--log-output-dest"),
                    OsString::from(
                        node_logs_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--local"),
                    OsString::from("--metrics-server-port"),
                    OsString::from("12001"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-custom"),
                    OsString::from("--rpc-url"),
                    OsString::from("http://localhost:8545/"),
                    OsString::from("--payment-token-address"),
                    OsString::from("0x5FbDB2315678afecb367f032d93F642f64180aa3"),
                    OsString::from("--data-payments-address"),
                    OsString::from("0x8464135c8F25Da09e49BC8782676a84730C318bC"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: node_data_dir
                    .to_path_buf()
                    .join("antnode1")
                    .join(ANTNODE_FILE_NAME),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: None,
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: peers_args.clone(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());
    assert_eq!(node_registry.nodes.len(), 1);
    assert_eq!(node_registry.nodes[0].version, latest_version);
    assert_eq!(node_registry.nodes[0].peers_args, peers_args);
    assert!(node_registry.nodes[0].peers_args.local);

    Ok(())
}

#[tokio::test]
async fn add_node_should_create_service_file_with_network_contacts_url_arg() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let peers_args = InitialPeersConfig {
        first: false,
        addrs: vec![],
        network_contacts_url: vec![
            "http://localhost:8080/contacts".to_string(),
            "http://localhost:8081/contacts".to_string(),
        ],
        local: false,
        disable_mainnet_contacts: false,
        ignore_cache: false,
        bootstrap_cache_dir: None,
    };

    let mut seq = Sequence::new();

    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(12001))
        .in_sequence(&mut seq);

    mock_service_control
        .expect_install()
        .times(1)
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--root-dir"),
                    OsString::from(
                        node_data_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--log-output-dest"),
                    OsString::from(
                        node_logs_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--network-contacts-url"),
                    OsString::from("http://localhost:8080/contacts,http://localhost:8081/contacts"),
                    OsString::from("--metrics-server-port"),
                    OsString::from("12001"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-custom"),
                    OsString::from("--rpc-url"),
                    OsString::from("http://localhost:8545/"),
                    OsString::from("--payment-token-address"),
                    OsString::from("0x5FbDB2315678afecb367f032d93F642f64180aa3"),
                    OsString::from("--data-payments-address"),
                    OsString::from("0x8464135c8F25Da09e49BC8782676a84730C318bC"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: node_data_dir
                    .to_path_buf()
                    .join("antnode1")
                    .join(ANTNODE_FILE_NAME),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: None,
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: peers_args.clone(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());
    assert_eq!(node_registry.nodes.len(), 1);
    assert_eq!(node_registry.nodes[0].version, latest_version);
    assert_eq!(node_registry.nodes[0].peers_args, peers_args);
    assert_eq!(
        node_registry.nodes[0].peers_args.network_contacts_url.len(),
        2
    );

    Ok(())
}

#[tokio::test]
async fn add_node_should_create_service_file_with_testnet_arg() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let peers_args = InitialPeersConfig {
        first: false,
        addrs: vec![],
        network_contacts_url: vec![],
        local: false,
        disable_mainnet_contacts: true,
        ignore_cache: false,
        bootstrap_cache_dir: None,
    };

    let mut seq = Sequence::new();

    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(12001))
        .in_sequence(&mut seq);

    mock_service_control
        .expect_install()
        .times(1)
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--root-dir"),
                    OsString::from(
                        node_data_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--log-output-dest"),
                    OsString::from(
                        node_logs_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--testnet"),
                    OsString::from("--metrics-server-port"),
                    OsString::from("12001"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-custom"),
                    OsString::from("--rpc-url"),
                    OsString::from("http://localhost:8545/"),
                    OsString::from("--payment-token-address"),
                    OsString::from("0x5FbDB2315678afecb367f032d93F642f64180aa3"),
                    OsString::from("--data-payments-address"),
                    OsString::from("0x8464135c8F25Da09e49BC8782676a84730C318bC"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: node_data_dir
                    .to_path_buf()
                    .join("antnode1")
                    .join(ANTNODE_FILE_NAME),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: None,
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: peers_args.clone(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());
    assert_eq!(node_registry.nodes.len(), 1);
    assert_eq!(node_registry.nodes[0].version, latest_version);
    assert_eq!(node_registry.nodes[0].peers_args, peers_args);
    assert!(node_registry.nodes[0].peers_args.disable_mainnet_contacts);

    Ok(())
}

#[tokio::test]
async fn add_node_should_create_service_file_with_ignore_cache_arg() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let peers_args = InitialPeersConfig {
        first: false,
        addrs: vec![],
        network_contacts_url: vec![],
        local: false,
        disable_mainnet_contacts: false,
        ignore_cache: true,
        bootstrap_cache_dir: None,
    };

    let mut seq = Sequence::new();
    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(12001))
        .in_sequence(&mut seq);

    mock_service_control
        .expect_install()
        .times(1)
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--root-dir"),
                    OsString::from(
                        node_data_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--log-output-dest"),
                    OsString::from(
                        node_logs_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--ignore-cache"),
                    OsString::from("--metrics-server-port"),
                    OsString::from("12001"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-custom"),
                    OsString::from("--rpc-url"),
                    OsString::from("http://localhost:8545/"),
                    OsString::from("--payment-token-address"),
                    OsString::from("0x5FbDB2315678afecb367f032d93F642f64180aa3"),
                    OsString::from("--data-payments-address"),
                    OsString::from("0x8464135c8F25Da09e49BC8782676a84730C318bC"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: node_data_dir
                    .to_path_buf()
                    .join("antnode1")
                    .join(ANTNODE_FILE_NAME),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: None,
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: peers_args.clone(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());
    assert_eq!(node_registry.nodes.len(), 1);
    assert_eq!(node_registry.nodes[0].version, latest_version);
    assert_eq!(node_registry.nodes[0].peers_args, peers_args);
    assert!(node_registry.nodes[0].peers_args.ignore_cache);

    Ok(())
}

#[tokio::test]
async fn add_node_should_create_service_file_with_custom_bootstrap_cache_path() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let peers_args = InitialPeersConfig {
        first: false,
        addrs: vec![],
        network_contacts_url: vec![],
        local: false,
        disable_mainnet_contacts: false,
        ignore_cache: false,
        bootstrap_cache_dir: Some(PathBuf::from("/path/to/bootstrap/cache")),
    };

    let mut seq = Sequence::new();

    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(12001))
        .in_sequence(&mut seq);

    mock_service_control
        .expect_install()
        .times(1)
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--root-dir"),
                    OsString::from(
                        node_data_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--log-output-dest"),
                    OsString::from(
                        node_logs_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--bootstrap-cache-dir"),
                    OsString::from("/path/to/bootstrap/cache"),
                    OsString::from("--metrics-server-port"),
                    OsString::from("12001"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-custom"),
                    OsString::from("--rpc-url"),
                    OsString::from("http://localhost:8545/"),
                    OsString::from("--payment-token-address"),
                    OsString::from("0x5FbDB2315678afecb367f032d93F642f64180aa3"),
                    OsString::from("--data-payments-address"),
                    OsString::from("0x8464135c8F25Da09e49BC8782676a84730C318bC"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: node_data_dir
                    .to_path_buf()
                    .join("antnode1")
                    .join(ANTNODE_FILE_NAME),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: None,
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: peers_args.clone(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());
    assert_eq!(node_registry.nodes.len(), 1);
    assert_eq!(node_registry.nodes[0].version, latest_version);
    assert_eq!(node_registry.nodes[0].peers_args, peers_args);
    assert_eq!(
        node_registry.nodes[0].peers_args.bootstrap_cache_dir,
        Some(PathBuf::from("/path/to/bootstrap/cache"))
    );

    Ok(())
}

#[tokio::test]
async fn add_node_should_create_service_file_with_network_id() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let mut seq = Sequence::new();

    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(12001))
        .in_sequence(&mut seq);

    mock_service_control
        .expect_install()
        .times(1)
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--root-dir"),
                    OsString::from(
                        node_data_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--log-output-dest"),
                    OsString::from(
                        node_logs_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--network-id"),
                    OsString::from("5"),
                    OsString::from("--metrics-server-port"),
                    OsString::from("12001"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-custom"),
                    OsString::from("--rpc-url"),
                    OsString::from("http://localhost:8545/"),
                    OsString::from("--payment-token-address"),
                    OsString::from("0x5FbDB2315678afecb367f032d93F642f64180aa3"),
                    OsString::from("--data-payments-address"),
                    OsString::from("0x8464135c8F25Da09e49BC8782676a84730C318bC"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: node_data_dir
                    .to_path_buf()
                    .join("antnode1")
                    .join(ANTNODE_FILE_NAME),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: None,
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: Some(5),
            node_ip: None,
            node_port: None,
            init_peers_config: Default::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());
    assert_eq!(node_registry.nodes.len(), 1);
    assert_eq!(node_registry.nodes[0].version, latest_version);
    assert_eq!(node_registry.nodes[0].network_id, Some(5));

    Ok(())
}

#[tokio::test]
async fn add_node_should_use_custom_ip() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let custom_ip = Ipv4Addr::new(192, 168, 1, 1);

    let mut seq = Sequence::new();

    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(12001))
        .in_sequence(&mut seq);

    mock_service_control
        .expect_install()
        .times(1)
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--root-dir"),
                    OsString::from(
                        node_data_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--log-output-dest"),
                    OsString::from(
                        node_logs_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--ip"),
                    OsString::from(custom_ip.to_string()),
                    OsString::from("--metrics-server-port"),
                    OsString::from("12001"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-custom"),
                    OsString::from("--rpc-url"),
                    OsString::from("http://localhost:8545/"),
                    OsString::from("--payment-token-address"),
                    OsString::from("0x5FbDB2315678afecb367f032d93F642f64180aa3"),
                    OsString::from("--data-payments-address"),
                    OsString::from("0x8464135c8F25Da09e49BC8782676a84730C318bC"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: node_data_dir
                    .to_path_buf()
                    .join("antnode1")
                    .join(ANTNODE_FILE_NAME),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: None,
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: Some(custom_ip),
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());

    assert_eq!(node_registry.nodes.len(), 1);
    assert_eq!(node_registry.nodes[0].node_ip, Some(custom_ip));

    Ok(())
}

#[tokio::test]
async fn add_node_should_use_custom_ports_for_one_service() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let custom_port = 12000;

    let mut seq = Sequence::new();

    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(12001))
        .in_sequence(&mut seq);
    let install_ctx = InstallNodeServiceCtxBuilder {
        autostart: false,
        data_dir_path: node_data_dir.to_path_buf().join("antnode1"),
        env_variables: None,
        evm_network: EvmNetwork::Custom(CustomNetwork {
            rpc_url_http: "http://localhost:8545".parse()?,
            payment_token_address: RewardsAddress::from_str(
                "0x5FbDB2315678afecb367f032d93F642f64180aa3",
            )?,
            data_payments_address: RewardsAddress::from_str(
                "0x8464135c8F25Da09e49BC8782676a84730C318bC",
            )?,
        }),
        home_network: false,
        log_dir_path: node_logs_dir.to_path_buf().join("antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: Some(12001),
        network_id: None,
        name: "antnode1".to_string(),
        node_ip: None,
        node_port: Some(custom_port),
        init_peers_config: InitialPeersConfig::default(),
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        upnp: false,
    }
    .build()?;

    mock_service_control
        .expect_install()
        .times(1)
        .with(eq(install_ctx), eq(false))
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: None,
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: Some(PortRange::Single(custom_port)),
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());

    assert_eq!(node_registry.nodes.len(), 1);
    assert_eq!(node_registry.nodes[0].node_port, Some(custom_port));

    Ok(())
}

#[tokio::test]
async fn add_node_should_use_a_custom_port_range() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let mut seq = Sequence::new();

    // First service
    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(15000))
        .in_sequence(&mut seq);
    mock_service_control
        .expect_install()
        .times(1)
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--root-dir"),
                    OsString::from(
                        node_data_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--log-output-dest"),
                    OsString::from(
                        node_logs_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--port"),
                    OsString::from("12000"),
                    OsString::from("--metrics-server-port"),
                    OsString::from("15000"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-custom"),
                    OsString::from("--rpc-url"),
                    OsString::from("http://localhost:8545/"),
                    OsString::from("--payment-token-address"),
                    OsString::from("0x5FbDB2315678afecb367f032d93F642f64180aa3"),
                    OsString::from("--data-payments-address"),
                    OsString::from("0x8464135c8F25Da09e49BC8782676a84730C318bC"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: node_data_dir
                    .to_path_buf()
                    .join("antnode1")
                    .join(ANTNODE_FILE_NAME),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    // Second service
    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(15001))
        .in_sequence(&mut seq);
    mock_service_control
        .expect_install()
        .times(1)
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--root-dir"),
                    OsString::from(
                        node_data_dir
                            .to_path_buf()
                            .join("antnode2")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--log-output-dest"),
                    OsString::from(
                        node_logs_dir
                            .to_path_buf()
                            .join("antnode2")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--port"),
                    OsString::from("12001"),
                    OsString::from("--metrics-server-port"),
                    OsString::from("15001"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-custom"),
                    OsString::from("--rpc-url"),
                    OsString::from("http://localhost:8545/"),
                    OsString::from("--payment-token-address"),
                    OsString::from("0x5FbDB2315678afecb367f032d93F642f64180aa3"),
                    OsString::from("--data-payments-address"),
                    OsString::from("0x8464135c8F25Da09e49BC8782676a84730C318bC"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode2".parse()?,
                program: node_data_dir
                    .to_path_buf()
                    .join("antnode2")
                    .join(ANTNODE_FILE_NAME),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    // Third service
    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(15002))
        .in_sequence(&mut seq);
    mock_service_control
        .expect_install()
        .times(1)
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--root-dir"),
                    OsString::from(
                        node_data_dir
                            .to_path_buf()
                            .join("antnode3")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--log-output-dest"),
                    OsString::from(
                        node_logs_dir
                            .to_path_buf()
                            .join("antnode3")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--port"),
                    OsString::from("12002"),
                    OsString::from("--metrics-server-port"),
                    OsString::from("15002"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-custom"),
                    OsString::from("--rpc-url"),
                    OsString::from("http://localhost:8545/"),
                    OsString::from("--payment-token-address"),
                    OsString::from("0x5FbDB2315678afecb367f032d93F642f64180aa3"),
                    OsString::from("--data-payments-address"),
                    OsString::from("0x8464135c8F25Da09e49BC8782676a84730C318bC"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode3".parse()?,
                program: node_data_dir
                    .to_path_buf()
                    .join("antnode3")
                    .join(ANTNODE_FILE_NAME),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: Some(3),
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: Some(PortRange::Range(12000, 12002)),
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());
    assert_eq!(node_registry.nodes.len(), 3);
    assert_eq!(node_registry.nodes[0].node_port, Some(12000));
    assert_eq!(node_registry.nodes[1].node_port, Some(12001));
    assert_eq!(node_registry.nodes[2].node_port, Some(12002));

    Ok(())
}

#[tokio::test]
async fn add_node_should_return_an_error_if_duplicate_custom_port_is_used() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_format: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: Some(12000),
            number: 1,
            peer_id: None,
            peers_args: InitialPeersConfig::default(),
            pid: None,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Added,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
        }],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let result = add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: None,
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: Some(PortRange::Single(12000)),
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &MockServiceControl::new(),
        VerbosityLevel::Normal,
    )
    .await;

    match result {
        Ok(_) => panic!("This test is supposed to result in a failure"),
        Err(e) => {
            assert_eq!(e.to_string(), "Port 12000 is being used by another service");
            Ok(())
        }
    }
}

#[tokio::test]
async fn add_node_should_return_an_error_if_duplicate_custom_port_in_range_is_used() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_format: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: Some(12000),
            peers_args: InitialPeersConfig::default(),
            number: 1,
            peer_id: None,
            pid: None,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Added,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
        }],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let result = add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: Some(3),
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: Some(PortRange::Range(12000, 12002)),
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &MockServiceControl::new(),
        VerbosityLevel::Normal,
    )
    .await;

    match result {
        Ok(_) => panic!("This test is supposed to result in a failure"),
        Err(e) => {
            assert_eq!(e.to_string(), "Port 12000 is being used by another service");
            Ok(())
        }
    }
}

#[tokio::test]
async fn add_node_should_return_an_error_if_port_and_node_count_do_not_match() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let result = add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: Some(2),
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: Some(PortRange::Range(12000, 12002)),
            init_peers_config: InitialPeersConfig::default(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            antnode_dir_path: temp_dir.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &MockServiceControl::new(),
        VerbosityLevel::Normal,
    )
    .await;

    match result {
        Ok(_) => panic!("This test should result in an error"),
        Err(e) => {
            assert_eq!(
                format!("The count (2) does not match the number of ports (3)"),
                e.to_string()
            )
        }
    }

    Ok(())
}

#[tokio::test]
async fn add_node_should_return_an_error_if_multiple_services_are_specified_with_a_single_port(
) -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let result = add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: Some(2),
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: Some(PortRange::Single(12000)),
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &MockServiceControl::new(),
        VerbosityLevel::Normal,
    )
    .await;

    match result {
        Ok(_) => panic!("This test should result in an error"),
        Err(e) => {
            assert_eq!(
                format!("The count (2) does not match the number of ports (1)"),
                e.to_string()
            )
        }
    }

    Ok(())
}

#[tokio::test]
async fn add_node_should_set_random_ports_if_enable_metrics_server_is_true() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let mut seq = Sequence::new();

    // First service
    let mut ports = vec![Ok(8081), Ok(15001)].into_iter();
    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(move || ports.next().unwrap())
        .in_sequence(&mut seq);
    mock_service_control
        .expect_install()
        .times(1)
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--root-dir"),
                    OsString::from(
                        node_data_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--log-output-dest"),
                    OsString::from(
                        node_logs_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--metrics-server-port"),
                    OsString::from("8081"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-custom"),
                    OsString::from("--rpc-url"),
                    OsString::from("http://localhost:8545/"),
                    OsString::from("--payment-token-address"),
                    OsString::from("0x5FbDB2315678afecb367f032d93F642f64180aa3"),
                    OsString::from("--data-payments-address"),
                    OsString::from("0x8464135c8F25Da09e49BC8782676a84730C318bC"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: node_data_dir
                    .to_path_buf()
                    .join("antnode1")
                    .join(ANTNODE_FILE_NAME),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: None,
            delete_antnode_src: true,
            enable_metrics_server: true,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    assert_eq!(node_registry.nodes[0].metrics_port, Some(8081));
    Ok(())
}

#[tokio::test]
async fn add_node_should_set_max_archived_log_files() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };

    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let mut seq = Sequence::new();

    // Expected calls for first installation
    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(8081))
        .in_sequence(&mut seq);

    mock_service_control
        .expect_install()
        .times(1)
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--root-dir"),
                    OsString::from(
                        node_data_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--log-output-dest"),
                    OsString::from(
                        node_logs_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--metrics-server-port"),
                    OsString::from("8081"),
                    OsString::from("--max-archived-log-files"),
                    OsString::from("20"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-custom"),
                    OsString::from("--rpc-url"),
                    OsString::from("http://localhost:8545/"),
                    OsString::from("--payment-token-address"),
                    OsString::from("0x5FbDB2315678afecb367f032d93F642f64180aa3"),
                    OsString::from("--data-payments-address"),
                    OsString::from("0x8464135c8F25Da09e49BC8782676a84730C318bC"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: node_data_dir
                    .to_path_buf()
                    .join("antnode1")
                    .join(ANTNODE_FILE_NAME),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: Some(1),
            delete_antnode_src: false,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: Some(20),
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    assert_matches!(node_registry.nodes[0].max_archived_log_files, Some(20));

    Ok(())
}

#[tokio::test]
async fn add_node_should_set_max_log_files() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };

    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let mut seq = Sequence::new();

    // Expected calls for first installation
    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(8081))
        .in_sequence(&mut seq);

    mock_service_control
        .expect_install()
        .times(1)
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--root-dir"),
                    OsString::from(
                        node_data_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--log-output-dest"),
                    OsString::from(
                        node_logs_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--metrics-server-port"),
                    OsString::from("8081"),
                    OsString::from("--max-log-files"),
                    OsString::from("20"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-custom"),
                    OsString::from("--rpc-url"),
                    OsString::from("http://localhost:8545/"),
                    OsString::from("--payment-token-address"),
                    OsString::from("0x5FbDB2315678afecb367f032d93F642f64180aa3"),
                    OsString::from("--data-payments-address"),
                    OsString::from("0x8464135c8F25Da09e49BC8782676a84730C318bC"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: node_data_dir
                    .to_path_buf()
                    .join("antnode1")
                    .join(ANTNODE_FILE_NAME),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: Some(1),
            delete_antnode_src: false,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: Some(20),
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    assert_matches!(node_registry.nodes[0].max_log_files, Some(20));

    Ok(())
}

#[tokio::test]
async fn add_node_should_use_a_custom_port_range_for_metrics_server() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let mut seq = Sequence::new();
    let control_ctx = ServiceInstallCtx {
        args: vec![
            OsString::from("--root-dir"),
            OsString::from(
                node_data_dir
                    .to_path_buf()
                    .join("antnode1")
                    .to_string_lossy()
                    .to_string(),
            ),
            OsString::from("--log-output-dest"),
            OsString::from(
                node_logs_dir
                    .to_path_buf()
                    .join("antnode1")
                    .to_string_lossy()
                    .to_string(),
            ),
            OsString::from("--metrics-server-port"),
            OsString::from("15000"),
            OsString::from("--rewards-address"),
            OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
            OsString::from("evm-custom"),
            OsString::from("--rpc-url"),
            OsString::from("http://localhost:8545/"),
            OsString::from("--payment-token-address"),
            OsString::from("0x5FbDB2315678afecb367f032d93F642f64180aa3"),
            OsString::from("--data-payments-address"),
            OsString::from("0x8464135c8F25Da09e49BC8782676a84730C318bC"),
        ],
        autostart: false,
        contents: None,
        environment: None,
        label: "antnode1".parse()?,
        program: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        username: Some(get_username()),
        working_directory: None,
        disable_restart_on_failure: true,
    };
    println!("control_ctx: {:?}", control_ctx);
    // First service
    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(15000))
        .in_sequence(&mut seq);
    mock_service_control
        .expect_install()
        .times(1)
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--root-dir"),
                    OsString::from(
                        node_data_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--log-output-dest"),
                    OsString::from(
                        node_logs_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--metrics-server-port"),
                    OsString::from("15000"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-custom"),
                    OsString::from("--rpc-url"),
                    OsString::from("http://localhost:8545/"),
                    OsString::from("--payment-token-address"),
                    OsString::from("0x5FbDB2315678afecb367f032d93F642f64180aa3"),
                    OsString::from("--data-payments-address"),
                    OsString::from("0x8464135c8F25Da09e49BC8782676a84730C318bC"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: node_data_dir
                    .to_path_buf()
                    .join("antnode1")
                    .join(ANTNODE_FILE_NAME),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    // Second service
    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(15001))
        .in_sequence(&mut seq);
    mock_service_control
        .expect_install()
        .times(1)
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--root-dir"),
                    OsString::from(
                        node_data_dir
                            .to_path_buf()
                            .join("antnode2")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--log-output-dest"),
                    OsString::from(
                        node_logs_dir
                            .to_path_buf()
                            .join("antnode2")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--metrics-server-port"),
                    OsString::from("15001"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-custom"),
                    OsString::from("--rpc-url"),
                    OsString::from("http://localhost:8545/"),
                    OsString::from("--payment-token-address"),
                    OsString::from("0x5FbDB2315678afecb367f032d93F642f64180aa3"),
                    OsString::from("--data-payments-address"),
                    OsString::from("0x8464135c8F25Da09e49BC8782676a84730C318bC"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode2".parse()?,
                program: node_data_dir
                    .to_path_buf()
                    .join("antnode2")
                    .join(ANTNODE_FILE_NAME),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    // Third service
    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(15002))
        .in_sequence(&mut seq);
    mock_service_control
        .expect_install()
        .times(1)
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--root-dir"),
                    OsString::from(
                        node_data_dir
                            .to_path_buf()
                            .join("antnode3")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--log-output-dest"),
                    OsString::from(
                        node_logs_dir
                            .to_path_buf()
                            .join("antnode3")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--metrics-server-port"),
                    OsString::from("15002"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-custom"),
                    OsString::from("--rpc-url"),
                    OsString::from("http://localhost:8545/"),
                    OsString::from("--payment-token-address"),
                    OsString::from("0x5FbDB2315678afecb367f032d93F642f64180aa3"),
                    OsString::from("--data-payments-address"),
                    OsString::from("0x8464135c8F25Da09e49BC8782676a84730C318bC"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode3".parse()?,
                program: node_data_dir
                    .to_path_buf()
                    .join("antnode3")
                    .join(ANTNODE_FILE_NAME),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: Some(3),
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    assert_eq!(node_registry.nodes.len(), 3);
    assert_eq!(node_registry.nodes[0].metrics_port, Some(15000));
    assert_eq!(node_registry.nodes[1].metrics_port, Some(15001));
    assert_eq!(node_registry.nodes[2].metrics_port, Some(15002));

    Ok(())
}

#[tokio::test]
async fn add_node_should_return_an_error_if_duplicate_custom_metrics_port_is_used() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: Some(12000),
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: None,
            peers_args: InitialPeersConfig::default(),
            pid: None,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Added,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
        }],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let result = add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: None,
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: Some(PortRange::Single(12000)),
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &MockServiceControl::new(),
        VerbosityLevel::Normal,
    )
    .await;

    match result {
        Ok(_) => panic!("This test is supposed to result in a failure"),
        Err(e) => {
            assert_eq!(e.to_string(), "Port 12000 is being used by another service");
            Ok(())
        }
    }
}

#[tokio::test]
async fn add_node_should_return_an_error_if_duplicate_custom_metrics_port_in_range_is_used(
) -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![NodeServiceData {
            auto_restart: false,
            connected_peers: None,
            data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            home_network: false,
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: Some(12000),
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: None,
            peers_args: InitialPeersConfig::default(),
            pid: None,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            reward_balance: Some(AttoTokens::zero()),
            rpc_socket_addr: None,
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Added,
            upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
        }],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let result = add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: Some(3),
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: Some(PortRange::Range(12000, 12002)),
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &MockServiceControl::new(),
        VerbosityLevel::Normal,
    )
    .await;

    match result {
        Ok(_) => panic!("This test is supposed to result in a failure"),
        Err(e) => {
            assert_eq!(e.to_string(), "Port 12000 is being used by another service");
            Ok(())
        }
    }
}

#[tokio::test]
async fn add_node_should_disable_upnp_and_home_network_if_nat_status_is_public() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: Some(NatDetectionStatus::Public),
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let mut seq = Sequence::new();

    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(12001))
        .in_sequence(&mut seq);

    let install_ctx = InstallNodeServiceCtxBuilder {
        autostart: false,
        data_dir_path: node_data_dir.to_path_buf().join("antnode1"),
        env_variables: None,
        evm_network: EvmNetwork::Custom(CustomNetwork {
            rpc_url_http: "http://localhost:8545".parse()?,
            payment_token_address: RewardsAddress::from_str(
                "0x5FbDB2315678afecb367f032d93F642f64180aa3",
            )?,
            data_payments_address: RewardsAddress::from_str(
                "0x8464135c8F25Da09e49BC8782676a84730C318bC",
            )?,
        }),
        home_network: false,
        log_dir_path: node_logs_dir.to_path_buf().join("antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: Some(12001),
        network_id: None,
        name: "antnode1".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        upnp: false,
    }
    .build()?;
    mock_service_control
        .expect_install()
        .times(1)
        .with(eq(install_ctx), eq(false))
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: true,
            count: None,
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: true,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: false,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    assert!(!node_registry.nodes[0].upnp);
    assert!(!node_registry.nodes[0].home_network);

    Ok(())
}

#[tokio::test]
async fn add_node_should_enable_upnp_if_nat_status_is_upnp() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: Some(NatDetectionStatus::UPnP),
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let mut seq = Sequence::new();

    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(12001))
        .in_sequence(&mut seq);

    let install_ctx = InstallNodeServiceCtxBuilder {
        autostart: false,
        data_dir_path: node_data_dir.to_path_buf().join("antnode1"),
        env_variables: None,
        evm_network: EvmNetwork::Custom(CustomNetwork {
            rpc_url_http: "http://localhost:8545".parse()?,
            payment_token_address: RewardsAddress::from_str(
                "0x5FbDB2315678afecb367f032d93F642f64180aa3",
            )?,
            data_payments_address: RewardsAddress::from_str(
                "0x8464135c8F25Da09e49BC8782676a84730C318bC",
            )?,
        }),
        home_network: false,
        log_dir_path: node_logs_dir.to_path_buf().join("antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: Some(12001),
        network_id: None,
        name: "antnode1".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        upnp: true,
    }
    .build()?;
    mock_service_control
        .expect_install()
        .times(1)
        .with(eq(install_ctx), eq(false))
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: true,
            count: None,
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: true,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    assert!(node_registry.nodes[0].upnp);
    assert!(!node_registry.nodes[0].home_network);

    Ok(())
}

#[tokio::test]
async fn add_node_should_enable_home_network_if_nat_status_is_private() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: Some(NatDetectionStatus::Private),
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let mut seq = Sequence::new();

    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(12001))
        .in_sequence(&mut seq);

    let install_ctx = InstallNodeServiceCtxBuilder {
        autostart: false,
        data_dir_path: node_data_dir.to_path_buf().join("antnode1"),
        env_variables: None,
        evm_network: EvmNetwork::Custom(CustomNetwork {
            rpc_url_http: "http://localhost:8545".parse()?,
            payment_token_address: RewardsAddress::from_str(
                "0x5FbDB2315678afecb367f032d93F642f64180aa3",
            )?,
            data_payments_address: RewardsAddress::from_str(
                "0x8464135c8F25Da09e49BC8782676a84730C318bC",
            )?,
        }),
        home_network: true,
        log_dir_path: node_logs_dir.to_path_buf().join("antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: Some(12001),
        network_id: None,
        name: "antnode1".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        upnp: false,
    }
    .build()?;
    mock_service_control
        .expect_install()
        .times(1)
        .with(eq(install_ctx), eq(false))
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: true,
            count: None,
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: false,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    assert!(!node_registry.nodes[0].upnp);
    assert!(node_registry.nodes[0].home_network);

    Ok(())
}

#[tokio::test]
async fn add_node_should_return_an_error_if_nat_status_is_none_but_auto_set_nat_flags_is_enabled(
) -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let mut seq = Sequence::new();

    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(12001))
        .in_sequence(&mut seq);

    let result = add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: true,
            count: None,
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: true,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await;

    match result {
        Ok(_) => panic!("This test should result in an error"),
        Err(e) => {
            assert_eq!(
                format!("NAT status has not been set. Run 'nat-detection' first"),
                e.to_string()
            )
        }
    }

    Ok(())
}

#[tokio::test]
async fn add_auditor_should_add_an_auditor_service() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let auditor_logs_dir = temp_dir.child("logs");
    auditor_logs_dir.create_dir_all()?;
    let auditor_install_dir = temp_dir.child("install");
    auditor_install_dir.create_dir_all()?;
    let auditor_install_path = auditor_install_dir.child(AUDITOR_FILE_NAME);
    let auditor_download_path = temp_dir.child(AUDITOR_FILE_NAME);
    auditor_download_path.write_binary(b"fake auditor bin")?;

    let mut node_registry = NodeRegistry {
        daemon: None,
        auditor: None,
        faucet: None,
        environment_variables: None,
        nat_status: None,
        nodes: vec![],
        save_path: node_reg_path.to_path_buf(),
    };

    let mut mock_service_control = MockServiceControl::new();

    mock_service_control
        .expect_install()
        .times(1)
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--log-output-dest"),
                    OsString::from(auditor_logs_dir.to_path_buf().as_os_str()),
                ],
                autostart: true,
                contents: None,
                environment: Some(vec![("ANT_LOG".to_string(), "all".to_string())]),
                label: "auditor".parse()?,
                program: auditor_install_path.to_path_buf(),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: false,
            }),
            eq(false),
        )
        .returning(|_, _| Ok(()));

    add_auditor(
        AddAuditorServiceOptions {
            beta_encryption_key: None,
            env_variables: Some(vec![("ANT_LOG".to_string(), "all".to_string())]),
            auditor_src_bin_path: auditor_download_path.to_path_buf(),
            auditor_install_bin_path: auditor_install_path.to_path_buf(),
            service_log_dir_path: auditor_logs_dir.to_path_buf(),
            user: get_username(),
            version: latest_version.to_string(),
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )?;

    auditor_download_path.assert(predicate::path::missing());
    auditor_install_path.assert(predicate::path::is_file());
    auditor_logs_dir.assert(predicate::path::is_dir());

    node_reg_path.assert(predicates::path::is_file());

    let saved_auditor = node_registry.auditor.unwrap();
    assert_eq!(
        saved_auditor.auditor_path,
        auditor_install_path.to_path_buf()
    );
    assert_eq!(saved_auditor.log_dir_path, auditor_logs_dir.to_path_buf());
    assert!(saved_auditor.pid.is_none());
    assert_eq!(saved_auditor.service_name, "auditor");
    assert_eq!(saved_auditor.status, ServiceStatus::Added);
    assert_eq!(saved_auditor.user, get_username());
    assert_eq!(saved_auditor.version, latest_version);

    Ok(())
}

#[tokio::test]
async fn add_auditor_should_return_an_error_if_a_auditor_service_was_already_created() -> Result<()>
{
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let auditor_logs_dir = temp_dir.child("logs");
    auditor_logs_dir.create_dir_all()?;
    let auditor_install_dir = temp_dir.child("install");
    auditor_install_dir.create_dir_all()?;
    let auditor_install_path = auditor_install_dir.child(AUDITOR_FILE_NAME);
    let auditor_download_path = temp_dir.child(AUDITOR_FILE_NAME);
    auditor_download_path.write_binary(b"fake auditor bin")?;

    let mut node_registry = NodeRegistry {
        daemon: None,
        auditor: Some(AuditorServiceData {
            auditor_path: auditor_download_path.to_path_buf(),
            log_dir_path: PathBuf::from("/var/log/auditor"),
            pid: Some(1000),
            service_name: "auditor".to_string(),
            status: ServiceStatus::Running,
            user: "ant".to_string(),
            version: latest_version.to_string(),
        }),
        faucet: None,
        environment_variables: None,
        nat_status: None,
        nodes: vec![],
        save_path: node_reg_path.to_path_buf(),
    };

    let result = add_auditor(
        AddAuditorServiceOptions {
            beta_encryption_key: None,
            env_variables: Some(vec![("ANT_LOG".to_string(), "all".to_string())]),
            auditor_src_bin_path: auditor_download_path.to_path_buf(),
            auditor_install_bin_path: auditor_install_path.to_path_buf(),
            service_log_dir_path: auditor_logs_dir.to_path_buf(),
            user: get_username(),
            version: latest_version.to_string(),
        },
        &mut node_registry,
        &MockServiceControl::new(),
        VerbosityLevel::Normal,
    );

    match result {
        Ok(_) => panic!("This test should result in an error"),
        Err(e) => {
            assert_eq!(
                format!("An Auditor service has already been created"),
                e.to_string()
            )
        }
    }

    Ok(())
}

#[tokio::test]
async fn add_auditor_should_include_beta_encryption_key_if_specified() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let auditor_logs_dir = temp_dir.child("logs");
    auditor_logs_dir.create_dir_all()?;
    let auditor_install_dir = temp_dir.child("install");
    auditor_install_dir.create_dir_all()?;
    let auditor_install_path = auditor_install_dir.child(AUDITOR_FILE_NAME);
    let auditor_download_path = temp_dir.child(AUDITOR_FILE_NAME);
    auditor_download_path.write_binary(b"fake auditor bin")?;

    let mut node_registry = NodeRegistry {
        daemon: None,
        auditor: None,
        faucet: None,
        environment_variables: None,
        nat_status: None,
        nodes: vec![],
        save_path: node_reg_path.to_path_buf(),
    };

    let mut mock_service_control = MockServiceControl::new();

    mock_service_control
        .expect_install()
        .times(1)
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--log-output-dest"),
                    OsString::from(auditor_logs_dir.to_path_buf().as_os_str()),
                    OsString::from("--beta-encryption-key"),
                    OsString::from("test"),
                ],
                autostart: true,
                contents: None,
                environment: Some(vec![("ANT_LOG".to_string(), "all".to_string())]),
                label: "auditor".parse()?,
                program: auditor_install_path.to_path_buf(),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: false,
            }),
            eq(false),
        )
        .returning(|_, _| Ok(()));

    add_auditor(
        AddAuditorServiceOptions {
            beta_encryption_key: Some("test".to_string()),
            env_variables: Some(vec![("ANT_LOG".to_string(), "all".to_string())]),
            auditor_src_bin_path: auditor_download_path.to_path_buf(),
            auditor_install_bin_path: auditor_install_path.to_path_buf(),
            service_log_dir_path: auditor_logs_dir.to_path_buf(),
            user: get_username(),
            version: latest_version.to_string(),
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )?;

    auditor_download_path.assert(predicate::path::missing());
    auditor_install_path.assert(predicate::path::is_file());
    auditor_logs_dir.assert(predicate::path::is_dir());

    node_reg_path.assert(predicates::path::is_file());

    let saved_auditor = node_registry.auditor.unwrap();
    assert_eq!(
        saved_auditor.auditor_path,
        auditor_install_path.to_path_buf()
    );
    assert_eq!(saved_auditor.log_dir_path, auditor_logs_dir.to_path_buf());
    assert!(saved_auditor.pid.is_none());
    assert_eq!(saved_auditor.service_name, "auditor");
    assert_eq!(saved_auditor.status, ServiceStatus::Added);
    assert_eq!(saved_auditor.user, get_username());
    assert_eq!(saved_auditor.version, latest_version);

    Ok(())
}

#[tokio::test]
async fn add_faucet_should_add_a_faucet_service() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let faucet_logs_dir = temp_dir.child("logs");
    faucet_logs_dir.create_dir_all()?;
    let faucet_data_dir = temp_dir.child("data");
    faucet_data_dir.create_dir_all()?;
    let faucet_install_dir = temp_dir.child("install");
    faucet_install_dir.create_dir_all()?;
    let faucet_install_path = faucet_install_dir.child(FAUCET_FILE_NAME);
    let faucet_download_path = temp_dir.child(FAUCET_FILE_NAME);
    faucet_download_path.write_binary(b"fake faucet bin")?;

    let mut node_registry = NodeRegistry {
        daemon: None,
        auditor: None,
        faucet: None,
        environment_variables: None,
        nat_status: None,
        nodes: vec![],
        save_path: node_reg_path.to_path_buf(),
    };

    let mut mock_service_control = MockServiceControl::new();

    mock_service_control
        .expect_install()
        .times(1)
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--log-output-dest"),
                    OsString::from(faucet_logs_dir.to_path_buf().as_os_str()),
                    OsString::from("server"),
                ],
                autostart: true,
                contents: None,
                environment: Some(vec![("ANT_LOG".to_string(), "all".to_string())]),
                label: "faucet".parse()?,
                program: faucet_install_path.to_path_buf(),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: false,
            }),
            eq(false),
        )
        .returning(|_, _| Ok(()));

    add_faucet(
        AddFaucetServiceOptions {
            env_variables: Some(vec![("ANT_LOG".to_string(), "all".to_string())]),
            faucet_src_bin_path: faucet_download_path.to_path_buf(),
            faucet_install_bin_path: faucet_install_path.to_path_buf(),
            local: false,
            service_data_dir_path: faucet_data_dir.to_path_buf(),
            service_log_dir_path: faucet_logs_dir.to_path_buf(),
            user: get_username(),
            version: latest_version.to_string(),
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )?;

    faucet_download_path.assert(predicate::path::missing());
    faucet_install_path.assert(predicate::path::is_file());
    faucet_logs_dir.assert(predicate::path::is_dir());

    node_reg_path.assert(predicates::path::is_file());

    let saved_faucet = node_registry.faucet.unwrap();
    assert_eq!(saved_faucet.faucet_path, faucet_install_path.to_path_buf());
    assert!(!saved_faucet.local);
    assert_eq!(saved_faucet.log_dir_path, faucet_logs_dir.to_path_buf());
    assert!(saved_faucet.pid.is_none());
    assert_eq!(saved_faucet.service_name, "faucet");
    assert_eq!(saved_faucet.status, ServiceStatus::Added);
    assert_eq!(saved_faucet.user, get_username());
    assert_eq!(saved_faucet.version, latest_version);

    Ok(())
}

#[tokio::test]
async fn add_faucet_should_return_an_error_if_a_faucet_service_was_already_created() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let faucet_logs_dir = temp_dir.child("logs");
    faucet_logs_dir.create_dir_all()?;
    let faucet_data_dir = temp_dir.child("data");
    faucet_data_dir.create_dir_all()?;
    let faucet_install_dir = temp_dir.child("install");
    faucet_install_dir.create_dir_all()?;
    let faucet_install_path = faucet_install_dir.child(FAUCET_FILE_NAME);
    let faucet_download_path = temp_dir.child(FAUCET_FILE_NAME);
    faucet_download_path.write_binary(b"fake faucet bin")?;

    let mut node_registry = NodeRegistry {
        daemon: None,
        auditor: None,
        faucet: Some(FaucetServiceData {
            faucet_path: faucet_download_path.to_path_buf(),
            local: false,
            log_dir_path: PathBuf::from("/var/log/faucet"),
            pid: Some(1000),
            service_name: "faucet".to_string(),
            status: ServiceStatus::Running,
            user: "ant".to_string(),
            version: latest_version.to_string(),
        }),
        environment_variables: None,
        nat_status: None,
        nodes: vec![],
        save_path: node_reg_path.to_path_buf(),
    };

    let result = add_faucet(
        AddFaucetServiceOptions {
            env_variables: Some(vec![("ANT_LOG".to_string(), "all".to_string())]),
            faucet_src_bin_path: faucet_download_path.to_path_buf(),
            faucet_install_bin_path: faucet_install_path.to_path_buf(),
            local: false,
            service_data_dir_path: faucet_data_dir.to_path_buf(),
            service_log_dir_path: faucet_logs_dir.to_path_buf(),
            user: get_username(),
            version: latest_version.to_string(),
        },
        &mut node_registry,
        &MockServiceControl::new(),
        VerbosityLevel::Normal,
    );

    match result {
        Ok(_) => panic!("This test should result in an error"),
        Err(e) => {
            assert_eq!(
                format!("A faucet service has already been created"),
                e.to_string()
            )
        }
    }

    Ok(())
}

#[tokio::test]
async fn add_daemon_should_add_a_daemon_service() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let daemon_install_dir = temp_dir.child("install");
    daemon_install_dir.create_dir_all()?;
    let daemon_install_path = daemon_install_dir.child(DAEMON_FILE_NAME);
    let daemon_download_path = temp_dir.child(DAEMON_FILE_NAME);
    daemon_download_path.write_binary(b"fake daemon bin")?;

    let mut node_registry = NodeRegistry {
        daemon: None,
        auditor: None,
        faucet: None,
        environment_variables: None,
        nat_status: None,
        nodes: vec![],
        save_path: node_reg_path.to_path_buf(),
    };

    let mut mock_service_control = MockServiceControl::new();

    mock_service_control
        .expect_install()
        .times(1)
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--port"),
                    OsString::from("8080"),
                    OsString::from("--address"),
                    OsString::from("127.0.0.1"),
                ],
                autostart: true,
                contents: None,
                environment: Some(vec![("ANT_LOG".to_string(), "ALL".to_string())]),
                label: "antctld".parse()?,
                program: daemon_install_path.to_path_buf(),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: false,
            }),
            eq(false),
        )
        .returning(|_, _| Ok(()));

    add_daemon(
        AddDaemonServiceOptions {
            address: Ipv4Addr::new(127, 0, 0, 1),
            daemon_install_bin_path: daemon_install_path.to_path_buf(),
            daemon_src_bin_path: daemon_download_path.to_path_buf(),
            env_variables: Some(vec![("ANT_LOG".to_string(), "ALL".to_string())]),
            port: 8080,
            user: get_username(),
            version: latest_version.to_string(),
        },
        &mut node_registry,
        &mock_service_control,
    )?;

    daemon_download_path.assert(predicate::path::missing());
    daemon_install_path.assert(predicate::path::is_file());

    node_reg_path.assert(predicates::path::is_file());

    let saved_daemon = node_registry.daemon.unwrap();
    assert_eq!(saved_daemon.daemon_path, daemon_install_path.to_path_buf());
    assert!(saved_daemon.pid.is_none());
    assert_eq!(saved_daemon.service_name, "antctld");
    assert_eq!(saved_daemon.status, ServiceStatus::Added);
    assert_eq!(saved_daemon.version, latest_version);

    Ok(())
}

#[tokio::test]
async fn add_daemon_should_return_an_error_if_a_daemon_service_was_already_created() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let daemon_install_dir = temp_dir.child("install");
    daemon_install_dir.create_dir_all()?;
    let daemon_install_path = daemon_install_dir.child(DAEMON_FILE_NAME);
    let daemon_download_path = temp_dir.child(DAEMON_FILE_NAME);
    daemon_download_path.write_binary(b"fake daemon bin")?;

    let mut node_registry = NodeRegistry {
        daemon: Some(DaemonServiceData {
            daemon_path: PathBuf::from("/usr/local/bin/antctld"),
            endpoint: Some(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                8080,
            )),
            pid: Some(1234),
            service_name: "antctld".to_string(),
            status: ServiceStatus::Running,
            version: latest_version.to_string(),
        }),
        auditor: None,
        faucet: None,
        environment_variables: None,
        nat_status: None,
        nodes: vec![],
        save_path: node_reg_path.to_path_buf(),
    };

    let result = add_daemon(
        AddDaemonServiceOptions {
            address: Ipv4Addr::new(127, 0, 0, 1),
            daemon_install_bin_path: daemon_install_path.to_path_buf(),
            daemon_src_bin_path: daemon_download_path.to_path_buf(),
            env_variables: Some(Vec::new()),
            port: 8080,
            user: get_username(),
            version: latest_version.to_string(),
        },
        &mut node_registry,
        &MockServiceControl::new(),
    );

    match result {
        Ok(_) => panic!("This test should result in an error"),
        Err(e) => {
            assert_eq!(
                format!("A antctld service has already been created"),
                e.to_string()
            )
        }
    }

    Ok(())
}

#[tokio::test]
async fn add_node_should_not_delete_the_source_binary_if_path_arg_is_used() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };

    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let mut seq = Sequence::new();

    // Expected calls for first installation
    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(8081))
        .in_sequence(&mut seq);

    let install_ctx = InstallNodeServiceCtxBuilder {
        autostart: false,
        data_dir_path: node_data_dir.to_path_buf().join("antnode1"),
        env_variables: None,
        evm_network: EvmNetwork::Custom(CustomNetwork {
            rpc_url_http: "http://localhost:8545".parse()?,
            payment_token_address: RewardsAddress::from_str(
                "0x5FbDB2315678afecb367f032d93F642f64180aa3",
            )?,
            data_payments_address: RewardsAddress::from_str(
                "0x8464135c8F25Da09e49BC8782676a84730C318bC",
            )?,
        }),
        home_network: false,
        log_dir_path: node_logs_dir.to_path_buf().join("antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: Some(8081),
        network_id: None,
        name: "antnode1".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        upnp: false,
    }
    .build()?;

    mock_service_control
        .expect_install()
        .times(1)
        .with(eq(install_ctx), eq(false))
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: Some(1),
            delete_antnode_src: false,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::is_file());

    Ok(())
}

#[tokio::test]
async fn add_node_should_apply_the_home_network_flag_if_it_is_used() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };

    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let mut seq = Sequence::new();

    // Expected calls for first installation
    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(8081))
        .in_sequence(&mut seq);

    let install_ctx = InstallNodeServiceCtxBuilder {
        autostart: false,
        data_dir_path: node_data_dir.to_path_buf().join("antnode1"),
        env_variables: None,
        home_network: true,
        evm_network: EvmNetwork::Custom(CustomNetwork {
            rpc_url_http: "http://localhost:8545".parse()?,
            payment_token_address: RewardsAddress::from_str(
                "0x5FbDB2315678afecb367f032d93F642f64180aa3",
            )?,
            data_payments_address: RewardsAddress::from_str(
                "0x8464135c8F25Da09e49BC8782676a84730C318bC",
            )?,
        }),
        log_dir_path: node_logs_dir.to_path_buf().join("antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: Some(8081),
        network_id: None,
        name: "antnode1".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        upnp: false,
    }
    .build()?;

    mock_service_control
        .expect_install()
        .times(1)
        .with(eq(install_ctx), eq(false))
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: Some(1),
            delete_antnode_src: false,
            enable_metrics_server: false,
            env_variables: None,
            relay: true,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    assert!(node_registry.nodes[0].home_network);

    Ok(())
}

#[tokio::test]
async fn add_node_should_add_the_node_in_user_mode() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };

    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let mut seq = Sequence::new();

    // Expected calls for first installation
    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(8081))
        .in_sequence(&mut seq);

    let install_ctx = InstallNodeServiceCtxBuilder {
        autostart: false,
        data_dir_path: node_data_dir.to_path_buf().join("antnode1"),
        env_variables: None,
        evm_network: EvmNetwork::Custom(CustomNetwork {
            rpc_url_http: "http://localhost:8545".parse()?,
            payment_token_address: RewardsAddress::from_str(
                "0x5FbDB2315678afecb367f032d93F642f64180aa3",
            )?,
            data_payments_address: RewardsAddress::from_str(
                "0x8464135c8F25Da09e49BC8782676a84730C318bC",
            )?,
        }),
        home_network: true,
        log_dir_path: node_logs_dir.to_path_buf().join("antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: Some(8081),
        network_id: None,
        name: "antnode1".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        upnp: false,
    }
    .build()?;

    mock_service_control
        .expect_install()
        .times(1)
        .with(eq(install_ctx), eq(true))
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: Some(1),
            delete_antnode_src: false,
            enable_metrics_server: false,
            env_variables: None,
            relay: true,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: true,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn add_node_should_add_the_node_with_upnp_enabled() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let mut node_registry = NodeRegistry {
        auditor: None,
        faucet: None,
        save_path: node_reg_path.to_path_buf(),
        nat_status: None,
        nodes: vec![],
        environment_variables: None,
        daemon: None,
    };

    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let mut seq = Sequence::new();

    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(8081))
        .in_sequence(&mut seq);

    let install_ctx = InstallNodeServiceCtxBuilder {
        autostart: false,
        data_dir_path: node_data_dir.to_path_buf().join("antnode1"),
        env_variables: None,
        evm_network: EvmNetwork::Custom(CustomNetwork {
            rpc_url_http: "http://localhost:8545".parse()?,
            payment_token_address: RewardsAddress::from_str(
                "0x5FbDB2315678afecb367f032d93F642f64180aa3",
            )?,
            data_payments_address: RewardsAddress::from_str(
                "0x8464135c8F25Da09e49BC8782676a84730C318bC",
            )?,
        }),
        home_network: true,
        log_dir_path: node_logs_dir.to_path_buf().join("antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: Some(8081),
        network_id: None,
        name: "antnode1".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        upnp: true,
    }
    .build()?;

    mock_service_control
        .expect_install()
        .times(1)
        .with(eq(install_ctx), eq(true))
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: false,
            auto_set_nat_flags: false,
            count: Some(1),
            delete_antnode_src: false,
            enable_metrics_server: false,
            env_variables: None,
            relay: true,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: false,
            user: Some(get_username()),
            user_mode: true,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    assert_eq!(node_registry.nodes.len(), 1);
    assert!(node_registry.nodes[0].upnp);

    Ok(())
}

#[tokio::test]
async fn add_node_should_auto_restart() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let mut node_registry = NodeRegistry {
        auditor: None,
        daemon: None,
        environment_variables: None,
        faucet: None,
        nat_status: None,
        nodes: vec![],
        save_path: node_reg_path.to_path_buf(),
    };

    let mut mock_service_control = MockServiceControl::new();
    let mut seq = Sequence::new();
    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(8081))
        .in_sequence(&mut seq);

    mock_service_control
        .expect_install()
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--root-dir"),
                    OsString::from(
                        node_data_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--log-output-dest"),
                    OsString::from(
                        node_logs_dir
                            .to_path_buf()
                            .join("antnode1")
                            .to_string_lossy()
                            .to_string(),
                    ),
                    OsString::from("--metrics-server-port"),
                    OsString::from("8081"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-custom"),
                    OsString::from("--rpc-url"),
                    OsString::from("http://localhost:8545/"),
                    OsString::from("--payment-token-address"),
                    OsString::from("0x5FbDB2315678afecb367f032d93F642f64180aa3"),
                    OsString::from("--data-payments-address"),
                    OsString::from("0x8464135c8F25Da09e49BC8782676a84730C318bC"),
                ],
                autostart: true,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: node_data_dir
                    .to_path_buf()
                    .join("antnode1")
                    .join(ANTNODE_FILE_NAME),
                username: Some(get_username()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .times(1)
        .returning(|_, _| Ok(()))
        .in_sequence(&mut seq);

    add_node(
        AddNodeServiceOptions {
            auto_restart: true,
            auto_set_nat_flags: false,
            count: None,
            delete_antnode_src: true,
            enable_metrics_server: false,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            no_upnp: true,
            user: Some(get_username()),
            user_mode: false,
            version: latest_version.to_string(),
            evm_network: EvmNetwork::Custom(CustomNetwork {
                rpc_url_http: "http://localhost:8545".parse()?,
                payment_token_address: RewardsAddress::from_str(
                    "0x5FbDB2315678afecb367f032d93F642f64180aa3",
                )?,
                data_payments_address: RewardsAddress::from_str(
                    "0x8464135c8F25Da09e49BC8782676a84730C318bC",
                )?,
            }),
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
        },
        &mut node_registry,
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    assert!(node_registry.nodes[0].auto_restart);

    Ok(())
}
