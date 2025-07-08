// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::*;
use ant_bootstrap::InitialPeersConfig;
use ant_evm::{AttoTokens, CustomNetwork, EvmNetwork, RewardsAddress};
use ant_logging::LogFormat;
use ant_service_management::{
    UpgradeOptions, UpgradeResult,
    error::{Error as ServiceControlError, Result as ServiceControlResult},
    metric::{MetricsAction, NodeMetrics},
    node::{NODE_SERVICE_DATA_SCHEMA_LATEST, NodeService, NodeServiceData},
    rpc::{NetworkInfo, NodeInfo, RecordAddress, RpcActions},
};
use assert_fs::prelude::*;
use assert_matches::assert_matches;
use async_trait::async_trait;
use color_eyre::eyre::Result;
use libp2p_identity::PeerId;
use mockall::{mock, predicate::*};
use predicates::prelude::*;
use service_manager::ServiceInstallCtx;
use std::{
    ffi::OsString,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};
use tokio::sync::RwLock;

mock! {
    pub RpcClient {}
    #[async_trait]
    impl RpcActions for RpcClient {
        async fn node_info(&self) -> ServiceControlResult<NodeInfo>;
        async fn network_info(&self) -> ServiceControlResult<NetworkInfo>;
        async fn record_addresses(&self) -> ServiceControlResult<Vec<RecordAddress>>;
        async fn node_restart(&self, delay_millis: u64, retain_peer_id: bool) -> ServiceControlResult<()>;
        async fn node_stop(&self, delay_millis: u64) -> ServiceControlResult<()>;
        async fn node_update(&self, delay_millis: u64) -> ServiceControlResult<()>;
        async fn wait_until_node_connects_to_network(&self, timeout: Option<std::time::Duration>) -> ServiceControlResult<()>;
        async fn update_log_level(&self, log_levels: String) -> ServiceControlResult<()>;
    }
}

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

mock! {
    pub MetricsClient {}
    #[async_trait]
    impl MetricsAction for MetricsClient {
        async fn get_node_metrics(&self) -> Result<NodeMetrics, ant_service_management::Error>;
        async fn wait_until_reachability_check_completes(&self, timeout: Option<std::time::Duration>) -> Result<(), ant_service_management::Error>;
    }
}

#[tokio::test]
async fn start_should_start_a_newly_installed_service() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();
    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| Ok(1000));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 1000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: "0.98.1".to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: vec![PeerId::from_str(
                    "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                )?],
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
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
        relay: false,
        initial_peers_config: InitialPeersConfig::default(),
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
        pid: None,
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Added,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: "0.98.1".to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager.start().await?;

    let service_data = service_data.read().await;
    assert_eq!(
        service_data.connected_peers,
        Some(vec![PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?,])
    );
    assert_eq!(service_data.pid, Some(1000));
    assert_eq!(
        service_data.peer_id,
        Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR"
        )?)
    );
    assert_matches!(service_data.status, ServiceStatus::Running);

    Ok(())
}

#[tokio::test]
async fn start_should_start_a_stopped_service() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();
    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| Ok(1000));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 1000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: "0.98.1".to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
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
        relay: false,
        initial_peers_config: InitialPeersConfig::default(),
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: None,
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Stopped,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: "0.98.1".to_string(),
        write_older_cache_files: false,
    };

    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager.start().await?;

    let service_data = service_data.read().await;
    assert_eq!(service_data.pid, Some(1000));
    assert_eq!(
        service_data.peer_id,
        Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR"
        )?)
    );
    assert_matches!(service_data.status, ServiceStatus::Running);

    Ok(())
}

#[tokio::test]
async fn start_should_not_attempt_to_start_a_running_service() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();
    let mock_rpc_client = MockRpcClient::new();
    let mock_metrics_client = MockMetricsClient::new();

    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| Ok(100));

    let service_data = NodeServiceData {
        alpha: false,
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
        relay: false,
        initial_peers_config: InitialPeersConfig::default(),
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: "0.98.1".to_string(),
        write_older_cache_files: false,
    };

    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager.start().await?;

    let service_data = service_data.read().await;
    assert_eq!(service_data.pid, Some(1000));
    assert_eq!(
        service_data.peer_id,
        Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR"
        )?)
    );
    assert_matches!(service_data.status, ServiceStatus::Running);

    Ok(())
}

#[tokio::test]
async fn start_should_start_a_service_marked_as_running_but_had_since_stopped() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();
    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| {
            Err(ServiceError::ServiceProcessNotFound(
                "Could not find process at '/var/antctl/services/antnode1/antnode'".to_string(),
            ))
        });
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| Ok(1000));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 1000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: "0.98.1".to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
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
        relay: false,
        initial_peers_config: InitialPeersConfig::default(),
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: "0.98.1".to_string(),
        write_older_cache_files: false,
    };

    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager.start().await?;

    let service_data = service_data.read().await;
    assert_eq!(service_data.pid, Some(1000));
    assert_eq!(
        service_data.peer_id,
        Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR"
        )?)
    );
    assert_matches!(service_data.status, ServiceStatus::Running);

    Ok(())
}

