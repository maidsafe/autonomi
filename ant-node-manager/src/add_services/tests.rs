// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    VerbosityLevel,
    add_services::{
        add_node,
        config::{AddNodeServiceOptions, InstallNodeServiceCtxBuilder, PortRange},
    },
};
use ant_bootstrap::InitialPeersConfig;
use ant_evm::{CustomNetwork, EvmNetwork, RewardsAddress};
use ant_service_management::error::Result as ServiceControlResult;
use ant_service_management::{
    NodeRegistryManager, NodeServiceData, ReachabilityProgress, ServiceStatus,
};
use ant_service_management::{control::ServiceControl, node::NODE_SERVICE_DATA_SCHEMA_LATEST};
use assert_fs::prelude::*;
use assert_matches::assert_matches;
use color_eyre::Result;
use mockall::{Sequence, mock, predicate::*};
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

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());

    let mut mock_service_control = MockServiceControl::new();
    let mut seq = Sequence::new();
    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(6001))
        .in_sequence(&mut seq);

    let init_peers_config = InitialPeersConfig {
        first: true,
        addrs: vec![],
        network_contacts_url: vec![],
        local: false,
        ignore_cache: false,
        bootstrap_cache_dir: None,
    };

    let install_ctx = InstallNodeServiceCtxBuilder {
        alpha: false,
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
        log_dir_path: node_logs_dir.to_path_buf().join("antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: 6001,
        name: "antnode1".to_string(),
        network_id: None,
        node_ip: None,
        node_port: None,
        init_peers_config: init_peers_config.clone(),
        skip_reachability_check: false,
        relay: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        rpc_socket_addr: None,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        no_upnp: false,
        write_older_cache_files: false,
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
            alpha: false,
            auto_restart: false,
            count: None,
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config,
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());

    node_reg_path.assert(predicates::path::is_file());
    let len = node_registry.nodes.read().await.len();
    assert_eq!(len, 1);
    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    assert!(node0.initial_peers_config.first);
    assert_eq!(node0.version, latest_version);
    assert_eq!(node0.service_name, "antnode1");
    assert_eq!(node0.user, Some(get_username()));
    assert_eq!(node0.number, 1);
    assert_eq!(node0.rpc_socket_addr, None);
    assert_eq!(
        node0.log_dir_path,
        node_logs_dir.to_path_buf().join("antnode1")
    );
    assert_eq!(
        node0.data_dir_path,
        node_data_dir.to_path_buf().join("antnode1")
    );
    assert_matches!(node0.status, ServiceStatus::Added);
    assert_eq!(
        node0.evm_network,
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
        node0.rewards_address,
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
        ignore_cache: false,
        bootstrap_cache_dir: None,
    };
    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
    node_registry
        .push_node(NodeServiceData {
            alpha: false,
            auto_restart: false,
            connected_peers: 10,
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
            initial_peers_config: init_peers_config.clone(),
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: 6001,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: None,
            pid: None,
            reachability_progress: ReachabilityProgress::NotRun,
            relay: false,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            rpc_socket_addr: Some(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                8081,
            )),
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
            service_name: "antnode1".to_string(),
            skip_reachability_check: false,
            status: ServiceStatus::Added,
            no_upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: latest_version.to_string(),
            write_older_cache_files: false,
        })
        .await;

    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("antnode1");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let custom_rpc_address = Ipv4Addr::new(127, 0, 0, 1);

    let result = add_node(
        AddNodeServiceOptions {
            alpha: false,
            auto_restart: false,

            count: None,
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config,
            skip_reachability_check: false,
            relay: false,
            rpc_address: Some(custom_rpc_address),
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
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

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());

    let init_peers_config = InitialPeersConfig {
        first: true,
        addrs: vec![],
        network_contacts_url: vec![],
        local: false,
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
            alpha: false,
            auto_restart: false,

            count: Some(3),
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config,
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
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
    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());

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
        .returning(|| Ok(6001))
        .in_sequence(&mut seq);

    let install_ctx = InstallNodeServiceCtxBuilder {
        alpha: false,
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
        log_dir_path: node_logs_dir.to_path_buf().join("antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: 6001,
        network_id: None,
        name: "antnode1".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        skip_reachability_check: false,
        relay: false,
        rpc_socket_addr: None,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        service_user: Some(get_username()),
        no_upnp: false,
        write_older_cache_files: false,
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
        .returning(|| Ok(6003))
        .in_sequence(&mut seq);
    let install_ctx = InstallNodeServiceCtxBuilder {
        alpha: false,
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
        log_dir_path: node_logs_dir.to_path_buf().join("antnode2"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: 6003,
        network_id: None,
        name: "antnode2".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        skip_reachability_check: false,
        relay: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        rpc_socket_addr: None,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode2")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        no_upnp: false,
        write_older_cache_files: false,
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
        .returning(|| Ok(6005))
        .in_sequence(&mut seq);
    let install_ctx = InstallNodeServiceCtxBuilder {
        alpha: false,
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
        log_format: None,
        log_dir_path: node_logs_dir.to_path_buf().join("antnode3"),
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: 6005,
        network_id: None,
        name: "antnode3".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        skip_reachability_check: false,
        relay: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        rpc_socket_addr: None,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode3")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        no_upnp: false,
        write_older_cache_files: false,
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
            alpha: false,
            auto_restart: false,

            count: Some(3),
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            relay: false,
            skip_reachability_check: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    let nodes_len = node_registry.nodes.read().await.len();
    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    let node1 = node_registry.nodes.read().await[1].read().await.clone();
    let node2 = node_registry.nodes.read().await[2].read().await.clone();

    assert_eq!(nodes_len, 3);
    assert_eq!(node0.version, latest_version);
    assert_eq!(node0.service_name, "antnode1");
    assert_eq!(node0.user, Some(get_username()));
    assert_eq!(node0.number, 1);
    assert_eq!(node0.rpc_socket_addr, None);
    assert_eq!(
        node0.log_dir_path,
        node_logs_dir.to_path_buf().join("antnode1")
    );
    assert_eq!(
        node0.data_dir_path,
        node_data_dir.to_path_buf().join("antnode1")
    );
    assert_matches!(node0.status, ServiceStatus::Added);
    assert_eq!(node1.version, latest_version);
    assert_eq!(node1.service_name, "antnode2");
    assert_eq!(node1.user, Some(get_username()));
    assert_eq!(node1.number, 2);
    assert_eq!(node1.rpc_socket_addr, None);
    assert_eq!(
        node1.log_dir_path,
        node_logs_dir.to_path_buf().join("antnode2")
    );
    assert_eq!(
        node1.data_dir_path,
        node_data_dir.to_path_buf().join("antnode2")
    );
    assert_matches!(node1.status, ServiceStatus::Added);
    assert_eq!(node2.version, latest_version);
    assert_eq!(node2.service_name, "antnode3");
    assert_eq!(node2.user, Some(get_username()));
    assert_eq!(node2.number, 3);
    assert_eq!(node2.rpc_socket_addr, None);
    assert_eq!(
        node2.log_dir_path,
        node_logs_dir.to_path_buf().join("antnode3")
    );
    assert_eq!(
        node2.data_dir_path,
        node_data_dir.to_path_buf().join("antnode3")
    );
    assert_matches!(node2.status, ServiceStatus::Added);

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

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());

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
        .returning(|| Ok(6001))
        .in_sequence(&mut seq);
    let install_ctx = InstallNodeServiceCtxBuilder {
        alpha: false,
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
        log_dir_path: node_logs_dir.to_path_buf().join("antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: 6001,
        network_id: None,
        name: "antnode1".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        skip_reachability_check: false,
        relay: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        rpc_socket_addr: None,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        no_upnp: false,
        write_older_cache_files: false,
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
            alpha: false,
            auto_restart: false,

            count: None,
            delete_antnode_src: true,
            env_variables: env_variables.clone(),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());

    assert_eq!(
        *node_registry.environment_variables.read().await,
        env_variables
    );

    assert_eq!(node_registry.nodes.read().await.len(), 1);
    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    assert_eq!(node0.version, latest_version);
    assert_eq!(node0.service_name, "antnode1");
    assert_eq!(node0.user, Some(get_username()));
    assert_eq!(node0.number, 1);
    assert_eq!(node0.rpc_socket_addr, None);
    assert_eq!(
        node0.log_dir_path,
        node_logs_dir.to_path_buf().join("antnode1")
    );
    assert_eq!(
        node0.data_dir_path,
        node_data_dir.to_path_buf().join("antnode1")
    );
    assert_matches!(node0.status, ServiceStatus::Added);

    Ok(())
}

#[tokio::test]
async fn add_new_node_should_add_another_service() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let latest_version = "0.96.4";
    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
    node_registry
        .push_node(NodeServiceData {
            alpha: false,
            auto_restart: false,
            connected_peers: 10,
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
            initial_peers_config: Default::default(),
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: 6001,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: None,
            pid: None,
            reachability_progress: ReachabilityProgress::NotRun,
            relay: false,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            rpc_socket_addr: Some(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                8081,
            )),
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
            service_name: "antnode1".to_string(),
            skip_reachability_check: false,
            status: ServiceStatus::Added,
            no_upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: latest_version.to_string(),
            write_older_cache_files: false,
        })
        .await;
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
        .returning(|| Ok(6003))
        .in_sequence(&mut seq);
    let install_ctx = InstallNodeServiceCtxBuilder {
        alpha: false,
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
        log_dir_path: node_logs_dir.to_path_buf().join("antnode2"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: 6003,
        network_id: None,
        name: "antnode2".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        skip_reachability_check: false,
        relay: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        rpc_socket_addr: None,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode2")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        no_upnp: false,
        write_older_cache_files: false,
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
            alpha: false,
            auto_restart: false,

            count: None,
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
            antnode_src_path: antnode_download_path.to_path_buf(),
            antnode_dir_path: temp_dir.to_path_buf(),
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    assert_eq!(node_registry.nodes.read().await.len(), 2);
    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    let node1 = node_registry.nodes.read().await[1].read().await.clone();
    assert_eq!(node1.version, latest_version);
    assert_eq!(node1.service_name, "antnode2");
    assert_eq!(node1.user, Some(get_username()));
    assert_eq!(node1.number, 2);
    assert_eq!(
        node0.rpc_socket_addr,
        Some(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            8081
        ))
    );
    assert_eq!(node1.rpc_socket_addr, None);
    assert_eq!(
        node1.log_dir_path,
        node_logs_dir.to_path_buf().join("antnode2")
    );
    assert_eq!(
        node1.data_dir_path,
        node_data_dir.to_path_buf().join("antnode2")
    );
    assert_matches!(node0.status, ServiceStatus::Added);
    assert!(!node0.auto_restart);

    Ok(())
}

#[tokio::test]
async fn add_node_should_create_service_file_with_first_arg() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let init_peers_config = InitialPeersConfig {
        first: true,
        addrs: vec![],
        network_contacts_url: vec![],
        local: false,
        ignore_cache: false,
        bootstrap_cache_dir: None,
    };

    let mut seq = Sequence::new();

    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(6001))
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
                    OsString::from("6001"),
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
            alpha: false,
            auto_restart: false,

            count: None,
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: init_peers_config.clone(),
            relay: false,
            skip_reachability_check: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());
    assert_eq!(node_registry.nodes.read().await.len(), 1);
    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    assert_eq!(node0.version, latest_version);
    assert_eq!(node0.initial_peers_config, init_peers_config);
    assert!(node0.initial_peers_config.first);

    Ok(())
}

#[tokio::test]
async fn add_node_should_create_service_file_with_peers_args() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let initial_peers_config = InitialPeersConfig {
        first: false,
        addrs: vec![
            "/ip4/127.0.0.1/tcp/8080/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
                .parse()?,
        ],
        network_contacts_url: vec![],
        local: false,
        ignore_cache: false,
        bootstrap_cache_dir: None,
    };

    let mut seq = Sequence::new();

    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(6001))
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
                    OsString::from("6001"),
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
            alpha: false,
            auto_restart: false,

            count: None,
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: initial_peers_config.clone(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());
    assert_eq!(node_registry.nodes.read().await.len(), 1);
    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    assert_eq!(node0.version, latest_version);
    assert_eq!(node0.initial_peers_config, initial_peers_config);
    assert_eq!(node0.initial_peers_config.addrs.len(), 1);

    Ok(())
}

