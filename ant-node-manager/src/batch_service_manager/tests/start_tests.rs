// Copyright (C) 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::helpers::*;
use crate::batch_service_manager::{BatchServiceManager, VerbosityLevel};
use ant_service_management::{
    NodeService, ReachabilityProgress, ServiceStateActions, ServiceStatus,
};
use assert_matches::assert_matches;
use color_eyre::eyre::Result;
use mockall::predicate::*;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::Instant;

#[tokio::test]
async fn start_all_should_start_newly_installed_services() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up expectations for 2 services
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
        .times(2)
        .returning(|_| ());

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

    // Create 2 services with basic mock setup for successful startup
    let mut services = Vec::new();
    for i in 1..=2 {
        let mut mock_fs_client = MockFileSystemClient::new();
        let mut mock_metrics_client = MockMetricsClient::new();

        // File system mock for listening addresses retrieval after successful startup
        mock_fs_client
            .expect_listen_addrs()
            .with(eq(PathBuf::from(format!(
                "/var/antctl/services/antnode{i}"
            ))))
            .times(1)
            .returning(|_| Ok(vec![]));

        mock_metrics_client
            .expect_get_node_metrics()
            .times(1..)
            .returning(|| {
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

        mock_metrics_client
            .expect_get_node_metadata_extended()
            .times(1)
            .returning(move || {
                Ok(ant_service_management::metric::NodeMetadataExtended {
                    pid: 1000 + i,
                    peer_id: get_test_peer_id((i - 1) as usize),
                    root_dir: PathBuf::from(format!("/var/antctl/services/antnode{i}")),
                    log_dir: PathBuf::from(format!("/var/log/antnode/antnode{i}")),
                })
            });

        let service_data = create_test_service_data(i as u16);
        let service_data = Arc::new(RwLock::new(service_data));

        let service = NodeService::new(
            service_data,
            Box::new(mock_fs_client),
            Box::new(mock_metrics_client),
        );
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let batch_result = batch_manager.start_all(1000, true).await;
    assert!(batch_result.errors.is_empty());

    // Verify services are in running state
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Running);
        assert!(service.pid().await.is_some());
    }

    Ok(())
}

#[tokio::test]
async fn start_all_should_skip_startup_check_if_disabled() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up expectations for 2 services
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
        .times(2)
        .returning(|_| ());

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

    // Create 2 services with basic mock setup for successful startup
    let mut services = Vec::new();
    for i in 1..=2 {
        let mut mock_fs_client = MockFileSystemClient::new();
        let mut mock_metrics_client = MockMetricsClient::new();

        // No calls should be made
        mock_fs_client.expect_listen_addrs().times(0);
        mock_metrics_client.expect_get_node_metrics().times(0);
        mock_metrics_client
            .expect_get_node_metadata_extended()
            .times(0);

        let service_data = create_test_service_data(i);
        let service_data = Arc::new(RwLock::new(service_data));

        let service = NodeService::new(
            service_data,
            Box::new(mock_fs_client),
            Box::new(mock_metrics_client),
        );
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let batch_result = batch_manager.start_all(1000, false).await;
    assert!(batch_result.errors.is_empty());

    // Verify services are in running state
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Running);
        assert!(service.pid().await.is_some());
    }

    Ok(())
}

#[tokio::test]
async fn start_all_should_start_stopped_services() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up expectations for 2 services
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
        .times(2)
        .returning(|_| ());

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

    // Create services and set them to stopped state
    let services = create_test_services_with_mocks(2)?;
    for service in &services {
        let mut service_data = service.service_data.write().await;
        service_data.status = ServiceStatus::Stopped;
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let batch_result = batch_manager.start_all(1000, true).await;
    assert!(batch_result.errors.is_empty());

    // Verify services are in running state
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Running);
    }

    Ok(())
}

#[tokio::test]
async fn start_all_should_not_attempt_to_start_running_services() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up expectations - get_process_pid should be called to check if running
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

    // No start() calls should be made

    // Create services and set them to running state
    let services = create_test_services_simple(2);
    for service in &services {
        let mut service_data = service.service_data.write().await;
        service_data.status = ServiceStatus::Running;
        service_data.pid = Some(1001);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let batch_result = batch_manager.start_all(1000, true).await;
    assert!(batch_result.errors.is_empty());

    // Verify services remain in running state
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Running);
    }

    Ok(())
}

