// Copyright (C) 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::helpers::*;
use crate::batch_service_manager::{BatchServiceManager, UpgradeOptions, VerbosityLevel};
use ant_evm::{CustomNetwork, EvmNetwork, RewardsAddress};
use ant_logging::LogFormat;
use ant_service_management::{
    ReachabilityProgress, ServiceStatus,
    fs::NodeInfo,
    node::{NodeService, NodeServiceData},
};
use color_eyre::eyre::Result;
use mockall::predicate::*;
use semver::Version;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    str::FromStr,
    sync::Arc,
};
use tokio::sync::RwLock;

// Helper function to create service with specific configuration for testing
fn create_test_service_with_config(
    number: u16,
    config_modifier: impl Fn(&mut NodeServiceData),
) -> Result<NodeService> {
    let mut service_data = create_test_service_data(number);
    config_modifier(&mut service_data);

    // Capture the expected port after config modification
    let expected_port = service_data.node_port.unwrap_or(6000 + number);

    let service_data = Arc::new(RwLock::new(service_data));
    let mut mock_fs_client = MockFileSystemClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // Set up filesystem mock expectations for node_info (called during on_start)
    mock_fs_client
        .expect_node_info()
        .times(1) // Called once during service startup after upgrade
        .returning(move |_root_dir| {
            Ok(NodeInfo {
                listeners: vec![
                    format!("/ip4/127.0.0.1/udp/{expected_port}")
                        .parse()
                        .unwrap(),
                ],
            })
        });

    // Set up metrics mock expectations for get_node_metrics
    mock_metrics_client
        .expect_get_node_metrics()
        .times(2)
        .returning(move || {
            Ok(ant_service_management::metric::NodeMetrics {
                reachability_status: ant_service_management::metric::ReachabilityStatusValues {
                    progress: ReachabilityProgress::Complete,
                    upnp: false,
                    public: true,
                    private: false,
                },
                connected_peers: 10,
            })
        });

    // Set up metrics mock expectations for get_node_metadata_extended
    mock_metrics_client
        .expect_get_node_metadata_extended()
        .times(1)
        .returning(move || {
            Ok(ant_service_management::metric::NodeMetadataExtended {
                pid: 1000 + number as u32,
                peer_id: get_test_peer_id(number as usize - 1),
                root_dir: PathBuf::from(format!("/var/antctl/services/antnode{number}")),
                log_dir: PathBuf::from(format!("/var/log/antnode/antnode{number}")),
            })
        });

    Ok(NodeService::new(
        service_data,
        Box::new(mock_fs_client),
        Box::new(mock_metrics_client),
    ))
}