#[tokio::test]
async fn start_should_return_an_error_if_the_process_was_not_found() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();
    let mock_rpc_client = MockRpcClient::new();
    let mock_metrics_client = MockMetricsClient::new();

    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| {
            Err(ServiceControlError::ServiceProcessNotFound(
                "/var/antctl/services/antnode1/antnode".to_string(),
            ))
        });

    let service_data = NodeServiceData {
        alpha: false,
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
        relay: false,
        initial_peers_config: InitialPeersConfig::default(),
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
        pid: None,
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Added,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: "0.98.1".to_string(),
        write_older_cache_files: false,
    };

    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );
    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    let result = service_manager.start().await;
    match result {
        Ok(_) => panic!("This test should have resulted in an error"),
        Err(e) => assert_eq!(
            "The PID of the process was not found after starting it.",
            e.to_string()
        ),
    }

    Ok(())
}

#[tokio::test]
async fn start_should_start_a_user_mode_service() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();
    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(true))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| Ok(100));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 1000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: "0.98.1".to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
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
        relay: false,
        initial_peers_config: InitialPeersConfig::default(),
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
        pid: None,
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Added,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: true,
        version: "0.98.1".to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager.start().await?;

    Ok(())
}

#[tokio::test]
async fn stop_should_stop_a_running_service() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();
    let mock_metrics_client = MockMetricsClient::new();

    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| Ok(100));

    let service_data = NodeServiceData {
        alpha: false,
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
        relay: false,
        initial_peers_config: InitialPeersConfig::default(),
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: "0.98.1".to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(MockRpcClient::new()),
        Box::new(mock_metrics_client),
    );
    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager.stop().await?;

    let service_data = service_data.read().await;
    assert_eq!(service_data.pid, None);
    assert_eq!(service_data.connected_peers, None);
    assert_matches!(service_data.status, ServiceStatus::Stopped);
    Ok(())
}

#[tokio::test]
async fn stop_should_not_return_error_for_attempt_to_stop_installed_service() -> Result<()> {
    let mock_metrics_client = MockMetricsClient::new();
    let service_data = NodeServiceData {
        alpha: false,
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
        relay: false,
        initial_peers_config: InitialPeersConfig::default(),
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
        pid: None,
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Added,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: "0.98.1".to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(MockRpcClient::new()),
        Box::new(mock_metrics_client),
    );
    let mut service_manager = ServiceManager::new(
        service,
        Box::new(MockServiceControl::new()),
        VerbosityLevel::Normal,
    );

    let result = service_manager.stop().await;

    match result {
        Ok(()) => Ok(()),
        Err(_) => {
            panic!("The stop command should be idempotent and do nothing for an added service");
        }
    }
}

#[tokio::test]
async fn stop_should_return_ok_when_attempting_to_stop_service_that_was_already_stopped()
-> Result<()> {
    let service_data = NodeServiceData {
        alpha: false,
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
        relay: false,
        initial_peers_config: InitialPeersConfig::default(),
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: None,
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Stopped,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: "0.98.1".to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(MockRpcClient::new()),
        Box::new(MockMetricsClient::new()),
    );
    let mut service_manager = ServiceManager::new(
        service,
        Box::new(MockServiceControl::new()),
        VerbosityLevel::Normal,
    );

    let result = service_manager.stop().await;

    match result {
        Ok(()) => Ok(()),
        Err(_) => {
            panic!("The stop command should be idempotent and do nothing for an stopped service");
        }
    }
}

#[tokio::test]
async fn stop_should_return_ok_when_attempting_to_stop_a_removed_service() -> Result<()> {
    let service_data = NodeServiceData {
        alpha: false,
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
        relay: false,
        initial_peers_config: InitialPeersConfig::default(),
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
        pid: None,
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Removed,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: "0.98.1".to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(MockRpcClient::new()),
        Box::new(MockMetricsClient::new()),
    );
    let mut service_manager = ServiceManager::new(
        service,
        Box::new(MockServiceControl::new()),
        VerbosityLevel::Normal,
    );

    let result = service_manager.stop().await;

    match result {
        Ok(()) => Ok(()),
        Err(_) => {
            panic!("The stop command should be idempotent and do nothing for a removed service");
        }
    }
}

#[tokio::test]
async fn stop_should_stop_a_user_mode_service() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(true))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| Ok(100));

    let service_data = NodeServiceData {
        alpha: false,
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
        relay: false,
        listen_addr: None,
        initial_peers_config: InitialPeersConfig::default(),
        log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: None,
        network_id: None,
        node_ip: None,
        node_port: None,
        number: 1,
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: None,
        user_mode: true,
        version: "0.98.1".to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(MockRpcClient::new()),
        Box::new(MockMetricsClient::new()),
    );
    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager.stop().await?;

    let service_data = service_data.read().await;
    assert_eq!(service_data.pid, None);
    assert_eq!(service_data.connected_peers, None);
    assert_matches!(service_data.status, ServiceStatus::Stopped);
    Ok(())
}

#[tokio::test]
async fn upgrade_should_upgrade_a_service_to_a_new_version() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(always(), always())
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(2000));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
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
        relay: false,
        initial_peers_config: InitialPeersConfig::default(),
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );
    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    let upgrade_result = service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    match upgrade_result {
        UpgradeResult::Upgraded(old_version, new_version) => {
            assert_eq!(old_version, current_version);
            assert_eq!(new_version, target_version);
        }
        _ => panic!("Expected UpgradeResult::Upgraded but was {upgrade_result:#?}"),
    }

    let service_data = service_data.read().await;
    assert_eq!(service_data.pid, Some(2000));
    assert_eq!(
        service_data.peer_id,
        Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?)
    );
    assert_eq!(service_data.version, target_version);

    Ok(())
}