#[tokio::test]
async fn start_all_should_start_services_marked_as_running_but_had_since_stopped() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // First check if processes are running (they're not)
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| {
            Err(
                ant_service_management::error::Error::ServiceProcessNotFound(
                    "antnode1".to_string(),
                ),
            )
        });
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode2/antnode")))
        .times(1)
        .returning(|_| {
            Err(
                ant_service_management::error::Error::ServiceProcessNotFound(
                    "antnode2".to_string(),
                ),
            )
        });

    // Then start the services
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
        .times(2)
        .returning(|_| ());

    // Finally check the PIDs after starting
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

    // Create services with proper mock setup and set them to running state
    let services = create_test_services_with_mocks(2)?;
    for service in &services {
        let mut service_data = service.service_data.write().await;
        service_data.status = ServiceStatus::Running;
        service_data.pid = Some(1001); // Set a PID even though process isn't actually running
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let batch_result = batch_manager.start_all(1000, true).await;
    assert!(batch_result.errors.is_empty());

    // Verify services are in running state
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Running);
        assert!(service.pid().await.is_some());
    }

    Ok(())
}

#[tokio::test]
async fn start_all_should_return_error_if_processes_not_found() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up services to start successfully
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
        .times(2)
        .returning(|_| ());

    // But PIDs can't be found after starting
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| {
            Err(
                ant_service_management::error::Error::ServiceProcessNotFound(
                    "antnode1".to_string(),
                ),
            )
        });
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode2/antnode")))
        .times(1)
        .returning(|_| {
            Err(
                ant_service_management::error::Error::ServiceProcessNotFound(
                    "antnode2".to_string(),
                ),
            )
        });

    let scenarios = vec![
        MockMetricsProgressScenario::Immediate,
        MockMetricsProgressScenario::Immediate,
    ];
    let services = create_test_services_with_progressive_mocks(2, scenarios)?;

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    // This should return a BatchResult with errors
    let result = batch_manager.start_all(1000, true).await;
    assert!(!result.errors.is_empty());

    Ok(())
}

#[tokio::test]
async fn start_all_should_start_user_mode_services() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up expectations for 2 user mode services
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
        .times(2)
        .returning(|_| ());

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

    // Create services and set them to user mode
    let services = create_test_services_with_mocks(2)?;
    for service in &services {
        let mut service_data = service.service_data.write().await;
        service_data.user_mode = true;
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let batch_result = batch_manager.start_all(1000, true).await;
    assert!(batch_result.errors.is_empty());

    // Verify services are in running state
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Running);
        assert!(service.pid().await.is_some());
    }

    Ok(())
}

#[tokio::test]
async fn start_all_should_monitor_progress_until_all_services_complete() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up expectations for 2 services with different progress rates
    for i in 1..=2 {
        mock_service_control
            .expect_start()
            .with(eq(format!("antnode{i}")), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        mock_service_control
            .expect_get_process_pid()
            .with(eq(PathBuf::from(format!(
                "/var/antctl/services/antnode{i}/antnode"
            ))))
            .times(1)
            .returning(move |_| Ok(1000 + i));
    }

    mock_service_control
        .expect_wait()
        .with(eq(1000))
        .times(2)
        .returning(|_| ());

    // First service completes quickly, second takes multiple steps
    let scenarios = vec![
        MockMetricsProgressScenario::Staged(vec![50, 100]), // 2 steps
        MockMetricsProgressScenario::Staged(vec![10, 30, 60, 80, 95, 100]), // 6 steps
    ];
    let services = create_test_services_with_progressive_mocks(2, scenarios)?;

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let batch_result = batch_manager.start_all(1000, true).await;
    assert!(batch_result.errors.is_empty());

    // Both services should be running after the progress monitoring completes
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Running);
        assert!(service.pid().await.is_some());
    }

    Ok(())
}