#[tokio::test]
async fn add_node_should_create_service_file_with_local_arg() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let init_peers_config = InitialPeersConfig {
        first: false,
        addrs: vec![],
        network_contacts_url: vec![],
        local: true,
        ignore_cache: false,
        bootstrap_cache_dir: None,
    };

    let mut seq = Sequence::new();

    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(6001))
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
                    OsString::from("6001"),
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
            alpha: false,
            auto_restart: false,

            count: None,
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: init_peers_config.clone(),
            relay: false,
            skip_reachability_check: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());
    assert_eq!(node_registry.nodes.read().await.len(), 1);
    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    assert_eq!(node0.version, latest_version);
    assert_eq!(node0.initial_peers_config, init_peers_config);
    assert!(node0.initial_peers_config.local);

    Ok(())
}

#[tokio::test]
async fn add_node_should_create_service_file_with_network_contacts_url_arg() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let init_peers_config = InitialPeersConfig {
        first: false,
        addrs: vec![],
        network_contacts_url: vec![
            "http://localhost:8080/contacts".to_string(),
            "http://localhost:8081/contacts".to_string(),
        ],
        local: false,
        ignore_cache: false,
        bootstrap_cache_dir: None,
    };

    let mut seq = Sequence::new();

    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(6001))
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
                    OsString::from("6001"),
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
            alpha: false,
            auto_restart: false,

            count: None,
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: init_peers_config.clone(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());
    assert_eq!(node_registry.nodes.read().await.len(), 1);
    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    assert_eq!(node0.version, latest_version);
    assert_eq!(node0.initial_peers_config, init_peers_config);
    assert_eq!(node0.initial_peers_config.network_contacts_url.len(), 2);

    Ok(())
}