#[tokio::test]
async fn upgrade_should_not_be_required_if_target_is_less_than_current_version() -> Result<()> {
    let current_version = "0.2.0";
    let target_version = "0.1.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mock_service_control = MockServiceControl::new();
    let mock_rpc_client = MockRpcClient::new();
    let mock_metrics_client = MockMetricsClient::new();

    let service_data = NodeServiceData {
        alpha: false,
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
        relay: false,
        initial_peers_config: InitialPeersConfig::default(),
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    let upgrade_result = service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    assert_matches!(upgrade_result, UpgradeResult::NotRequired);

    Ok(())
}

#[tokio::test]
async fn upgrade_should_downgrade_to_a_previous_version_if_force_is_used() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(always(), always())
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(2000));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
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
        relay: false,
        initial_peers_config: InitialPeersConfig::default(),
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    let upgrade_result = service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: true,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    match upgrade_result {
        UpgradeResult::Forced(old_version, new_version) => {
            assert_eq!(old_version, current_version);
            assert_eq!(new_version, target_version);
        }
        _ => panic!("Expected UpgradeResult::Forced but was {upgrade_result:#?}"),
    }

    let service_data = service_data.read().await;
    assert_eq!(service_data.pid, Some(2000));
    assert_eq!(
        service_data.peer_id,
        Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?)
    );
    assert_eq!(service_data.version, target_version);

    Ok(())
}

#[tokio::test]
async fn upgrade_should_upgrade_and_not_start_the_service() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(always(), always())
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(0)
        .returning(|_| Ok(()));
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(0)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(0)
        .returning(|_| ());
    mock_rpc_client.expect_node_info().times(0).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(0)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
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
        relay: false,
        initial_peers_config: InitialPeersConfig::default(),
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    let upgrade_result = service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: false,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    match upgrade_result {
        UpgradeResult::Upgraded(old_version, new_version) => {
            assert_eq!(old_version, current_version);
            assert_eq!(new_version, target_version);
        }
        _ => panic!("Expected UpgradeResult::Upgraded but was {upgrade_result:#?}"),
    }

    let service_data = service_data.read().await;
    assert_eq!(service_data.pid, None);
    assert_eq!(
        service_data.peer_id,
        Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?)
    );
    assert_eq!(service_data.version, target_version);
    assert_matches!(service_data.status, ServiceStatus::Stopped);

    Ok(())
}