#[tokio::test]
async fn start_all_should_respect_fixed_intervals() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up expectations for 3 services
    for i in 1..=3 {
        mock_service_control
            .expect_start()
            .with(eq(format!("antnode{i}")), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        mock_service_control
            .expect_get_process_pid()
            .with(eq(PathBuf::from(format!(
                "/var/antctl/services/antnode{i}/antnode"
            ))))
            .times(1)
            .returning(move |_| Ok(1000 + i));
    }

    // The wait should be called with the specified interval for each service
    mock_service_control
        .expect_wait()
        .with(eq(1000)) // 1 second interval
        .times(3)
        .returning(|_| ());

    // Create services with immediate completion for timing tests
    let scenarios = vec![
        MockMetricsProgressScenario::Immediate,
        MockMetricsProgressScenario::Immediate,
        MockMetricsProgressScenario::Immediate,
    ];
    let services = create_test_services_with_progressive_mocks(3, scenarios)?;

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let start_time = Instant::now();
    let batch_result = batch_manager.start_all(1000, true).await;
    let _elapsed = start_time.elapsed();

    assert!(batch_result.errors.is_empty());

    // Should take at least some time due to the intervals, but not too long due to mocked waits
    // The actual timing is mocked, but we can verify the services are all started properly
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Running);
        assert!(service.pid().await.is_some());
    }

    Ok(())
}

#[tokio::test]
async fn start_all_should_handle_services_starting_at_different_rates() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up expectations for services
    for i in 1..=3 {
        mock_service_control
            .expect_start()
            .with(eq(format!("antnode{i}")), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        mock_service_control
            .expect_get_process_pid()
            .with(eq(PathBuf::from(format!(
                "/var/antctl/services/antnode{i}/antnode"
            ))))
            .times(1)
            .returning(move |_| Ok(1000 + i));
    }

    mock_service_control
        .expect_wait()
        .with(eq(1000))
        .times(3)
        .returning(|_| ());

    // Create services with different startup characteristics
    let scenarios = vec![
        MockMetricsProgressScenario::Immediate, // Fast starter
        MockMetricsProgressScenario::Staged(vec![0, 25, 50, 75, 100]), // Gradual starter
        MockMetricsProgressScenario::Staged(vec![0, 0, 0, 100]), // Slow starter
    ];
    let services = create_test_services_with_progressive_mocks(3, scenarios)?;

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let batch_result = batch_manager.start_all(1000, true).await;
    assert!(batch_result.errors.is_empty());

    // All services should eventually reach running state
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Running);
        assert!(service.pid().await.is_some());
    }

    Ok(())
}

#[tokio::test]
async fn start_all_should_continue_with_other_services_when_one_fails() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // First service fails to start
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| {
            Err(ant_service_management::error::Error::Io(
                std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Service start failed"),
            ))
        });

    // Second and third services start successfully
    for i in 2..=3 {
        mock_service_control
            .expect_start()
            .with(eq(format!("antnode{i}")), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        mock_service_control
            .expect_get_process_pid()
            .with(eq(PathBuf::from(format!(
                "/var/antctl/services/antnode{i}/antnode"
            ))))
            .times(1)
            .returning(move |_| Ok(1000 + i));
    }

    mock_service_control
        .expect_wait()
        .with(eq(1000))
        .times(2) // Only for successful services
        .returning(|_| ());

    let scenarios = vec![
        MockMetricsProgressScenario::NeverCalled, // First service fails to start, so progress monitoring never begins
        MockMetricsProgressScenario::Immediate,
        MockMetricsProgressScenario::Immediate,
    ];
    let services = create_test_services_with_progressive_mocks(3, scenarios)?;

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let batch_result = batch_manager.start_all(1000, true).await;

    // Should have error for first service
    assert_eq!(batch_result.errors.len(), 1);
    assert!(batch_result.errors.contains_key("antnode1"));

    // Services 2 and 3 should be running
    for i in 1..3 {
        let service = &batch_manager.services[i];
        assert_matches!(service.status().await, ServiceStatus::Running);
        assert!(service.pid().await.is_some());
    }

    Ok(())
}

