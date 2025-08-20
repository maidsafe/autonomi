// Copyright (C) 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::helpers::*;
use ant_service_management::{
    ServiceStateActions, ServiceStatus, UpgradeOptions, fs::NodeInfo, node::NodeService,
};
use assert_fs::prelude::*;
use assert_matches::assert_matches;
use color_eyre::eyre::Result;
use mockall::predicate::*;
use semver::Version;
use service_manager::ServiceInstallCtx;
use std::{ffi::OsString, path::PathBuf, sync::Arc};
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
        let mut mock_fs_client = MockFileSystemClient::new();
        let mut mock_metrics_client = MockMetricsClient::new();

        mock_fs_client
            .expect_node_info()
            .times(1)
            .returning(move |_root_dir| {
                Ok(NodeInfo {
                    listeners: vec![format!("/ip4/127.0.0.1/udp/600{i}").parse().unwrap()],
                })
            });

        // Set up metrics mock expectations for get_node_metrics
        mock_metrics_client
            .expect_get_node_metrics()
            .times(1)
            .returning(move || {
                Ok(ant_service_management::metric::NodeMetrics {
                    reachability_status: ant_service_management::metric::ReachabilityStatusValues {
                        progress_percent: 100,
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
                    pid: 1000 + i as u32,
                    peer_id: get_test_peer_id((i - 1) as usize),
                    root_dir: PathBuf::from(format!("/var/antctl/services/antnode{i}")),
                    log_dir: PathBuf::from(format!("/var/log/antnode/antnode{i}")),
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
            Box::new(mock_fs_client),
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
        let mock_fs_client = MockFileSystemClient::new();
        let mock_metrics_client = MockMetricsClient::new();

        let service = NodeService::new(
            service_data,
            Box::new(mock_fs_client),
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
        let mut mock_fs_client = MockFileSystemClient::new();
        let mut mock_metrics_client = MockMetricsClient::new();

        mock_fs_client
            .expect_node_info()
            .times(1)
            .returning(move |_root_dir| {
                Ok(NodeInfo {
                    listeners: vec![format!("/ip4/127.0.0.1/udp/600{i}").parse().unwrap()],
                })
            });

        // Set up metrics mock expectations for get_node_metrics
        mock_metrics_client
            .expect_get_node_metrics()
            .times(1)
            .returning(move || {
                Ok(ant_service_management::metric::NodeMetrics {
                    reachability_status: ant_service_management::metric::ReachabilityStatusValues {
                        progress_percent: 100,
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
                    pid: 1000 + i as u32,
                    peer_id: get_test_peer_id((i - 1) as usize),
                    root_dir: PathBuf::from(format!("/var/antctl/services/antnode{i}")),
                    log_dir: PathBuf::from(format!("/var/log/antnode/antnode{i}")),
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
            Box::new(mock_fs_client),
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
        let mock_fs_client = MockFileSystemClient::new();
        let mock_metrics_client = MockMetricsClient::new();

        let service = NodeService::new(
            service_data,
            Box::new(mock_fs_client),
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
        let mock_fs_client = MockFileSystemClient::new();
        let mut mock_metrics_client = MockMetricsClient::new();

        // Set up metrics mock expectations - these will be called during start but fail
        mock_metrics_client
            .expect_wait_until_reachability_check_completes()
            .with(eq(None))
            .times(1)
            .returning(|_| Ok(()));

        let service = NodeService::new(
            service_data,
            Box::new(mock_fs_client),
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
        let mut mock_fs_client = MockFileSystemClient::new();
        let mut mock_metrics_client = MockMetricsClient::new();

        mock_fs_client
            .expect_node_info()
            .times(1)
            .returning(move |_root_dir| {
                Ok(NodeInfo {
                    listeners: vec![format!("/ip4/127.0.0.1/udp/600{i}").parse().unwrap()],
                })
            });

        // Set up metrics mock expectations for get_node_metrics
        mock_metrics_client
            .expect_get_node_metrics()
            .times(1)
            .returning(move || {
                Ok(ant_service_management::metric::NodeMetrics {
                    reachability_status: ant_service_management::metric::ReachabilityStatusValues {
                        progress_percent: 100,
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
                    pid: 1000 + i as u32,
                    peer_id: get_test_peer_id((i - 1) as usize),
                    root_dir: PathBuf::from(format!("/var/antctl/services/antnode{i}")),
                    log_dir: PathBuf::from(format!("/var/log/antnode/antnode{i}")),
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
            Box::new(mock_fs_client),
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
async fn upgrade_all_should_set_metrics_port_if_not_set() -> Result<()> {
    let current_version = "0.1.0";
    let target_version = "0.2.0";

    let tmp_data_dir = assert_fs::TempDir::new()?;
    let current_install_dir = tmp_data_dir.child("antnode_install");
    current_install_dir.create_dir_all()?;

    let mut mock_service_control = MockServiceControl::new();

    // Create upgrade options
    let upgrade_options = UpgradeOptions {
        auto_restart: false,
        env_variables: None,
        force: false,
        start_service: true,
        target_bin_path: {
            let target_node_bin = tmp_data_dir.child("antnode");
            target_node_bin.write_binary(b"fake antnode binary")?;
            target_node_bin.to_path_buf()
        },
        target_version: Version::parse(target_version).unwrap(),
    };

    // Create services with metrics_port set to None and specific paths
    let mut services = Vec::new();
    for i in 1..=2 {
        let mut service_data = create_test_service_data(i);
        service_data.status = ServiceStatus::Running;
        service_data.pid = Some(1000 + i as u32);
        service_data.metrics_port = None; // Important: no metrics port initially
        service_data.version = current_version.to_string();

        // Set specific binary path for this test
        let current_node_bin = current_install_dir.child(format!("antnode{i}"));
        current_node_bin.write_binary(b"fake antnode binary")?;
        service_data.antnode_path = current_node_bin.to_path_buf();

        let service_name = format!("antnode{i}");

        // Mock the stop process for this service
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf()))
            .times(1)
            .returning(move |_| Ok(1000 + i as u32));
        mock_service_control
            .expect_stop()
            .with(eq(service_name.clone()), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // Mock uninstall
        mock_service_control
            .expect_uninstall()
            .with(eq(service_name.clone()), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // Mock get_available_port - this should be called when metrics_port is None
        let expected_port = 6000 + i;
        mock_service_control
            .expect_get_available_port()
            .times(1)
            .returning(move || Ok(expected_port));

        // Mock install with specific ServiceInstallCtx expectations
        let expected_service_name = service_name.clone();
        let expected_bin_path = current_node_bin.to_path_buf();
        let expected_port_for_assert = expected_port;
        let expected_rpc_port = 8080 + i;

        mock_service_control
            .expect_install()
            .with(
                eq(ServiceInstallCtx {
                    args: vec![
                        OsString::from("--rpc"),
                        OsString::from(format!("127.0.0.1:{expected_rpc_port}")),
                        OsString::from("--root-dir"),
                        OsString::from(format!("/var/antctl/services/antnode{i}")),
                        OsString::from("--log-output-dest"),
                        OsString::from(format!("/var/log/antnode/antnode{i}")),
                        OsString::from("--metrics-server-port"),
                        OsString::from(expected_port_for_assert.to_string()),
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
                    label: expected_service_name.parse()?,
                    program: expected_bin_path,
                    username: Some("ant".to_string()),
                    working_directory: None,
                    disable_restart_on_failure: true,
                }),
                eq(false),
            )
            .times(1)
            .returning(|_, _| Ok(()));

        // Mock the start process
        mock_service_control
            .expect_start()
            .with(eq(service_name.clone()), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // Mock post-upgrade verification
        mock_service_control
            .expect_get_process_pid()
            .with(eq(current_node_bin.to_path_buf()))
            .times(1)
            .returning(move |_| Ok(2000 + i as u32));

        let service_data = Arc::new(RwLock::new(service_data));
        let mut mock_fs_client = MockFileSystemClient::new();
        let mut mock_metrics_client = MockMetricsClient::new();

        mock_fs_client
            .expect_node_info()
            .times(1)
            .returning(move |_root_dir| {
                Ok(NodeInfo {
                    listeners: vec![format!("/ip4/127.0.0.1/udp/600{i}").parse().unwrap()],
                })
            });

        // Set up metrics mock expectations for get_node_metrics
        mock_metrics_client
            .expect_get_node_metrics()
            .times(1)
            .returning(move || {
                Ok(ant_service_management::metric::NodeMetrics {
                    reachability_status: ant_service_management::metric::ReachabilityStatusValues {
                        progress_percent: 100,
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
                    pid: 2000 + i as u32,
                    peer_id: get_test_peer_id((i - 1) as usize),
                    root_dir: PathBuf::from(format!("/var/antctl/services/antnode{i}")),
                    log_dir: PathBuf::from(format!("/var/log/antnode/antnode{i}")),
                })
            });

        // Set up metrics mock expectations
        mock_metrics_client
            .expect_wait_until_reachability_check_completes()
            .with(eq(None))
            .times(1)
            .returning(|_| Ok(()));

        let service = NodeService::new(
            Arc::clone(&service_data),
            Box::new(mock_fs_client),
            Box::new(mock_metrics_client),
        );
        services.push(service);
    }

    mock_service_control
        .expect_wait()
        .with(eq(1000))
        .returning(|_| ());

    let batch_manager = setup_batch_service_manager(services, mock_service_control);

    let (_batch_result, _upgrade_summary) = batch_manager.upgrade_all(upgrade_options, 1000).await;

    // Verify services have been upgraded and metrics ports are set
    for (i, service) in batch_manager.services.iter().enumerate() {
        assert_eq!(service.version().await, target_version);
        assert_matches!(service.status().await, ServiceStatus::Running);

        // Verify metrics port has been set
        let service_data = service.service_data.read().await;
        assert!(service_data.metrics_port.is_some());
        assert_eq!(service_data.metrics_port.unwrap(), 6001 + i as u16);
    }

    Ok(())
}