#[tokio::test]
async fn add_node_should_create_service_file_with_ignore_cache_arg() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let init_peers_config = InitialPeersConfig {
        first: false,
        addrs: vec![],
        network_contacts_url: vec![],
        local: false,
        ignore_cache: true,
        bootstrap_cache_dir: None,
    };

    let mut seq = Sequence::new();

    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(6001))
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
                    OsString::from("6001"),
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
            alpha: false,
            auto_restart: false,

            count: None,
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: init_peers_config.clone(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());
    assert_eq!(node_registry.nodes.read().await.len(), 1);
    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    assert_eq!(node0.version, latest_version);
    assert_eq!(node0.initial_peers_config, init_peers_config);
    assert!(node0.initial_peers_config.ignore_cache);

    Ok(())
}

#[tokio::test]
async fn add_node_should_create_service_file_with_custom_bootstrap_cache_path() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let initial_peers_config = InitialPeersConfig {
        first: false,
        addrs: vec![],
        network_contacts_url: vec![],
        local: false,
        ignore_cache: false,
        bootstrap_cache_dir: Some(PathBuf::from("/path/to/bootstrap/cache")),
    };

    let mut seq = Sequence::new();

    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(6001))
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
                    OsString::from("6001"),
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
            alpha: false,
            auto_restart: false,

            count: None,
            delete_antnode_src: true,
            env_variables: None,
            init_peers_config: initial_peers_config.clone(),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());
    assert_eq!(node_registry.nodes.read().await.len(), 1);
    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    assert_eq!(node0.version, latest_version);
    assert_eq!(node0.initial_peers_config, initial_peers_config);
    assert_eq!(
        node0.initial_peers_config.bootstrap_cache_dir,
        Some(PathBuf::from("/path/to/bootstrap/cache"))
    );

    Ok(())
}