#[tokio::test]
async fn start_all_should_handle_mixed_user_and_system_modes() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // First service in user mode
    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(true))
        .times(1)
        .returning(|_, _| Ok(()));

    // Second service in system mode
    mock_service_control
        .expect_start()
        .with(eq("antnode2"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

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
        .expect_wait()
        .with(eq(1000))
        .times(2)
        .returning(|_| ());

    // Create services explicitly: one in user mode, one in system mode
    let mut services = Vec::new();
    for i in 1..=2 {
        let mut mock_fs_client = MockFileSystemClient::new();
        let mut mock_metrics_client = MockMetricsClient::new();

        // File system mock for listening addresses retrieval after successful startup
        mock_fs_client
            .expect_listen_addrs()
            .with(eq(PathBuf::from(format!(
                "/var/antctl/services/antnode{i}"
            ))))
            .times(1)
            .returning(|_| Ok(vec![]));

        mock_metrics_client
            .expect_get_node_metrics()
            .times(1..)
            .returning(|| {
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

        mock_metrics_client
            .expect_get_node_metadata_extended()
            .times(1)
            .returning(move || {
                Ok(ant_service_management::metric::NodeMetadataExtended {
                    pid: 1000 + i,
                    peer_id: get_test_peer_id((i - 1) as usize),
                    root_dir: PathBuf::from(format!("/var/antctl/services/antnode{i}")),
                    log_dir: PathBuf::from(format!("/var/log/antnode/antnode{i}")),
                })
            });

        let mut service_data = create_test_service_data(i as u16);
        service_data.user_mode = i == 1; // First service in user mode, second in system mode
        let service_data = Arc::new(RwLock::new(service_data));

        let service = NodeService::new(
            service_data,
            Box::new(mock_fs_client),
            Box::new(mock_metrics_client),
        );
        services.push(service);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let batch_result = batch_manager.start_all(1000, true).await;
    assert!(batch_result.errors.is_empty());

    // Both services should be running
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Running);
        assert!(service.pid().await.is_some());
    }

    Ok(())
}

#[tokio::test]
async fn start_all_should_skip_already_running_services_and_start_others() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // First service is already running - should check PID and skip
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| Ok(1001));

    // Second service needs to be started
    mock_service_control
        .expect_start()
        .with(eq("antnode2"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    mock_service_control
        .expect_wait()
        .with(eq(1000))
        .times(1) // Only for service that gets started
        .returning(|_| ());

    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode2/antnode")))
        .times(1)
        .returning(|_| Ok(1002));

    let scenarios = vec![
        MockMetricsProgressScenario::NeverCalled, // First service is already running, so progress monitoring never begins
        MockMetricsProgressScenario::Immediate,
    ];
    let services = create_test_services_with_progressive_mocks(2, scenarios)?;

    // Set first service to running
    {
        let mut service_data = services[0].service_data.write().await;
        service_data.status = ServiceStatus::Running;
        service_data.pid = Some(1001);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let batch_result = batch_manager.start_all(1000, true).await;
    assert!(batch_result.errors.is_empty());

    // Both services should be running
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Running);
        assert!(service.pid().await.is_some());
    }

    Ok(())
}

#[tokio::test]
async fn start_all_should_record_critical_failure_in_registry() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

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
        .times(2)
        .returning(|_| ());

    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| Ok(1001));

    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode2/antnode")))
        .times(0..=1)
        .returning(|_| Ok(1002));

    let mut services = Vec::new();

    {
        let mut mock_fs_client = MockFileSystemClient::new();
        let mut mock_metrics_client = MockMetricsClient::new();

        mock_metrics_client
            .expect_get_node_metrics()
            .times(1..)
            .returning(|| {
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

        mock_metrics_client
            .expect_get_node_metadata_extended()
            .times(1)
            .returning(|| {
                Ok(ant_service_management::metric::NodeMetadataExtended {
                    pid: 1001,
                    peer_id: get_test_peer_id(0),
                    root_dir: PathBuf::from("/var/antctl/services/antnode1"),
                    log_dir: PathBuf::from("/var/log/antnode/antnode1"),
                })
            });

        mock_fs_client
            .expect_listen_addrs()
            .times(1)
            .returning(|_| Ok(vec![]));

        let service_data = create_test_service_data(1);
        let service_data = Arc::new(RwLock::new(service_data));

        let service = NodeService::new(
            service_data,
            Box::new(mock_fs_client),
            Box::new(mock_metrics_client),
        );
        services.push(service);
    }

    {
        let mut mock_fs_client = MockFileSystemClient::new();
        let mut mock_metrics_client = MockMetricsClient::new();

        mock_metrics_client
            .expect_get_node_metrics()
            .times(1)
            .returning(|| {
                Ok(ant_service_management::metric::NodeMetrics {
                    reachability_status: ant_service_management::metric::ReachabilityStatusValues {
                        progress: ReachabilityProgress::Complete,
                        upnp: false,
                        public: false,
                        private: false,
                    },
                    connected_peers: 0,
                })
            });

        mock_fs_client
            .expect_critical_failure()
            .with(eq(PathBuf::from("/var/antctl/services/antnode2")))
            .times(1)
            .returning(|_| {
                Ok(Some(ant_service_management::fs::CriticalFailure {
                    date_time: chrono::Utc::now(),
                    reason: "Unreachable".to_string(),
                }))
            });

        let mut service_data = create_test_service_data(2);
        service_data.last_critical_failure = None;
        let service_data = Arc::new(RwLock::new(service_data));

        let service = NodeService::new(
            service_data,
            Box::new(mock_fs_client),
            Box::new(mock_metrics_client),
        );
        services.push(service);
    }

    let registry = create_test_registry();
    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        registry.clone(),
        VerbosityLevel::Normal,
    );

    let batch_result = batch_manager.start_all(1000, true).await;

    assert_eq!(batch_result.errors.len(), 1);
    assert!(batch_result.errors.contains_key("antnode2"));

    {
        let service1_data = batch_manager.services[0].service_data.read().await;
        assert!(service1_data.last_critical_failure.is_none());
        assert_eq!(service1_data.status, ServiceStatus::Running);
    }

    {
        let service2_data = batch_manager.services[1].service_data.read().await;
        assert!(service2_data.last_critical_failure.is_some());
        let critical_failure = service2_data.last_critical_failure.as_ref().unwrap();
        assert_eq!(critical_failure.reason, "Unreachable");
    }

    Ok(())
}

