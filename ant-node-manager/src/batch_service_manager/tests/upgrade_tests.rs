// Copyright (C) 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::helpers::*;
use ant_service_management::{
    ServiceStateActions, ServiceStatus, UpgradeOptions,
    node::NodeService,
    rpc::{NetworkInfo, NodeInfo},
};
use assert_fs::prelude::*;
use assert_matches::assert_matches;
use color_eyre::eyre::Result;
use mockall::predicate::*;
use semver::Version;
use std::{path::PathBuf, str::FromStr, sync::Arc};
use tokio::sync::RwLock;

#[tokio::test]
async fn upgrade_all_should_upgrade_services_to_new_version() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Create upgrade options
    let upgrade_options = UpgradeOptions {
        auto_restart: false,
        env_variables: None,
        force: false,
        start_service: true,
        target_bin_path: {
            let tmp_data_dir = assert_fs::TempDir::new()?;
            let target_node_bin = tmp_data_dir.child("antnode");
            target_node_bin.write_binary(b"fake antnode binary")?;
            target_node_bin.to_path_buf()
        },
        target_version: Version::new(0, 99, 0),
    };

    // Mock the stop process for each service
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| Ok(1001));
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode2/antnode")))
        .times(1)
        .returning(|_| Ok(1002));

    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_stop()
        .with(eq("antnode2"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // No reinstall process needed for binary copy upgrades

    // Mock the start process
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_start()
        .with(eq("antnode2"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    mock_service_control
        .expect_wait()
        .with(eq(1000))
        .returning(|_| ());

    // Mock get_process_pid for post-upgrade verification
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| Ok(2001));
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode2/antnode")))
        .times(1)
        .returning(|_| Ok(2002));

    // Create services with Running status for upgrade test
    let mut services = Vec::new();
    for i in 1..=2 {
        let mut service_data = create_test_service_data(i);
        service_data.status = ServiceStatus::Running;
        service_data.pid = Some(1000 + i as u32);

        let service_data = Arc::new(RwLock::new(service_data));
        let mut mock_rpc_client = MockRpcClient::new();
        let mut mock_metrics_client = MockMetricsClient::new();

        // Set up RPC mock expectations for start process
        mock_rpc_client
            .expect_wait_until_node_connects_to_network()
            .with(eq(None))
            .times(1)
            .returning(|_| Ok(()));
        mock_rpc_client
            .expect_node_info()
            .times(1)
            .returning(move || {
                Ok(NodeInfo {
                    pid: 1000 + i as u32,
                    peer_id: libp2p_identity::PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from(format!("/var/antctl/services/antnode{i}")),
                    log_path: PathBuf::from(format!("/var/log/antnode/antnode{i}")),
                    version: "0.98.1".to_string(),
                    uptime: std::time::Duration::from_secs(1),
                    wallet_balance: 0,
                })
            });
        mock_rpc_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: vec![libp2p_identity::PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?],
                    listeners: Vec::new(),
                })
            });

        // Set up metrics mock expectations
        mock_metrics_client
            .expect_wait_until_reachability_check_completes()
            .with(eq(None))
            .times(1)
            .returning(|_| Ok(()));

        let service = NodeService::new(
            service_data,
            Box::new(mock_rpc_client),
            Box::new(mock_metrics_client),
        );
        services.push(service);
    }

    let batch_manager = setup_batch_service_manager(services, mock_service_control);

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify services have been upgraded
    for service in &batch_manager.services {
        assert_eq!(service.version().await, "0.99.0");
        assert_matches!(service.status().await, ServiceStatus::Running);
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_skip_if_target_version_lower() -> Result<()> {
    let mock_service_control = MockServiceControl::new();

    // Create upgrade options with lower version
    let upgrade_options = UpgradeOptions {
        auto_restart: false,
        env_variables: None,
        force: false,
        start_service: true,
        target_bin_path: {
            let tmp_data_dir = assert_fs::TempDir::new()?;
            let target_node_bin = tmp_data_dir.child("antnode");
            target_node_bin.write_binary(b"fake antnode binary")?;
            target_node_bin.to_path_buf()
        },
        target_version: Version::new(0, 97, 0), // Lower than current 0.98.1
    };

    // No service control calls should be made when version is lower

    // Create services with Running status and exact PIDs
    let mut services = Vec::new();
    for i in 1..=2 {
        let mut service_data = create_test_service_data(i);
        service_data.status = ServiceStatus::Running;
        service_data.pid = Some(1000 + i as u32);

        let service_data = Arc::new(RwLock::new(service_data));
        let mock_rpc_client = MockRpcClient::new();
        let mock_metrics_client = MockMetricsClient::new();

        let service = NodeService::new(
            service_data,
            Box::new(mock_rpc_client),
            Box::new(mock_metrics_client),
        );
        services.push(service);
    }

    let batch_manager = setup_batch_service_manager(services, mock_service_control);

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify services remain at original version
    for service in &batch_manager.services {
        assert_eq!(service.version().await, "0.98.1");
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_force_downgrade_when_requested() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Create upgrade options with force flag
    let upgrade_options = UpgradeOptions {
        auto_restart: false,
        env_variables: None,
        force: true,
        start_service: true,
        target_bin_path: {
            let tmp_data_dir = assert_fs::TempDir::new()?;
            let target_node_bin = tmp_data_dir.child("antnode");
            target_node_bin.write_binary(b"fake antnode binary")?;
            target_node_bin.to_path_buf()
        },
        target_version: Version::new(0, 97, 0), // Lower than current 0.98.1
    };

    // Mock the stop process for each service
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| Ok(1001));
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode2/antnode")))
        .times(1)
        .returning(|_| Ok(1002));

    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_stop()
        .with(eq("antnode2"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // No reinstall process needed for binary copy upgrades

    // Mock the start process
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_start()
        .with(eq("antnode2"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    mock_service_control
        .expect_wait()
        .with(eq(1000))
        .returning(|_| ());

    // Mock get_process_pid for post-upgrade verification
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| Ok(2001));
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode2/antnode")))
        .times(1)
        .returning(|_| Ok(2002));

    // Create services with Running status for upgrade test
    let mut services = Vec::new();
    for i in 1..=2 {
        let mut service_data = create_test_service_data(i);
        service_data.status = ServiceStatus::Running;
        service_data.pid = Some(1000 + i as u32);

        let service_data = Arc::new(RwLock::new(service_data));
        let mut mock_rpc_client = MockRpcClient::new();
        let mut mock_metrics_client = MockMetricsClient::new();

        // Set up RPC mock expectations for start process
        mock_rpc_client
            .expect_wait_until_node_connects_to_network()
            .with(eq(None))
            .times(1)
            .returning(|_| Ok(()));
        mock_rpc_client
            .expect_node_info()
            .times(1)
            .returning(move || {
                Ok(NodeInfo {
                    pid: 1000 + i as u32,
                    peer_id: libp2p_identity::PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from(format!("/var/antctl/services/antnode{i}")),
                    log_path: PathBuf::from(format!("/var/log/antnode/antnode{i}")),
                    version: "0.98.1".to_string(),
                    uptime: std::time::Duration::from_secs(1),
                    wallet_balance: 0,
                })
            });
        mock_rpc_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: vec![libp2p_identity::PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?],
                    listeners: Vec::new(),
                })
            });

        // Set up metrics mock expectations
        mock_metrics_client
            .expect_wait_until_reachability_check_completes()
            .with(eq(None))
            .times(1)
            .returning(|_| Ok(()));

        let service = NodeService::new(
            service_data,
            Box::new(mock_rpc_client),
            Box::new(mock_metrics_client),
        );
        services.push(service);
    }

    let batch_manager = setup_batch_service_manager(services, mock_service_control);

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify services have been downgraded
    for service in &batch_manager.services {
        assert_eq!(service.version().await, "0.97.0");
        assert_matches!(service.status().await, ServiceStatus::Running);
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_upgrade_and_not_start_services() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Create upgrade options without starting services
    let upgrade_options = UpgradeOptions {
        auto_restart: false,
        env_variables: None,
        force: false,
        start_service: false,
        target_bin_path: {
            let tmp_data_dir = assert_fs::TempDir::new()?;
            let target_node_bin = tmp_data_dir.child("antnode");
            target_node_bin.write_binary(b"fake antnode binary")?;
            target_node_bin.to_path_buf()
        },
        target_version: Version::new(0, 99, 0),
    };

    // Mock the stop process for each service
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| Ok(1001));
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode2/antnode")))
        .times(1)
        .returning(|_| Ok(1002));

    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_stop()
        .with(eq("antnode2"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // No reinstall process needed for binary copy upgrades

    // No start calls should be made when start_service: false

    // Create services with Running status for upgrade test
    let mut services = Vec::new();
    for i in 1..=2 {
        let mut service_data = create_test_service_data(i);
        service_data.status = ServiceStatus::Running;
        service_data.pid = Some(1000 + i as u32);

        let service_data = Arc::new(RwLock::new(service_data));
        let mock_rpc_client = MockRpcClient::new();
        let mock_metrics_client = MockMetricsClient::new();

        let service = NodeService::new(
            service_data,
            Box::new(mock_rpc_client),
            Box::new(mock_metrics_client),
        );
        services.push(service);
    }

    let batch_manager = setup_batch_service_manager(services, mock_service_control);

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify services have been upgraded but not started due to start_service: false
    for service in &batch_manager.services {
        assert_eq!(service.version().await, "0.99.0");
        assert_matches!(service.status().await, ServiceStatus::Stopped);
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_handle_start_failures_after_upgrade() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    let upgrade_options = UpgradeOptions {
        auto_restart: false,
        env_variables: None,
        force: false,
        start_service: true,
        target_bin_path: {
            let tmp_data_dir = assert_fs::TempDir::new()?;
            let target_node_bin = tmp_data_dir.child("antnode");
            target_node_bin.write_binary(b"fake antnode binary")?;
            target_node_bin.to_path_buf()
        },
        target_version: Version::new(0, 99, 0),
    };

    // Mock the stop process for each service
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| Ok(1001));
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode2/antnode")))
        .times(1)
        .returning(|_| Ok(1002));

    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_stop()
        .with(eq("antnode2"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // No reinstall process needed for binary copy upgrades

    // Start succeeds but process PID lookup fails
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_start()
        .with(eq("antnode2"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    mock_service_control
        .expect_wait()
        .with(eq(1000))
        .returning(|_| ());

    // Mock get_process_pid for post-upgrade verification - this will fail
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| {
            Err(ant_service_management::error::Error::ServiceProcessNotFound("service".to_string()))
        });
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode2/antnode")))
        .times(1)
        .returning(|_| {
            Err(ant_service_management::error::Error::ServiceProcessNotFound("service".to_string()))
        });

    // Create services with Running status for upgrade test
    let mut services = Vec::new();
    for i in 1..=2 {
        let mut service_data = create_test_service_data(i);
        service_data.status = ServiceStatus::Running;
        service_data.pid = Some(1000 + i as u32);

        let service_data = Arc::new(RwLock::new(service_data));
        let mut mock_rpc_client = MockRpcClient::new();
        let mut mock_metrics_client = MockMetricsClient::new();

        // Set up RPC mock expectations - these will be called during start but fail
        mock_rpc_client
            .expect_wait_until_node_connects_to_network()
            .with(eq(None))
            .times(1)
            .returning(|_| Ok(()));

        // Set up metrics mock expectations - these will be called during start but fail
        mock_metrics_client
            .expect_wait_until_reachability_check_completes()
            .with(eq(None))
            .times(1)
            .returning(|_| Ok(()));

        let service = NodeService::new(
            service_data,
            Box::new(mock_rpc_client),
            Box::new(mock_metrics_client),
        );
        services.push(service);
    }

    let batch_manager = setup_batch_service_manager(services, mock_service_control);

    // This should complete but with errors in the BatchResult
    let (batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;
    assert!(!batch_result.errors.is_empty());

    // Verify services have been upgraded but failed to start
    for service in &batch_manager.services {
        assert_eq!(service.version().await, "0.99.0");
    }

    Ok(())
}

#[tokio::test]
async fn upgrade_all_should_upgrade_user_mode_services() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    let upgrade_options = UpgradeOptions {
        auto_restart: false,
        env_variables: None,
        force: false,
        start_service: true,
        target_bin_path: {
            let tmp_data_dir = assert_fs::TempDir::new()?;
            let target_node_bin = tmp_data_dir.child("antnode");
            target_node_bin.write_binary(b"fake antnode binary")?;
            target_node_bin.to_path_buf()
        },
        target_version: Version::new(0, 99, 0),
    };

    // Mock the stop process for each service
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| Ok(1001));
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode2/antnode")))
        .times(1)
        .returning(|_| Ok(1002));

    mock_service_control
        .expect_stop()
        .with(eq("antnode1"), eq(true))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_stop()
        .with(eq("antnode2"), eq(true))
        .times(1)
        .returning(|_, _| Ok(()));

    // No reinstall process needed for binary copy upgrades

    // Mock the start process
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(true))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_start()
        .with(eq("antnode2"), eq(true))
        .times(1)
        .returning(|_, _| Ok(()));

    mock_service_control
        .expect_wait()
        .with(eq(1000))
        .returning(|_| ());

    // Mock get_process_pid for post-upgrade verification
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| Ok(2001));
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode2/antnode")))
        .times(1)
        .returning(|_| Ok(2002));

    // Create services with Running status and user mode
    let mut services = Vec::new();
    for i in 1..=2 {
        let mut service_data = create_test_service_data(i);
        service_data.status = ServiceStatus::Running;
        service_data.pid = Some(1000 + i as u32);
        service_data.user_mode = true;

        let service_data = Arc::new(RwLock::new(service_data));
        let mut mock_rpc_client = MockRpcClient::new();
        let mut mock_metrics_client = MockMetricsClient::new();

        // Set up RPC mock expectations for start process
        mock_rpc_client
            .expect_wait_until_node_connects_to_network()
            .with(eq(None))
            .times(1)
            .returning(|_| Ok(()));
        mock_rpc_client
            .expect_node_info()
            .times(1)
            .returning(move || {
                Ok(NodeInfo {
                    pid: 1000 + i as u32,
                    peer_id: libp2p_identity::PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?,
                    data_path: PathBuf::from(format!("/var/antctl/services/antnode{i}")),
                    log_path: PathBuf::from(format!("/var/log/antnode/antnode{i}")),
                    version: "0.98.1".to_string(),
                    uptime: std::time::Duration::from_secs(1),
                    wallet_balance: 0,
                })
            });
        mock_rpc_client
            .expect_network_info()
            .times(1)
            .returning(|| {
                Ok(NetworkInfo {
                    connected_peers: vec![libp2p_identity::PeerId::from_str(
                        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
                    )?],
                    listeners: Vec::new(),
                })
            });

        // Set up metrics mock expectations
        mock_metrics_client
            .expect_wait_until_reachability_check_completes()
            .with(eq(None))
            .times(1)
            .returning(|_| Ok(()));

        let service = NodeService::new(
            service_data,
            Box::new(mock_rpc_client),
            Box::new(mock_metrics_client),
        );
        services.push(service);
    }

    let batch_manager = setup_batch_service_manager(services, mock_service_control);

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify services have been upgraded
    for service in &batch_manager.services {
        assert_eq!(service.version().await, "0.99.0");
        assert_matches!(service.status().await, ServiceStatus::Running);
    }

    Ok(())
}