#[tokio::test]
async fn add_node_should_create_service_file_with_network_id() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
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
        .returning(|| Ok(6001))
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
                    OsString::from("6001"),
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
            alpha: false,
            auto_restart: false,

            count: None,
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: Some(5),
            node_ip: None,
            node_port: None,
            init_peers_config: Default::default(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());
    assert_eq!(node_registry.nodes.read().await.len(), 1);
    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    assert_eq!(node0.version, latest_version);
    assert_eq!(node0.network_id, Some(5));

    Ok(())
}

#[tokio::test]
async fn add_node_should_use_custom_ip() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
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
        .returning(|| Ok(6001))
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
                    OsString::from("6001"),
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
            alpha: false,
            auto_restart: false,

            count: None,
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: Some(custom_ip),
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());

    assert_eq!(node_registry.nodes.read().await.len(), 1);
    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    assert_eq!(node0.node_ip, Some(custom_ip));

    Ok(())
}

#[tokio::test]
async fn add_node_should_use_custom_ports_for_one_service() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
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
        .returning(|| Ok(6001))
        .in_sequence(&mut seq);
    let install_ctx = InstallNodeServiceCtxBuilder {
        alpha: false,
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
        log_dir_path: node_logs_dir.to_path_buf().join("antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: 6001,
        network_id: None,
        name: "antnode1".to_string(),
        node_ip: None,
        node_port: Some(custom_port),
        init_peers_config: InitialPeersConfig::default(),
        skip_reachability_check: false,
        relay: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        rpc_socket_addr: None,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        no_upnp: false,
        write_older_cache_files: false,
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
            alpha: false,
            auto_restart: false,

            count: None,
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: Some(PortRange::Single(custom_port)),
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());

    assert_eq!(node_registry.nodes.read().await.len(), 1);
    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    assert_eq!(node0.node_port, Some(custom_port));

    Ok(())
}

#[tokio::test]
async fn add_node_should_use_a_custom_port_range() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
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
        .returning(|| Ok(6001))
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
                    OsString::from("6001"),
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
        .returning(|| Ok(6002))
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
                    OsString::from("6002"),
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
        .returning(|| Ok(6003))
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
                    OsString::from("6003"),
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
            alpha: false,
            auto_restart: false,

            count: Some(3),
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: Some(PortRange::Range(12000, 12002)),
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());
    assert_eq!(node_registry.nodes.read().await.len(), 3);
    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    let node1 = node_registry.nodes.read().await[1].read().await.clone();
    let node2 = node_registry.nodes.read().await[2].read().await.clone();
    assert_eq!(node0.node_port, Some(12000));
    assert_eq!(node1.node_port, Some(12001));
    assert_eq!(node2.node_port, Some(12002));

    Ok(())
}