#[tokio::test]
async fn start_all_should_clear_critical_failure_on_success() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    mock_service_control
        .expect_start()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    mock_service_control
        .expect_wait()
        .with(eq(1000))
        .times(1)
        .returning(|_| ());

    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| Ok(1001));

    let mut mock_fs_client = MockFileSystemClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    mock_metrics_client
        .expect_get_node_metrics()
        .times(1..)
        .returning(|| {
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

    mock_metrics_client
        .expect_get_node_metadata_extended()
        .times(1)
        .returning(|| {
            Ok(ant_service_management::metric::NodeMetadataExtended {
                pid: 1001,
                peer_id: get_test_peer_id(0),
                root_dir: PathBuf::from("/var/antctl/services/antnode1"),
                log_dir: PathBuf::from("/var/log/antnode/antnode1"),
            })
        });

    mock_fs_client
        .expect_listen_addrs()
        .times(1)
        .returning(|_| Ok(vec![]));

    let mut service_data = create_test_service_data(1);
    service_data.last_critical_failure = Some(ant_service_management::fs::CriticalFailure {
        date_time: chrono::Utc::now(),
        reason: "PreviousStartupFailure".to_string(),
    });
    let service_data = Arc::new(RwLock::new(service_data));

    {
        let data = service_data.read().await;
        assert!(data.last_critical_failure.is_some());
        assert_eq!(
            data.last_critical_failure.as_ref().unwrap().reason,
            "PreviousStartupFailure"
        );
    }

    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_fs_client),
        Box::new(mock_metrics_client),
    );

    let services = vec![service];

    let registry = create_test_registry();
    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        registry,
        VerbosityLevel::Normal,
    );

    let batch_result = batch_manager.start_all(1000, true).await;

    assert!(batch_result.errors.is_empty());

    {
        let data = service_data.read().await;
        assert!(data.last_critical_failure.is_none());
        assert_eq!(data.status, ServiceStatus::Running);
        assert_eq!(data.reachability_progress, ReachabilityProgress::Complete);
    }

    Ok(())
}