#[tokio::test]
async fn upgrade_should_return_upgraded_but_not_started_if_service_did_not_start() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let current_node_bin_str = current_node_bin.to_path_buf().to_string_lossy().to_string();

    let mut mock_service_control = MockServiceControl::new();
    let mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(always(), always())
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(move |_| {
            Err(ServiceControlError::ServiceProcessNotFound(
                current_node_bin_str.clone(),
            ))
        });
    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(0)
        .returning(|_| Ok(()));

    let service_data = NodeServiceData {
        alpha: false,
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
        relay: false,
        initial_peers_config: InitialPeersConfig::default(),
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );
    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    let upgrade_result = service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    match upgrade_result {
        UpgradeResult::UpgradedButNotStarted(old_version, new_version, _) => {
            assert_eq!(old_version, current_version);
            assert_eq!(new_version, target_version);
        }
        _ => {
            panic!("Expected UpgradeResult::UpgradedButNotStarted but was {upgrade_result:#?}")
        }
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_should_upgrade_a_service_in_user_mode() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(true))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(true))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(always(), eq(true))
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(true))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(2000));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
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
        relay: false,
        initial_peers_config: InitialPeersConfig::default(),
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: None,
        user_mode: true,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    let upgrade_result = service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    match upgrade_result {
        UpgradeResult::Upgraded(old_version, new_version) => {
            assert_eq!(old_version, current_version);
            assert_eq!(new_version, target_version);
        }
        _ => panic!("Expected UpgradeResult::Upgraded but was {upgrade_result:#?}"),
    }

    let service_data = service_data.read().await;
    assert_eq!(service_data.pid, Some(2000));
    assert_eq!(
        service_data.peer_id,
        Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?)
    );
    assert_eq!(service_data.version, target_version);

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_the_first_flag() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8081"),
                    OsString::from("--root-dir"),
                    OsString::from("/var/antctl/services/antnode1"),
                    OsString::from("--log-output-dest"),
                    OsString::from("/var/log/antnode/antnode1"),
                    OsString::from("--first"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-arbitrum-one"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: current_node_bin.to_path_buf(),
                username: Some("ant".to_string()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(100));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: false,
        connected_peers: None,
        data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
        evm_network: EvmNetwork::ArbitrumOne,
        relay: false,
        initial_peers_config: InitialPeersConfig {
            first: true,
            addrs: vec![],
            network_contacts_url: vec![],
            local: false,
            ignore_cache: false,
            bootstrap_cache_dir: None,
        },
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    let service_data = service_data.read().await;
    assert!(service_data.initial_peers_config.first);

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_the_peers_arg() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--rpc"),
                        OsString::from("127.0.0.1:8081"),
                        OsString::from("--root-dir"),
                        OsString::from("/var/antctl/services/antnode1"),
                        OsString::from("--log-output-dest"),
                        OsString::from("/var/log/antnode/antnode1"),
                        OsString::from("--peer"),
                        OsString::from(
                            "/ip4/127.0.0.1/tcp/8080/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
                        ),
                        OsString::from("--rewards-address"),
                        OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                        OsString::from("evm-arbitrum-one"),
                    ],
                    autostart: false,
                    contents: None,
                    environment: None,
                    label: "antnode1".parse()?,
                    program: current_node_bin.to_path_buf(),
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(100));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: false,
        connected_peers: None,
        data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
        evm_network: EvmNetwork::ArbitrumOne,
        relay: false,
        initial_peers_config: InitialPeersConfig {
            first: false,
            addrs: vec![
                "/ip4/127.0.0.1/tcp/8080/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
                    .parse()?,
            ],
            network_contacts_url: vec![],
            local: false,
            ignore_cache: false,
            bootstrap_cache_dir: None,
        },
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    let service_data = service_data.read().await;
    assert!(!service_data.initial_peers_config.addrs.is_empty());

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_the_network_id_arg() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8081"),
                    OsString::from("--root-dir"),
                    OsString::from("/var/antctl/services/antnode1"),
                    OsString::from("--log-output-dest"),
                    OsString::from("/var/log/antnode/antnode1"),
                    OsString::from("--network-id"),
                    OsString::from("5"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-arbitrum-one"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: current_node_bin.to_path_buf(),
                username: Some("ant".to_string()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(100));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: false,
        connected_peers: None,
        data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
        evm_network: EvmNetwork::ArbitrumOne,
        relay: false,
        initial_peers_config: Default::default(),
        listen_addr: None,
        log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: None,
        network_id: Some(5),
        node_ip: None,
        node_port: None,
        number: 1,
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    let service_data = service_data.read().await;
    assert_eq!(service_data.network_id, Some(5));

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_the_local_flag() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8081"),
                    OsString::from("--root-dir"),
                    OsString::from("/var/antctl/services/antnode1"),
                    OsString::from("--log-output-dest"),
                    OsString::from("/var/log/antnode/antnode1"),
                    OsString::from("--local"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-arbitrum-one"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: current_node_bin.to_path_buf(),
                username: Some("ant".to_string()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(100));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: false,
        connected_peers: None,
        data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
        evm_network: EvmNetwork::ArbitrumOne,
        relay: false,
        initial_peers_config: InitialPeersConfig {
            first: false,
            addrs: vec![],
            network_contacts_url: vec![],
            local: true,
            ignore_cache: false,
            bootstrap_cache_dir: None,
        },
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    let service_data = service_data.read().await;
    assert!(service_data.initial_peers_config.local);

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_the_network_contacts_url_arg() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8081"),
                    OsString::from("--root-dir"),
                    OsString::from("/var/antctl/services/antnode1"),
                    OsString::from("--log-output-dest"),
                    OsString::from("/var/log/antnode/antnode1"),
                    OsString::from("--network-contacts-url"),
                    OsString::from(
                        "http://localhost:8080/contacts.json,http://localhost:8081/contacts.json",
                    ),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-arbitrum-one"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: current_node_bin.to_path_buf(),
                username: Some("ant".to_string()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(100));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: false,
        connected_peers: None,
        data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
        evm_network: EvmNetwork::ArbitrumOne,
        relay: false,
        initial_peers_config: InitialPeersConfig {
            first: false,
            addrs: vec![],
            network_contacts_url: vec![
                "http://localhost:8080/contacts.json".to_string(),
                "http://localhost:8081/contacts.json".to_string(),
            ],
            local: false,
            ignore_cache: false,
            bootstrap_cache_dir: None,
        },
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    let service_data = service_data.read().await;
    assert_eq!(
        service_data.initial_peers_config.network_contacts_url.len(),
        2
    );

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_the_ignore_cache_flag() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8081"),
                    OsString::from("--root-dir"),
                    OsString::from("/var/antctl/services/antnode1"),
                    OsString::from("--log-output-dest"),
                    OsString::from("/var/log/antnode/antnode1"),
                    OsString::from("--ignore-cache"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-arbitrum-one"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: current_node_bin.to_path_buf(),
                username: Some("ant".to_string()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(100));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: false,
        connected_peers: None,
        data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
        evm_network: EvmNetwork::ArbitrumOne,
        relay: false,
        initial_peers_config: InitialPeersConfig {
            first: false,
            addrs: vec![],
            network_contacts_url: vec![],
            local: false,
            ignore_cache: true,
            bootstrap_cache_dir: None,
        },
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    let service_data = service_data.read().await;
    assert!(service_data.initial_peers_config.ignore_cache);

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_the_custom_bootstrap_cache_path() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8081"),
                    OsString::from("--root-dir"),
                    OsString::from("/var/antctl/services/antnode1"),
                    OsString::from("--log-output-dest"),
                    OsString::from("/var/log/antnode/antnode1"),
                    OsString::from("--bootstrap-cache-dir"),
                    OsString::from("/var/antctl/services/antnode1/bootstrap_cache"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-arbitrum-one"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: current_node_bin.to_path_buf(),
                username: Some("ant".to_string()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(100));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: false,
        connected_peers: None,
        data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
        evm_network: EvmNetwork::ArbitrumOne,
        relay: false,
        initial_peers_config: InitialPeersConfig {
            first: false,
            addrs: vec![],
            network_contacts_url: vec![],
            local: false,
            ignore_cache: false,
            bootstrap_cache_dir: Some(PathBuf::from(
                "/var/antctl/services/antnode1/bootstrap_cache",
            )),
        },
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    let service_data = service_data.read().await;
    assert_eq!(
        service_data.initial_peers_config.bootstrap_cache_dir,
        Some(PathBuf::from(
            "/var/antctl/services/antnode1/bootstrap_cache"
        ))
    );

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_the_no_upnp_flag() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8081"),
                    OsString::from("--root-dir"),
                    OsString::from("/var/antctl/services/antnode1"),
                    OsString::from("--log-output-dest"),
                    OsString::from("/var/log/antnode/antnode1"),
                    OsString::from("--no-upnp"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-arbitrum-one"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: current_node_bin.to_path_buf(),
                username: Some("ant".to_string()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(100));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: false,
        connected_peers: None,
        data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
        evm_network: EvmNetwork::ArbitrumOne,
        relay: false,
        initial_peers_config: Default::default(),
        listen_addr: None,
        log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: None,
        network_id: None,
        node_ip: None,
        node_port: None,
        no_upnp: true,
        number: 1,
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    let service_data = service_data.read().await;
    assert!(service_data.no_upnp);

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_the_log_format_flag() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8081"),
                    OsString::from("--root-dir"),
                    OsString::from("/var/antctl/services/antnode1"),
                    OsString::from("--log-output-dest"),
                    OsString::from("/var/log/antnode/antnode1"),
                    OsString::from("--log-format"),
                    OsString::from("json"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-arbitrum-one"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: current_node_bin.to_path_buf(),
                username: Some("ant".to_string()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(100));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: false,
        connected_peers: None,
        data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
        evm_network: EvmNetwork::ArbitrumOne,
        relay: false,
        initial_peers_config: Default::default(),
        listen_addr: None,
        log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
        log_format: Some(LogFormat::Json),
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: None,
        network_id: None,
        node_ip: None,
        node_port: None,
        number: 1,
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    let service_data = service_data.read().await;
    assert!(service_data.log_format.is_some());
    assert_eq!(service_data.log_format, Some(LogFormat::Json));

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_the_relay_flag() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8081"),
                    OsString::from("--root-dir"),
                    OsString::from("/var/antctl/services/antnode1"),
                    OsString::from("--log-output-dest"),
                    OsString::from("/var/log/antnode/antnode1"),
                    OsString::from("--relay"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-arbitrum-one"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: current_node_bin.to_path_buf(),
                username: Some("ant".to_string()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(100));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: false,
        connected_peers: None,
        data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
        evm_network: EvmNetwork::ArbitrumOne,
        relay: true,
        initial_peers_config: Default::default(),
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    let service_data = service_data.read().await;
    assert!(service_data.relay);

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_the_reachability_check_flag() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8081"),
                    OsString::from("--root-dir"),
                    OsString::from("/var/antctl/services/antnode1"),
                    OsString::from("--log-output-dest"),
                    OsString::from("/var/log/antnode/antnode1"),
                    OsString::from("--reachability-check"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-arbitrum-one"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: current_node_bin.to_path_buf(),
                username: Some("ant".to_string()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(100));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: false,
        connected_peers: None,
        data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
        evm_network: EvmNetwork::ArbitrumOne,
        relay: false,
        initial_peers_config: Default::default(),
        listen_addr: None,
        log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: None,
        network_id: None,
        node_ip: None,
        node_port: None,
        no_upnp: false,
        number: 1,
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: true,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    let service_data = service_data.read().await;

    assert!(service_data.reachability_check);

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_custom_node_ip() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8081"),
                    OsString::from("--root-dir"),
                    OsString::from("/var/antctl/services/antnode1"),
                    OsString::from("--log-output-dest"),
                    OsString::from("/var/log/antnode/antnode1"),
                    OsString::from("--ip"),
                    OsString::from("192.168.1.1"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-arbitrum-one"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: current_node_bin.to_path_buf(),
                username: Some("ant".to_string()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(100));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: false,
        connected_peers: None,
        data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
        evm_network: EvmNetwork::ArbitrumOne,
        relay: false,
        initial_peers_config: Default::default(),
        listen_addr: None,
        log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: None,
        network_id: None,
        number: 1,
        node_ip: Some(Ipv4Addr::new(192, 168, 1, 1)),
        node_port: None,
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    let service_data = service_data.read().await;
    assert_eq!(service_data.node_ip, Some(Ipv4Addr::new(192, 168, 1, 1)));

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_custom_node_ports() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8081"),
                    OsString::from("--root-dir"),
                    OsString::from("/var/antctl/services/antnode1"),
                    OsString::from("--log-output-dest"),
                    OsString::from("/var/log/antnode/antnode1"),
                    OsString::from("--port"),
                    OsString::from("12000"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-arbitrum-one"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: current_node_bin.to_path_buf(),
                username: Some("ant".to_string()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(100));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: false,
        connected_peers: None,
        data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
        evm_network: EvmNetwork::ArbitrumOne,
        relay: false,
        initial_peers_config: Default::default(),
        listen_addr: None,
        log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: None,
        network_id: None,
        number: 1,
        node_ip: None,
        node_port: Some(12000),
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    let service_data = service_data.read().await;
    assert_eq!(service_data.node_port, Some(12000));

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_max_archived_log_files() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8081"),
                    OsString::from("--root-dir"),
                    OsString::from("/var/antctl/services/antnode1"),
                    OsString::from("--log-output-dest"),
                    OsString::from("/var/log/antnode/antnode1"),
                    OsString::from("--max-archived-log-files"),
                    OsString::from("20"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-arbitrum-one"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: current_node_bin.to_path_buf(),
                username: Some("ant".to_string()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(100));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: false,
        connected_peers: None,
        data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
        relay: false,
        initial_peers_config: Default::default(),
        listen_addr: None,
        log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
        log_format: None,
        max_archived_log_files: Some(20),
        max_log_files: None,
        metrics_port: None,
        network_id: None,
        node_ip: None,
        node_port: None,
        number: 1,
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        evm_network: EvmNetwork::ArbitrumOne,
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    let service_data = service_data.read().await;
    assert_matches!(service_data.max_archived_log_files, Some(20));

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_max_log_files() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8081"),
                    OsString::from("--root-dir"),
                    OsString::from("/var/antctl/services/antnode1"),
                    OsString::from("--log-output-dest"),
                    OsString::from("/var/log/antnode/antnode1"),
                    OsString::from("--max-log-files"),
                    OsString::from("20"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-arbitrum-one"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: current_node_bin.to_path_buf(),
                username: Some("ant".to_string()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(100));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: false,
        connected_peers: None,
        data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
        relay: false,
        initial_peers_config: Default::default(),
        listen_addr: None,
        log_dir_path: PathBuf::from("/var/log/antnode/antnode1"),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: Some(20),
        metrics_port: None,
        network_id: None,
        node_ip: None,
        node_port: None,
        number: 1,
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        evm_network: EvmNetwork::ArbitrumOne,
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    let service_data = service_data.read().await;
    assert_matches!(service_data.max_log_files, Some(20));

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_custom_metrics_ports() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8081"),
                    OsString::from("--root-dir"),
                    OsString::from("/var/antctl/services/antnode1"),
                    OsString::from("--log-output-dest"),
                    OsString::from("/var/log/antnode/antnode1"),
                    OsString::from("--metrics-server-port"),
                    OsString::from("12000"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-arbitrum-one"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: current_node_bin.to_path_buf(),
                username: Some("ant".to_string()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(100));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: false,
        connected_peers: None,
        data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
        evm_network: EvmNetwork::ArbitrumOne,
        relay: false,
        initial_peers_config: Default::default(),
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    let service_data = service_data.read().await;
    assert_eq!(service_data.metrics_port, Some(12000));

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_custom_rpc_ports() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8081"),
                    OsString::from("--root-dir"),
                    OsString::from("/var/antctl/services/antnode1"),
                    OsString::from("--log-output-dest"),
                    OsString::from("/var/log/antnode/antnode1"),
                    OsString::from("--metrics-server-port"),
                    OsString::from("12000"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-arbitrum-one"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: current_node_bin.to_path_buf(),
                username: Some("ant".to_string()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(100));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: false,
        connected_peers: None,
        data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
        evm_network: EvmNetwork::ArbitrumOne,
        relay: false,
        initial_peers_config: Default::default(),
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    let service_data = service_data.read().await;
    assert_eq!(
        service_data.rpc_socket_addr,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081)
    );

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_auto_restart() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8081"),
                    OsString::from("--root-dir"),
                    OsString::from("/var/antctl/services/antnode1"),
                    OsString::from("--log-output-dest"),
                    OsString::from("/var/log/antnode/antnode1"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-arbitrum-one"),
                ],
                autostart: true,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: current_node_bin.to_path_buf(),
                username: Some("ant".to_string()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(100));
    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: true,
        connected_peers: None,
        data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
        evm_network: EvmNetwork::ArbitrumOne,
        relay: false,
        initial_peers_config: Default::default(),
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager
        .upgrade(UpgradeOptions {
            auto_restart: true,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    let service_data = service_data.read().await;
    assert!(service_data.auto_restart,);

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_evm_network_settings() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8081"),
                    OsString::from("--root-dir"),
                    OsString::from("/var/antctl/services/antnode1"),
                    OsString::from("--log-output-dest"),
                    OsString::from("/var/log/antnode/antnode1"),
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
                program: current_node_bin.to_path_buf(),
                username: Some("ant".to_string()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(100));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: true,
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
        relay: false,
        initial_peers_config: Default::default(),
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),

        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager
        .upgrade(UpgradeOptions {
            auto_restart: true,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    let service_data = service_data.read().await;
    assert!(service_data.auto_restart,);

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_the_rewards_address() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8081"),
                    OsString::from("--root-dir"),
                    OsString::from("/var/antctl/services/antnode1"),
                    OsString::from("--log-output-dest"),
                    OsString::from("/var/log/antnode/antnode1"),
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
                program: current_node_bin.to_path_buf(),
                username: Some("ant".to_string()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(100));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: true,
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
        relay: false,
        initial_peers_config: Default::default(),
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),

        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager
        .upgrade(UpgradeOptions {
            auto_restart: true,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    let service_data = service_data.read().await;
    assert!(service_data.auto_restart,);

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_write_older_cache_files() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8081"),
                    OsString::from("--root-dir"),
                    OsString::from("/var/antctl/services/antnode1"),
                    OsString::from("--log-output-dest"),
                    OsString::from("/var/log/antnode/antnode1"),
                    OsString::from("--metrics-server-port"),
                    OsString::from("12000"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("--write-older-cache-files"),
                    OsString::from("evm-arbitrum-one"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: current_node_bin.to_path_buf(),
                username: Some("ant".to_string()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(100));

    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: false,
        connected_peers: None,
        data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
        evm_network: EvmNetwork::ArbitrumOne,
        relay: false,
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        initial_peers_config: InitialPeersConfig::default(),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: true,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(MockMetricsClient::new()),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    let service_data = service_data.read().await;
    assert!(service_data.write_older_cache_files,);

    Ok(())
}

#[tokio::test]
async fn remove_should_remove_an_added_node() -> Result<()> {
    let temp_dir = assert_fs::TempDir::new()?;
    let log_dir = temp_dir.child("antnode1-logs");
    log_dir.create_dir_all()?;
    let data_dir = temp_dir.child("antnode1-data");
    data_dir.create_dir_all()?;
    let antnode_bin = data_dir.child("antnode");
    antnode_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: false,
        connected_peers: None,
        data_dir_path: data_dir.to_path_buf(),
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
        initial_peers_config: Default::default(),
        listen_addr: None,
        log_dir_path: log_dir.to_path_buf(),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: None,
        network_id: None,
        node_ip: None,
        node_port: None,
        number: 1,
        peer_id: None,
        pid: None,
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: antnode_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        status: ServiceStatus::Stopped,
        service_name: "antnode1".to_string(),
        version: "0.98.1".to_string(),
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(MockRpcClient::new()),
        Box::new(MockMetricsClient::new()),
    );
    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager.remove(false).await?;

    let service_data = service_data.read().await;
    assert_matches!(service_data.status, ServiceStatus::Removed);
    log_dir.assert(predicate::path::missing());
    data_dir.assert(predicate::path::missing());

    Ok(())
}

#[tokio::test]
async fn remove_should_return_an_error_if_attempting_to_remove_a_running_node() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();
    let mock_metrics_client = MockMetricsClient::new();
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| Ok(1000));

    let service_data = NodeServiceData {
        alpha: false,
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
        relay: false,
        initial_peers_config: Default::default(),
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
        pid: Some(1000),
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: "0.98.1".to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(MockRpcClient::new()),
        Box::new(mock_metrics_client),
    );
    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    let result = service_manager.remove(false).await;
    match result {
        Ok(_) => panic!("This test should result in an error"),
        Err(e) => assert_eq!(
            "Unable to remove a running service [\"antnode1\"], stop this service first before removing",
            e.to_string()
        ),
    }

    Ok(())
}

#[tokio::test]
async fn remove_should_return_an_error_for_a_node_that_was_marked_running_but_was_not_actually_running()
-> Result<()> {
    let temp_dir = assert_fs::TempDir::new()?;
    let log_dir = temp_dir.child("antnode1-logs");
    log_dir.create_dir_all()?;
    let data_dir = temp_dir.child("antnode1-data");
    data_dir.create_dir_all()?;
    let antnode_bin = data_dir.child("antnode");
    antnode_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mock_metrics_client = MockMetricsClient::new();
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| {
            Err(ServiceError::ServiceProcessNotFound(
                "Could not find process at '/var/antctl/services/antnode1/antnode'".to_string(),
            ))
        });

    let service_data = NodeServiceData {
        alpha: false,
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
        relay: false,
        initial_peers_config: Default::default(),
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
        pid: Some(1000),
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: PathBuf::from("/var/antctl/services/antnode1/antnode"),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: "0.98.1".to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(MockRpcClient::new()),
        Box::new(mock_metrics_client),
    );
    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    let result = service_manager.remove(false).await;
    match result {
        Ok(_) => panic!("This test should result in an error"),
        Err(e) => assert_eq!(
            "The service status is not as expected. Expected: Running",
            e.to_string()
        ),
    }

    Ok(())
}

#[tokio::test]
async fn remove_should_remove_an_added_node_and_keep_directories() -> Result<()> {
    let temp_dir = assert_fs::TempDir::new()?;
    let log_dir = temp_dir.child("antnode1-logs");
    log_dir.create_dir_all()?;
    let data_dir = temp_dir.child("antnode1-data");
    data_dir.create_dir_all()?;
    let antnode_bin = data_dir.child("antnode");
    antnode_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mock_metrics_client = MockMetricsClient::new();
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: false,
        connected_peers: None,
        data_dir_path: data_dir.to_path_buf(),
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
        initial_peers_config: Default::default(),
        listen_addr: None,
        log_dir_path: log_dir.to_path_buf(),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: None,
        network_id: None,
        node_ip: None,
        node_port: None,
        number: 1,
        pid: None,
        peer_id: None,
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: antnode_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Stopped,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: "0.98.1".to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(MockRpcClient::new()),
        Box::new(mock_metrics_client),
    );
    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager.remove(true).await?;

    let service_data = service_data.read().await;
    assert_matches!(service_data.status, ServiceStatus::Removed);
    log_dir.assert(predicate::path::is_dir());
    data_dir.assert(predicate::path::is_dir());

    Ok(())
}

#[tokio::test]
async fn remove_should_remove_a_user_mode_service() -> Result<()> {
    let temp_dir = assert_fs::TempDir::new()?;
    let log_dir = temp_dir.child("antnode1-logs");
    log_dir.create_dir_all()?;
    let data_dir = temp_dir.child("antnode1-data");
    data_dir.create_dir_all()?;
    let antnode_bin = data_dir.child("antnode");
    antnode_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mock_metrics_client = MockMetricsClient::new();
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(true))
        .times(1)
        .returning(|_, _| Ok(()));

    let service_data = NodeServiceData {
        alpha: false,
        auto_restart: false,
        connected_peers: None,
        data_dir_path: data_dir.to_path_buf(),
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
        initial_peers_config: Default::default(),
        listen_addr: None,
        log_dir_path: log_dir.to_path_buf(),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: None,
        network_id: None,
        node_ip: None,
        node_port: None,
        number: 1,
        pid: None,
        peer_id: None,
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: antnode_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        status: ServiceStatus::Stopped,
        service_name: "antnode1".to_string(),
        no_upnp: false,
        user: None,
        user_mode: true,
        version: "0.98.1".to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(MockRpcClient::new()),
        Box::new(mock_metrics_client),
    );
    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager.remove(false).await?;

    let service_data = service_data.read().await;
    assert_matches!(service_data.status, ServiceStatus::Removed);
    log_dir.assert(predicate::path::missing());
    data_dir.assert(predicate::path::missing());

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_the_alpha_flag() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let current_node_bin = current_install_dir.child("antnode");
    current_node_bin.write_binary(b"fake antnode binary")?;
    let target_node_bin = tmp_data_dir.child("antnode");
    target_node_bin.write_binary(b"fake antnode binary")?;

    let mut mock_service_control = MockServiceControl::new();
    let mut mock_rpc_client = MockRpcClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // before binary upgrade
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(1000));
    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // after binary upgrade
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_install()
        .with(
            eq(ServiceInstallCtx {
                args: vec![
                    OsString::from("--rpc"),
                    OsString::from("127.0.0.1:8081"),
                    OsString::from("--root-dir"),
                    OsString::from("/var/antctl/services/antnode1"),
                    OsString::from("--log-output-dest"),
                    OsString::from("/var/log/antnode/antnode1"),
                    OsString::from("--alpha"),
                    OsString::from("--rewards-address"),
                    OsString::from("0x03B770D9cD32077cC0bF330c13C114a87643B124"),
                    OsString::from("evm-arbitrum-one"),
                ],
                autostart: false,
                contents: None,
                environment: None,
                label: "antnode1".parse()?,
                program: current_node_bin.to_path_buf(),
                username: Some("ant".to_string()),
                working_directory: None,
                disable_restart_on_failure: true,
            }),
            eq(false),
        )
        .times(1)
        .returning(|_, _| Ok(()));

    // after service restart
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_wait()
        .with(eq(3000))
        .times(1)
        .returning(|_| ());
    mock_service_control
        .expect_get_process_pid()
        .with(eq(current_node_bin.to_path_buf().clone()))
        .times(1)
        .returning(|_| Ok(100));

    mock_metrics_client
        .expect_wait_until_reachability_check_completes()
        .with(eq(None))
        .times(1)
        .returning(|_| Ok(()));

    mock_rpc_client
        .expect_wait_until_node_connects_to_network()
        .times(1)
        .returning(|_| Ok(()));
    mock_rpc_client.expect_node_info().times(1).returning(|| {
        Ok(NodeInfo {
            pid: 2000,
            peer_id: PeerId::from_str("12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR")?,
            data_path: PathBuf::from("/var/antctl/services/antnode1"),
            log_path: PathBuf::from("/var/log/antnode/antnode1"),
            version: target_version.to_string(),
            uptime: std::time::Duration::from_secs(1), // the service was just started
            wallet_balance: 0,
        })
    });
    mock_rpc_client
        .expect_network_info()
        .times(1)
        .returning(|| {
            Ok(NetworkInfo {
                connected_peers: Vec::new(),
                listeners: Vec::new(),
            })
        });

    let service_data = NodeServiceData {
        alpha: true,
        auto_restart: false,
        connected_peers: None,
        data_dir_path: PathBuf::from("/var/antctl/services/antnode1"),
        evm_network: EvmNetwork::ArbitrumOne,
        relay: false,
        initial_peers_config: InitialPeersConfig {
            first: false,
            addrs: vec![],
            network_contacts_url: vec![],
            local: false,
            ignore_cache: false,
            bootstrap_cache_dir: None,
        },
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
        peer_id: Some(PeerId::from_str(
            "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        )?),
        pid: Some(1000),
        reachability_check: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")?,
        reward_balance: Some(AttoTokens::zero()),
        rpc_socket_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081),
        antnode_path: current_node_bin.to_path_buf(),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: "antnode1".to_string(),
        status: ServiceStatus::Running,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: current_version.to_string(),
        write_older_cache_files: false,
    };
    let service_data = Arc::new(RwLock::new(service_data));
    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_rpc_client),
        Box::new(mock_metrics_client),
    );

    let mut service_manager = ServiceManager::new(
        service,
        Box::new(mock_service_control),
        VerbosityLevel::Normal,
    );

    service_manager
        .upgrade(UpgradeOptions {
            auto_restart: false,
            env_variables: None,
            force: false,
            start_service: true,
            target_bin_path: target_node_bin.to_path_buf(),
            target_version: Version::parse(target_version).unwrap(),
        })
        .await?;

    let service_data = service_data.read().await;
    assert!(service_data.alpha);

    Ok(())
}