#[tokio::test]
async fn add_node_should_return_an_error_if_duplicate_custom_port_is_used() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
    node_registry
        .push_node(NodeServiceData {
            alpha: false,
            auto_restart: false,
            connected_peers: 10,
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
            initial_peers_config: Default::default(),
            listen_addr: None,
            log_format: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: 6001,
            network_id: None,
            node_ip: None,
            node_port: Some(12000),
            number: 1,
            peer_id: None,
            pid: None,
            reachability_progress: ReachabilityProgress::NotRun,
            relay: false,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            rpc_socket_addr: Some(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                8081,
            )),
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
            service_name: "antnode1".to_string(),
            skip_reachability_check: false,
            status: ServiceStatus::Added,
            no_upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
            write_older_cache_files: false,
        })
        .await;
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
            alpha: false,
            auto_restart: false,

            count: None,
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: Some(PortRange::Single(12000)),
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
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

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
    node_registry
        .push_node(NodeServiceData {
            alpha: false,
            auto_restart: false,
            connected_peers: 10,
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
            initial_peers_config: Default::default(),
            listen_addr: None,
            log_format: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: 6001,
            network_id: None,
            node_ip: None,
            node_port: Some(12000),
            number: 1,
            peer_id: None,
            pid: None,
            reachability_progress: ReachabilityProgress::NotRun,
            relay: false,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            rpc_socket_addr: Some(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                8081,
            )),
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
            service_name: "antnode1".to_string(),
            skip_reachability_check: false,
            status: ServiceStatus::Added,
            no_upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
            write_older_cache_files: false,
        })
        .await;
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
            alpha: false,
            auto_restart: false,

            count: Some(3),
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: Some(PortRange::Range(12000, 12002)),
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry,
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

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
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
            alpha: false,
            auto_restart: false,

            count: Some(2),
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: Some(PortRange::Range(12000, 12002)),
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
            antnode_src_path: antnode_download_path.to_path_buf(),
            antnode_dir_path: temp_dir.to_path_buf(),
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
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
async fn add_node_should_return_an_error_if_multiple_services_are_specified_with_a_single_port()
-> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
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
            alpha: false,
            auto_restart: false,

            count: Some(2),
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: Some(PortRange::Single(12000)),
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry,
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
async fn add_node_should_set_random_ports_for_metrics_server() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
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
        .returning(|| Ok(6001))
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
                    OsString::from("6001"),
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
            alpha: false,
            auto_restart: false,

            count: None,
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    assert_eq!(node0.metrics_port, 6001);
    Ok(())
}

#[tokio::test]
async fn add_node_should_set_max_archived_log_files() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
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
        .returning(|| Ok(6001))
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
                    OsString::from("6001"),
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
            alpha: false,
            auto_restart: false,

            count: Some(1),
            delete_antnode_src: false,
            env_variables: None,
            log_format: None,
            max_archived_log_files: Some(20),
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    assert_matches!(node0.max_archived_log_files, Some(20));

    Ok(())
}

#[tokio::test]
async fn add_node_should_set_max_log_files() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();
    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());

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
        .returning(|| Ok(6001))
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
                    OsString::from("6001"),
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
            alpha: false,
            auto_restart: false,

            count: Some(1),
            delete_antnode_src: false,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: Some(20),
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    assert_matches!(node0.max_log_files, Some(20));

    Ok(())
}

#[tokio::test]
async fn add_node_should_use_a_custom_port_range_for_metrics_server() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
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
                    OsString::from("6001"),
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
                    OsString::from("6002"),
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
                    OsString::from("6003"),
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
            alpha: false,
            auto_restart: false,

            count: Some(3),
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: Some(PortRange::Range(6001, 6003)),
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    assert_eq!(node_registry.nodes.read().await.len(), 3);
    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    let node1 = node_registry.nodes.read().await[1].read().await.clone();
    let node2 = node_registry.nodes.read().await[2].read().await.clone();

    assert_eq!(node0.metrics_port, 6001);
    assert_eq!(node1.metrics_port, 6002);
    assert_eq!(node2.metrics_port, 6003);

    Ok(())
}