// Helper function to set up upgrade mock expectations for config retention tests
fn setup_upgrade_mock_expectations(
    mock_service_control: &mut MockServiceControl,
    service_count: usize,
) {
    for i in 1..=service_count {
        let service_name = format!("antnode{i}");

        // Mock the stop process during upgrade
        mock_service_control
            .expect_get_process_pid()
            .with(eq(PathBuf::from(format!(
                "/var/antctl/services/antnode{i}/antnode"
            ))))
            .times(1)
            .returning(move |_| Ok(1000 + i as u32));
        mock_service_control
            .expect_stop()
            .with(eq(service_name.clone()), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // Mock the reinstall process (may not be called if upgrade is just binary copy)
        // No uninstall/install calls expected for config retention tests

        // Mock the start process after upgrade
        mock_service_control
            .expect_start()
            .with(eq(service_name), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_service_control
            .expect_wait()
            .with(eq(1000))
            .times(1)
            .returning(|_| ());

        // Mock get_process_pid for post-upgrade verification
        mock_service_control
            .expect_get_process_pid()
            .with(eq(PathBuf::from(format!(
                "/var/antctl/services/antnode{i}/antnode"
            ))))
            .times(1)
            .returning(move |_| Ok(2000 + i as u32));
    }
}

// Helper function to create upgrade options for config retention tests
fn create_upgrade_options() -> UpgradeOptions {
    UpgradeOptions {
        auto_restart: false,
        env_variables: None,
        force: false,
        start_service: true,
        target_bin_path: PathBuf::from("/fake/antnode"),
        target_version: Version::parse("0.99.0").unwrap(), // Higher than default 0.98.1
    }
}

#[tokio::test]
async fn upgrade_all_should_retain_the_first_flag() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up upgrade expectations for 2 services
    setup_upgrade_mock_expectations(&mut mock_service_control, 2);

    let mut services = Vec::new();
    for i in 1..=2 {
        let service = create_test_service_with_config(i, |data| {
            data.initial_peers_config.first = true;
            data.status = ServiceStatus::Running;
            data.pid = Some(1000 + i as u32);
        })?;
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let upgrade_options = create_upgrade_options();

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify first flag is retained for all services
    for service in &batch_manager.services {
        let service_data = service.service_data.read().await;
        assert!(service_data.initial_peers_config.first);
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_retain_the_peers_arg() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up upgrade expectations for 2 services
    setup_upgrade_mock_expectations(&mut mock_service_control, 2);

    let test_peers = vec![
        "/ip4/127.0.0.1/tcp/12000".parse().unwrap(),
        "/ip4/127.0.0.1/tcp/12001".parse().unwrap(),
    ];

    let mut services = Vec::new();
    for i in 1..=2 {
        let service = create_test_service_with_config(i, |data| {
            data.initial_peers_config.addrs = test_peers.clone();
            data.status = ServiceStatus::Running;
            data.pid = Some(1000 + i as u32);
        })?;
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let upgrade_options = create_upgrade_options();

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify peers are retained for all services
    for service in &batch_manager.services {
        let service_data = service.service_data.read().await;
        assert_eq!(service_data.initial_peers_config.addrs, test_peers);
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_retain_the_network_id_arg() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up upgrade expectations for 2 services
    setup_upgrade_mock_expectations(&mut mock_service_control, 2);

    let test_network_id = Some(123u8);

    let mut services = Vec::new();
    for i in 1..=2 {
        let service = create_test_service_with_config(i, |data| {
            data.network_id = test_network_id;
            data.status = ServiceStatus::Running;
            data.pid = Some(1000 + i as u32);
        })?;
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let upgrade_options = create_upgrade_options();

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify network_id is retained for all services
    for service in &batch_manager.services {
        let service_data = service.service_data.read().await;
        assert_eq!(service_data.network_id, test_network_id);
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_retain_the_local_flag() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up upgrade expectations for 2 services
    setup_upgrade_mock_expectations(&mut mock_service_control, 2);

    let mut services = Vec::new();
    for i in 1..=2 {
        let service = create_test_service_with_config(i, |data| {
            data.initial_peers_config.local = true;
            data.status = ServiceStatus::Running;
            data.pid = Some(1000 + i as u32);
        })?;
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let upgrade_options = create_upgrade_options();

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify local flag is retained for all services
    for service in &batch_manager.services {
        let service_data = service.service_data.read().await;
        assert!(service_data.initial_peers_config.local);
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_retain_the_network_contacts_url_arg() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up upgrade expectations for 2 services
    setup_upgrade_mock_expectations(&mut mock_service_control, 2);

    let test_contacts_url = vec!["https://bootstrap.example.com".to_string()];

    let mut services = Vec::new();
    for i in 1..=2 {
        let service = create_test_service_with_config(i, |data| {
            data.initial_peers_config.network_contacts_url = test_contacts_url.clone();
            data.status = ServiceStatus::Running;
            data.pid = Some(1000 + i as u32);
        })?;
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let upgrade_options = create_upgrade_options();

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify network_contacts_url is retained for all services
    for service in &batch_manager.services {
        let service_data = service.service_data.read().await;
        assert_eq!(
            service_data.initial_peers_config.network_contacts_url,
            test_contacts_url
        );
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_retain_the_ignore_cache_flag() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up upgrade expectations for 2 services
    setup_upgrade_mock_expectations(&mut mock_service_control, 2);

    let mut services = Vec::new();
    for i in 1..=2 {
        let service = create_test_service_with_config(i, |data| {
            data.initial_peers_config.ignore_cache = true;
            data.status = ServiceStatus::Running;
            data.pid = Some(1000 + i as u32);
        })?;
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let upgrade_options = create_upgrade_options();

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify ignore_cache flag is retained for all services
    for service in &batch_manager.services {
        let service_data = service.service_data.read().await;
        assert!(service_data.initial_peers_config.ignore_cache);
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_retain_the_custom_bootstrap_cache_path() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up upgrade expectations for 2 services
    setup_upgrade_mock_expectations(&mut mock_service_control, 2);

    let test_cache_path = Some(PathBuf::from("/custom/bootstrap/cache"));

    let mut services = Vec::new();
    for i in 1..=2 {
        let service = create_test_service_with_config(i, |data| {
            data.initial_peers_config.bootstrap_cache_dir = test_cache_path.clone();
            data.status = ServiceStatus::Running;
            data.pid = Some(1000 + i as u32);
        })?;
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let upgrade_options = create_upgrade_options();

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify bootstrap_cache_dir is retained for all services
    for service in &batch_manager.services {
        let service_data = service.service_data.read().await;
        assert_eq!(
            service_data.initial_peers_config.bootstrap_cache_dir,
            test_cache_path
        );
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_retain_the_no_upnp_flag() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up upgrade expectations for 2 services
    setup_upgrade_mock_expectations(&mut mock_service_control, 2);

    let mut services = Vec::new();
    for i in 1..=2 {
        let service = create_test_service_with_config(i, |data| {
            data.no_upnp = true;
            data.status = ServiceStatus::Running;
            data.pid = Some(1000 + i as u32);
        })?;
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let upgrade_options = create_upgrade_options();

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify no_upnp flag is retained for all services
    for service in &batch_manager.services {
        let service_data = service.service_data.read().await;
        assert!(service_data.no_upnp);
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_retain_the_log_format_flag() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up upgrade expectations for 2 services
    setup_upgrade_mock_expectations(&mut mock_service_control, 2);

    let test_log_format = Some(LogFormat::Json);

    let mut services = Vec::new();
    for i in 1..=2 {
        let service = create_test_service_with_config(i, |data| {
            data.log_format = test_log_format;
            data.status = ServiceStatus::Running;
            data.pid = Some(1000 + i as u32);
        })?;
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let upgrade_options = create_upgrade_options();

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify log_format is retained for all services
    for service in &batch_manager.services {
        let service_data = service.service_data.read().await;
        assert_eq!(service_data.log_format, test_log_format);
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_retain_the_skip_reachability_check_flag() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up upgrade expectations for 2 services
    setup_upgrade_mock_expectations(&mut mock_service_control, 2);

    let mut services = Vec::new();
    for i in 1..=2 {
        let service = create_test_service_with_config(i, |data| {
            data.skip_reachability_check = true;
            data.status = ServiceStatus::Running;
            data.pid = Some(1000 + i as u32);
        })?;
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let upgrade_options = create_upgrade_options();

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify skip_reachability_check flag is retained for all services
    for service in &batch_manager.services {
        let service_data = service.service_data.read().await;
        assert!(service_data.skip_reachability_check);
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_retain_custom_node_ip() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up upgrade expectations for 2 services
    setup_upgrade_mock_expectations(&mut mock_service_control, 2);

    let test_node_ip = Some(Ipv4Addr::new(192, 168, 1, 100));

    let mut services = Vec::new();
    for i in 1..=2 {
        let service = create_test_service_with_config(i, |data| {
            data.node_ip = test_node_ip;
            data.status = ServiceStatus::Running;
            data.pid = Some(1000 + i as u32);
        })?;
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let upgrade_options = create_upgrade_options();

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify node_ip is retained for all services
    for service in &batch_manager.services {
        let service_data = service.service_data.read().await;
        assert_eq!(service_data.node_ip, test_node_ip);
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_retain_custom_node_ports() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up upgrade expectations for 2 services
    setup_upgrade_mock_expectations(&mut mock_service_control, 2);

    let mut services = Vec::new();
    for i in 1..=2 {
        let service = create_test_service_with_config(i, |data| {
            data.node_port = Some(9000 + i);
            data.status = ServiceStatus::Running;
            data.pid = Some(1000 + i as u32);
        })?;
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let upgrade_options = create_upgrade_options();

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify node_port is retained for all services
    for (idx, service) in batch_manager.services.iter().enumerate() {
        let service_data = service.service_data.read().await;
        assert_eq!(service_data.node_port, Some(9000 + (idx + 1) as u16));
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_retain_max_archived_log_files() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up upgrade expectations for 2 services
    setup_upgrade_mock_expectations(&mut mock_service_control, 2);

    let test_max_archived = Some(50);

    let mut services = Vec::new();
    for i in 1..=2 {
        let service = create_test_service_with_config(i, |data| {
            data.max_archived_log_files = test_max_archived;
            data.status = ServiceStatus::Running;
            data.pid = Some(1000 + i as u32);
        })?;
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let upgrade_options = create_upgrade_options();

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify max_archived_log_files is retained for all services
    for service in &batch_manager.services {
        let service_data = service.service_data.read().await;
        assert_eq!(service_data.max_archived_log_files, test_max_archived);
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_retain_max_log_files() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up upgrade expectations for 2 services
    setup_upgrade_mock_expectations(&mut mock_service_control, 2);

    let test_max_log_files = Some(20);

    let mut services = Vec::new();
    for i in 1..=2 {
        let service = create_test_service_with_config(i, |data| {
            data.max_log_files = test_max_log_files;
            data.status = ServiceStatus::Running;
            data.pid = Some(1000 + i as u32);
        })?;
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let upgrade_options = create_upgrade_options();

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify max_log_files is retained for all services
    for service in &batch_manager.services {
        let service_data = service.service_data.read().await;
        assert_eq!(service_data.max_log_files, test_max_log_files);
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_retain_custom_metrics_ports() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up upgrade expectations for 2 services
    setup_upgrade_mock_expectations(&mut mock_service_control, 2);

    let mut services = Vec::new();
    for i in 1..=2 {
        let service = create_test_service_with_config(i, |data| {
            data.metrics_port = 8000 + i;
            data.status = ServiceStatus::Running;
            data.pid = Some(1000 + i as u32);
        })?;
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let upgrade_options = create_upgrade_options();

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify metrics_port is retained for all services
    for (idx, service) in batch_manager.services.iter().enumerate() {
        let service_data = service.service_data.read().await;
        assert_eq!(service_data.metrics_port, 8000 + (idx + 1) as u16);
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_retain_custom_rpc_ports() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up upgrade expectations for 2 services
    setup_upgrade_mock_expectations(&mut mock_service_control, 2);

    let mut services = Vec::new();
    for i in 1..=2 {
        let service = create_test_service_with_config(i, |data| {
            data.rpc_socket_addr = Some(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                7000 + i,
            ));
            data.status = ServiceStatus::Running;
            data.pid = Some(1000 + i as u32);
        })?;
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let upgrade_options = create_upgrade_options();

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify rpc_socket_addr is retained for all services
    for (idx, service) in batch_manager.services.iter().enumerate() {
        let service_data = service.service_data.read().await;
        let expected_addr = SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            7000 + (idx + 1) as u16,
        );
        assert_eq!(service_data.rpc_socket_addr, Some(expected_addr));
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_retain_auto_restart() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up upgrade expectations for 2 services
    setup_upgrade_mock_expectations(&mut mock_service_control, 2);

    let mut services = Vec::new();
    for i in 1..=2 {
        let service = create_test_service_with_config(i, |data| {
            data.auto_restart = true;
            data.status = ServiceStatus::Running;
            data.pid = Some(1000 + i as u32);
        })?;
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let upgrade_options = create_upgrade_options();

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify auto_restart is retained for all services
    for service in &batch_manager.services {
        let service_data = service.service_data.read().await;
        assert!(service_data.auto_restart);
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_retain_evm_network_settings() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up upgrade expectations for 2 services
    setup_upgrade_mock_expectations(&mut mock_service_control, 2);

    let test_evm_network = EvmNetwork::Custom(CustomNetwork {
        rpc_url_http: "http://localhost:9545".parse().unwrap(),
        payment_token_address: RewardsAddress::from_str(
            "0x1234567890123456789012345678901234567890",
        )?,
        data_payments_address: RewardsAddress::from_str(
            "0x0987654321098765432109876543210987654321",
        )?,
    });

    let mut services = Vec::new();
    for i in 1..=2 {
        let service = create_test_service_with_config(i, |data| {
            data.evm_network = test_evm_network.clone();
            data.status = ServiceStatus::Running;
            data.pid = Some(1000 + i as u32);
        })?;
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let upgrade_options = create_upgrade_options();

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify evm_network is retained for all services
    for service in &batch_manager.services {
        let service_data = service.service_data.read().await;
        match (&service_data.evm_network, &test_evm_network) {
            (EvmNetwork::Custom(actual), EvmNetwork::Custom(expected)) => {
                assert_eq!(actual.rpc_url_http, expected.rpc_url_http);
                assert_eq!(actual.payment_token_address, expected.payment_token_address);
                assert_eq!(actual.data_payments_address, expected.data_payments_address);
            }
            _ => panic!("Expected Custom EVM network"),
        }
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_retain_the_rewards_address() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up upgrade expectations for 2 services
    setup_upgrade_mock_expectations(&mut mock_service_control, 2);

    let test_rewards_address =
        RewardsAddress::from_str("0x1111111111111111111111111111111111111111")?;

    let mut services = Vec::new();
    for i in 1..=2 {
        let service = create_test_service_with_config(i, |data| {
            data.rewards_address = test_rewards_address;
            data.status = ServiceStatus::Running;
            data.pid = Some(1000 + i as u32);
        })?;
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let upgrade_options = create_upgrade_options();

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify rewards_address is retained for all services
    for service in &batch_manager.services {
        let service_data = service.service_data.read().await;
        assert_eq!(service_data.rewards_address, test_rewards_address);
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_retain_the_alpha_flag() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up upgrade expectations for 2 services
    setup_upgrade_mock_expectations(&mut mock_service_control, 2);

    let mut services = Vec::new();
    for i in 1..=2 {
        let service = create_test_service_with_config(i, |data| {
            data.alpha = true;
            data.status = ServiceStatus::Running;
            data.pid = Some(1000 + i as u32);
        })?;
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let upgrade_options = create_upgrade_options();

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify alpha flag is retained for all services
    for service in &batch_manager.services {
        let service_data = service.service_data.read().await;
        assert!(service_data.alpha);
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_should_retain_write_older_cache_files() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up upgrade expectations for 2 services
    setup_upgrade_mock_expectations(&mut mock_service_control, 2);

    let mut services = Vec::new();
    for i in 1..=2 {
        let service = create_test_service_with_config(i, |data| {
            data.write_older_cache_files = true;
            data.status = ServiceStatus::Running;
            data.pid = Some(1000 + i as u32);
        })?;
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let upgrade_options = create_upgrade_options();

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify write_older_cache_files flag is retained for all services
    for service in &batch_manager.services {
        let service_data = service.service_data.read().await;
        assert!(service_data.write_older_cache_files);
    }

    Ok(())
}