#[tokio::test]
async fn start_all_should_preserve_critical_failure_when_not_full_refresh() -> Result<()> {
    let mock_fs_client = MockFileSystemClient::new();
    let mock_metrics_client = MockMetricsClient::new();

    let mut service_data = create_test_service_data(1);
    service_data.last_critical_failure = Some(ant_service_management::fs::CriticalFailure {
        date_time: chrono::Utc::now(),
        reason: "ExistingFailure".to_string(),
    });

    let service_data = Arc::new(RwLock::new(service_data));

    let service = NodeService::new(
        Arc::clone(&service_data),
        Box::new(mock_fs_client),
        Box::new(mock_metrics_client),
    );

    service.on_start(Some(1234), false).await?;

    let data = service_data.read().await;
    assert!(data.last_critical_failure.is_some());
    assert_eq!(
        data.last_critical_failure.as_ref().unwrap().reason,
        "ExistingFailure"
    );
    assert_eq!(data.pid, Some(1234));
    assert_eq!(data.status, ServiceStatus::Running);

    Ok(())
}

#[tokio::test]
async fn start_all_should_update_critical_failure_during_full_refresh() -> Result<()> {
    {
        let mut mock_fs_client = MockFileSystemClient::new();
        let mut mock_metrics_client = MockMetricsClient::new();

        mock_metrics_client
            .expect_get_node_metrics()
            .times(1)
            .returning(|| {
                Ok(ant_service_management::metric::NodeMetrics {
                    reachability_status: ant_service_management::metric::ReachabilityStatusValues {
                        progress: ReachabilityProgress::Complete,
                        upnp: false,
                        public: false,
                        private: false,
                    },
                    connected_peers: 0,
                })
            });

        mock_fs_client
            .expect_critical_failure()
            .with(eq(PathBuf::from("/var/antctl/services/antnode1")))
            .times(1)
            .returning(|_| {
                Ok(Some(ant_service_management::fs::CriticalFailure {
                    date_time: chrono::Utc::now(),
                    reason: "NetworkUnreachable".to_string(),
                }))
            });

        mock_metrics_client
            .expect_get_node_metadata_extended()
            .times(1)
            .returning(|| {
                Ok(ant_service_management::metric::NodeMetadataExtended {
                    pid: 1234,
                    peer_id: get_test_peer_id(0),
                    root_dir: PathBuf::from("/var/antctl/services/antnode1"),
                    log_dir: PathBuf::from("/var/log/antnode/antnode1"),
                })
            });

        mock_fs_client
            .expect_listen_addrs()
            .times(1)
            .returning(|_| Ok(vec![]));

        let mut service_data = create_test_service_data(1);
        service_data.last_critical_failure = None;
        let service_data = Arc::new(RwLock::new(service_data));

        let service = NodeService::new(
            Arc::clone(&service_data),
            Box::new(mock_fs_client),
            Box::new(mock_metrics_client),
        );

        service.on_start(Some(1234), true).await?;

        let data = service_data.read().await;
        assert!(data.last_critical_failure.is_some());
        assert_eq!(
            data.last_critical_failure.as_ref().unwrap().reason,
            "NetworkUnreachable"
        );
    }

    {
        let mut mock_fs_client = MockFileSystemClient::new();
        let mut mock_metrics_client = MockMetricsClient::new();

        mock_metrics_client
            .expect_get_node_metrics()
            .times(1)
            .returning(|| {
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

        mock_metrics_client
            .expect_get_node_metadata_extended()
            .times(1)
            .returning(|| {
                Ok(ant_service_management::metric::NodeMetadataExtended {
                    pid: 1235,
                    peer_id: get_test_peer_id(1),
                    root_dir: PathBuf::from("/var/antctl/services/antnode2"),
                    log_dir: PathBuf::from("/var/log/antnode/antnode2"),
                })
            });

        mock_fs_client
            .expect_listen_addrs()
            .times(1)
            .returning(|_| Ok(vec![]));

        let mut service_data = create_test_service_data(2);
        service_data.last_critical_failure = Some(ant_service_management::fs::CriticalFailure {
            date_time: chrono::Utc::now(),
            reason: "PreviousFailure".to_string(),
        });
        let service_data = Arc::new(RwLock::new(service_data));

        let service = NodeService::new(
            Arc::clone(&service_data),
            Box::new(mock_fs_client),
            Box::new(mock_metrics_client),
        );

        service.on_start(Some(1235), true).await?;

        let data = service_data.read().await;
        assert!(data.last_critical_failure.is_none());
        assert_eq!(data.reachability_progress, ReachabilityProgress::Complete);
    }

    Ok(())
}