#[tokio::test]
async fn add_node_should_return_an_error_if_duplicate_custom_metrics_port_is_used() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
    node_registry
        .push_node(NodeServiceData {
            alpha: false,
            auto_restart: false,
            connected_peers: 10,
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
            initial_peers_config: Default::default(),
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: 12000,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: None,
            pid: None,
            skip_reachability_check: false,
            reachability_progress: ReachabilityProgress::NotRun,
            relay: false,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            rpc_socket_addr: Some(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                8081,
            )),
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Added,
            no_upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
            write_older_cache_files: false,
        })
        .await;
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
            alpha: false,
            auto_restart: false,

            count: None,
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: Some(PortRange::Single(12000)),
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry,
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
async fn add_node_should_return_an_error_if_duplicate_custom_metrics_port_in_range_is_used()
-> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
    node_registry
        .push_node(NodeServiceData {
            alpha: false,
            auto_restart: false,
            connected_peers: 10,
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
            initial_peers_config: Default::default(),
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: 12000,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: None,
            pid: None,
            skip_reachability_check: false,
            reachability_progress: ReachabilityProgress::NotRun,
            relay: false,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            rpc_socket_addr: Some(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                8081,
            )),
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Added,
            no_upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
            write_older_cache_files: false,
        })
        .await;
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
            alpha: false,
            auto_restart: false,

            count: Some(3),
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: Some(PortRange::Range(12000, 12002)),
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry,
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
async fn add_node_should_use_a_custom_port_range_for_the_rpc_server() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());

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
        .returning(|| Ok(6001))
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
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8081"),
                    OsString::from("--metrics-server-port"),
                    OsString::from("6001"),
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
        .returning(|| Ok(6002))
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
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8082"),
                    OsString::from("--metrics-server-port"),
                    OsString::from("6002"),
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
        .returning(|| Ok(6003))
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
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8083"),
                    OsString::from("--metrics-server-port"),
                    OsString::from("6003"),
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
            alpha: false,
            auto_restart: false,

            count: Some(3),
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: Some(PortRange::Range(8081, 8083)),
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());
    assert_eq!(node_registry.nodes.read().await.len(), 3);
    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    let node1 = node_registry.nodes.read().await[1].read().await.clone();
    let node2 = node_registry.nodes.read().await[2].read().await.clone();
    assert_eq!(
        node0.rpc_socket_addr,
        Some(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            8081
        ))
    );
    assert_eq!(
        node1.rpc_socket_addr,
        Some(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            8082
        ))
    );
    assert_eq!(
        node2.rpc_socket_addr,
        Some(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            8083
        ))
    );
    Ok(())
}

#[tokio::test]
async fn add_node_should_return_an_error_if_duplicate_custom_rpc_port_is_used() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
    node_registry
        .push_node(NodeServiceData {
            alpha: false,
            auto_restart: false,
            connected_peers: 10,
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
            initial_peers_config: Default::default(),
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: 6001,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: None,
            pid: None,
            skip_reachability_check: false,
            reachability_progress: ReachabilityProgress::NotRun,
            relay: false,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            rpc_socket_addr: Some(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                8081,
            )),
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Added,
            no_upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
            write_older_cache_files: false,
        })
        .await;
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
            alpha: false,
            auto_restart: false,

            count: None,
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: Some(PortRange::Single(8081)),
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
            write_older_cache_files: false,
        },
        node_registry,
        &MockServiceControl::new(),
        VerbosityLevel::Normal,
    )
    .await;

    match result {
        Ok(_) => panic!("This test is supposed to result in a failure"),
        Err(e) => {
            assert_eq!(e.to_string(), "Port 8081 is being used by another service");
            Ok(())
        }
    }
}

#[tokio::test]
async fn add_node_should_return_an_error_if_duplicate_custom_rpc_port_in_range_is_used()
-> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
    node_registry
        .push_node(NodeServiceData {
            alpha: false,
            auto_restart: false,
            connected_peers: 10,
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
            initial_peers_config: Default::default(),
            listen_addr: None,
            log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: 6001,
            network_id: None,
            node_ip: None,
            node_port: None,
            number: 1,
            peer_id: None,
            pid: None,
            skip_reachability_check: false,
            reachability_progress: ReachabilityProgress::NotRun,
            relay: false,
            rewards_address: RewardsAddress::from_str(
                "0x03B770D9cD32077cC0bF330c13C114a87643B124",
            )?,
            rpc_socket_addr: Some(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                8081,
            )),
            antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
            schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
            service_name: "antnode1".to_string(),
            status: ServiceStatus::Added,
            no_upnp: false,
            user: Some("ant".to_string()),
            user_mode: false,
            version: "0.98.1".to_string(),
            write_older_cache_files: false,
        })
        .await;
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
            alpha: false,
            auto_restart: false,

            count: Some(2),
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: Some(PortRange::Range(8081, 8082)),
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &MockServiceControl::new(),
        VerbosityLevel::Normal,
    )
    .await;

    match result {
        Ok(_) => panic!("This test is supposed to result in a failure"),
        Err(e) => {
            assert_eq!(e.to_string(), "Port 8081 is being used by another service");
            Ok(())
        }
    }
}

#[tokio::test]
async fn add_node_should_not_delete_the_source_binary_if_path_arg_is_used() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());

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
        .returning(|| Ok(6001))
        .in_sequence(&mut seq);

    let install_ctx = InstallNodeServiceCtxBuilder {
        alpha: false,
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
        log_dir_path: node_logs_dir.to_path_buf().join("antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: 6001,
        network_id: None,
        name: "antnode1".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        skip_reachability_check: false,
        relay: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        rpc_socket_addr: None,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        no_upnp: false,
        write_older_cache_files: false,
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
            alpha: false,
            auto_restart: false,

            count: Some(1),
            delete_antnode_src: false,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::is_file());

    Ok(())
}

#[tokio::test]
async fn add_node_should_apply_the_relay_flag_if_it_is_used() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());

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
        .returning(|| Ok(6001))
        .in_sequence(&mut seq);

    let install_ctx = InstallNodeServiceCtxBuilder {
        alpha: false,
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
        log_dir_path: node_logs_dir.to_path_buf().join("antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: 6001,
        network_id: None,
        name: "antnode1".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        skip_reachability_check: false,
        relay: true,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        rpc_socket_addr: None,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        no_upnp: false,
        write_older_cache_files: false,
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
            alpha: false,
            auto_restart: false,

            count: Some(1),
            delete_antnode_src: false,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: true,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    assert!(node0.relay);

    Ok(())
}

#[tokio::test]
async fn add_node_should_add_the_node_in_user_mode() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());

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
        .returning(|| Ok(6001))
        .in_sequence(&mut seq);

    let install_ctx = InstallNodeServiceCtxBuilder {
        alpha: false,
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
        log_dir_path: node_logs_dir.to_path_buf().join("antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: 6001,
        network_id: None,
        name: "antnode1".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        skip_reachability_check: false,
        relay: true,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        rpc_socket_addr: None,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        no_upnp: false,
        write_older_cache_files: false,
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
            alpha: false,
            auto_restart: false,

            count: Some(1),
            delete_antnode_src: false,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: true,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn add_node_should_add_the_node_with_no_upnp_flag() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());

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
        .returning(|| Ok(6001))
        .in_sequence(&mut seq);

    let install_ctx = InstallNodeServiceCtxBuilder {
        alpha: false,
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
        log_dir_path: node_logs_dir.to_path_buf().join("antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: 6001,
        network_id: None,
        name: "antnode1".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        skip_reachability_check: false,
        relay: true,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        rpc_socket_addr: None,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        no_upnp: true,
        write_older_cache_files: false,
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
            alpha: false,
            auto_restart: false,

            count: Some(1),
            delete_antnode_src: false,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: true,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    assert_eq!(node_registry.nodes.read().await.len(), 1);
    assert!(node0.no_upnp);

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

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());

    let mut mock_service_control = MockServiceControl::new();
    let mut seq = Sequence::new();
    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(6001))
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
                    OsString::from("6001"),
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
            alpha: false,
            auto_restart: true,
            count: None,
            delete_antnode_src: true,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: false,
            relay: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    assert!(node0.auto_restart);

    Ok(())
}

#[tokio::test]
async fn add_node_should_add_the_node_with_write_older_cache_files() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());

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
        .returning(|| Ok(6001))
        .in_sequence(&mut seq);

    let install_ctx = InstallNodeServiceCtxBuilder {
        alpha: false,
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
        relay: false,
        log_dir_path: node_logs_dir.to_path_buf().join("antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: 6001,
        network_id: None,
        name: "antnode1".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        rpc_socket_addr: None,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        no_upnp: true,
        skip_reachability_check: false,
        write_older_cache_files: true,
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
            alpha: false,
            auto_restart: false,

            count: Some(1),
            delete_antnode_src: false,
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
            rpc_address: None,
            rpc_port: None,
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
            skip_reachability_check: false,
            write_older_cache_files: true,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    assert_eq!(node_registry.nodes.read().await.len(), 1);
    assert!(node0.write_older_cache_files);

    Ok(())
}

#[tokio::test]
async fn add_node_should_create_service_file_with_alpha_arg() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());
    let latest_version = "0.96.4";
    let temp_dir = assert_fs::TempDir::new()?;
    let node_data_dir = temp_dir.child("data");
    node_data_dir.create_dir_all()?;
    let node_logs_dir = temp_dir.child("logs");
    node_logs_dir.create_dir_all()?;
    let antnode_download_path = temp_dir.child(ANTNODE_FILE_NAME);
    antnode_download_path.write_binary(b"fake antnode bin")?;

    let init_peers_config = InitialPeersConfig {
        first: false,
        addrs: vec![],
        network_contacts_url: vec![],
        local: false,
        ignore_cache: false,
        bootstrap_cache_dir: None,
    };

    let mut seq = Sequence::new();

    mock_service_control
        .expect_get_available_port()
        .times(1)
        .returning(|| Ok(6001))
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
                    OsString::from("--alpha"),
                    OsString::from("--metrics-server-port"),
                    OsString::from("6001"),
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
            alpha: true,
            auto_restart: false,

            count: None,
            delete_antnode_src: true,
            env_variables: None,
            relay: false,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: init_peers_config.clone(),
            skip_reachability_check: false,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    antnode_download_path.assert(predicate::path::missing());
    node_data_dir.assert(predicate::path::is_dir());
    node_logs_dir.assert(predicate::path::is_dir());
    assert_eq!(node_registry.nodes.read().await.len(), 1);
    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    assert_eq!(node0.version, latest_version);
    assert_eq!(node0.initial_peers_config, init_peers_config);
    assert!(node0.alpha);

    Ok(())
}

#[tokio::test]
async fn add_node_should_add_the_node_with_skip_reachability_check_flag() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());

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
        .returning(|| Ok(6001))
        .in_sequence(&mut seq);

    let install_ctx = InstallNodeServiceCtxBuilder {
        alpha: false,
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
        log_dir_path: node_logs_dir.to_path_buf().join("antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: 6001,
        network_id: None,
        name: "antnode1".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        skip_reachability_check: true,
        relay: true,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        rpc_socket_addr: None,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        no_upnp: true,
        write_older_cache_files: false,
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
            alpha: false,
            auto_restart: false,
            count: Some(1),
            delete_antnode_src: false,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            skip_reachability_check: true,
            relay: true,
            rpc_address: None,
            rpc_port: None,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    assert_eq!(node_registry.nodes.read().await.len(), 1);
    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    assert!(node0.skip_reachability_check);

    Ok(())
}

#[tokio::test]
async fn add_node_should_set_reachability_check_progress_to_none() -> Result<()> {
    let tmp_data_dir = assert_fs::TempDir::new()?;
    let node_reg_path = tmp_data_dir.child("node_reg.json");

    let mut mock_service_control = MockServiceControl::new();

    let node_registry = NodeRegistryManager::empty(node_reg_path.to_path_buf());

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
        .returning(|| Ok(6001))
        .in_sequence(&mut seq);

    let install_ctx = InstallNodeServiceCtxBuilder {
        alpha: false,
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
        log_dir_path: node_logs_dir.to_path_buf().join("antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: 6001,
        network_id: None,
        name: "antnode1".to_string(),
        node_ip: None,
        node_port: None,
        init_peers_config: InitialPeersConfig::default(),
        skip_reachability_check: false,
        relay: true,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        rpc_socket_addr: None,
        antnode_path: node_data_dir
            .to_path_buf()
            .join("antnode1")
            .join(ANTNODE_FILE_NAME),
        service_user: Some(get_username()),
        no_upnp: true,
        write_older_cache_files: false,
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
            alpha: false,
            auto_restart: false,
            count: Some(1),
            delete_antnode_src: false,
            env_variables: None,
            log_format: None,
            max_archived_log_files: None,
            max_log_files: None,
            metrics_port: None,
            network_id: None,
            node_ip: None,
            node_port: None,
            init_peers_config: InitialPeersConfig::default(),
            relay: true,
            rpc_address: None,
            rpc_port: None,
            antnode_dir_path: temp_dir.to_path_buf(),
            antnode_src_path: antnode_download_path.to_path_buf(),
            service_data_dir_path: node_data_dir.to_path_buf(),
            service_log_dir_path: node_logs_dir.to_path_buf(),
            skip_reachability_check: false,
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
            write_older_cache_files: false,
        },
        node_registry.clone(),
        &mock_service_control,
        VerbosityLevel::Normal,
    )
    .await?;

    assert_eq!(node_registry.nodes.read().await.len(), 1);
    let node0 = node_registry.nodes.read().await[0].read().await.clone();
    assert_eq!(node0.reachability_progress, ReachabilityProgress::NotRun);

    Ok(())
}
