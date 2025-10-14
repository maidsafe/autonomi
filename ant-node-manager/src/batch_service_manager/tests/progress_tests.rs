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
    NodeService, ServiceStateActions, ServiceStatus, fs::CriticalFailure,
    metric::NodeMetadataExtended,
};
use assert_matches::assert_matches;
use chrono::Utc;
use color_eyre::eyre::Result;
use mockall::predicate::*;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::RwLock;

#[tokio::test]
async fn start_all_should_handle_incremental_progress() -> Result<()> {
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

    let mut services = Vec::new();

    {
        let mut mock_fs_client = MockFileSystemClient::new();
        let mut mock_metrics_client = MockMetricsClient::new();

        mock_fs_client
            .expect_listen_addrs()
            .times(1..=2)
            .returning(|_| Ok(vec![]));

        mock_metrics_client
            .expect_get_node_metadata_extended()
            .times(1..=2)
            .returning(|| {
                Ok(NodeMetadataExtended {
                    pid: 1001,
                    peer_id: get_test_peer_id(0),
                    root_dir: PathBuf::from("/var/antctl/services/antnode1"),
                    log_dir: PathBuf::from("/var/log/antnode/antnode1"),
                })
            });

        setup_incremental_progress_mock(&mut mock_metrics_client, 0, 50);

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

        mock_fs_client
            .expect_listen_addrs()
            .times(1..=2)
            .returning(|_| Ok(vec![]));

        mock_metrics_client
            .expect_get_node_metadata_extended()
            .times(1..=2)
            .returning(|| {
                Ok(NodeMetadataExtended {
                    pid: 1002,
                    peer_id: get_test_peer_id(1),
                    root_dir: PathBuf::from("/var/antctl/services/antnode2"),
                    log_dir: PathBuf::from("/var/log/antnode/antnode2"),
                })
            });

        setup_incremental_progress_mock(&mut mock_metrics_client, 25, 25);

        let service_data = create_test_service_data(2);
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
async fn start_all_should_handle_mixed_progress_rates() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up expectations for 4 services with different progress rates
    for i in 1..=4 {
        mock_service_control
            .expect_start()
            .with(eq(format!("antnode{i}")), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        if i != 3 {
            mock_service_control
                .expect_get_process_pid()
                .with(eq(PathBuf::from(format!(
                    "/var/antctl/services/antnode{i}/antnode"
                ))))
                .times(1)
                .returning(move |_| Ok(1000 + i));
        }
    }

    mock_service_control
        .expect_wait()
        .with(eq(1000))
        .times(4)
        .returning(|_| ());

    let mut services = Vec::new();

    {
        let mut mock_fs_client = MockFileSystemClient::new();
        let mut mock_metrics_client = MockMetricsClient::new();

        mock_fs_client
            .expect_listen_addrs()
            .times(1..=2)
            .returning(|_| Ok(vec![]));

        mock_metrics_client
            .expect_get_node_metadata_extended()
            .times(1..=2)
            .returning(|| {
                Ok(NodeMetadataExtended {
                    pid: 1001,
                    peer_id: get_test_peer_id(0),
                    root_dir: PathBuf::from("/var/antctl/services/antnode1"),
                    log_dir: PathBuf::from("/var/log/antnode/antnode1"),
                })
            });

        setup_incremental_progress_mock(&mut mock_metrics_client, 0, 25);

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

        mock_fs_client
            .expect_listen_addrs()
            .times(1..=2)
            .returning(|_| Ok(vec![]));

        mock_metrics_client
            .expect_get_node_metadata_extended()
            .times(1..=2)
            .returning(|| {
                Ok(NodeMetadataExtended {
                    pid: 1002,
                    peer_id: get_test_peer_id(1),
                    root_dir: PathBuf::from("/var/antctl/services/antnode2"),
                    log_dir: PathBuf::from("/var/log/antnode/antnode2"),
                })
            });

        setup_staged_progress_mock(&mut mock_metrics_client, vec![10, 50, 80, 100]);

        let service_data = create_test_service_data(2);
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

        mock_fs_client
            .expect_listen_addrs()
            .times(1)
            .returning(|_| Ok(vec![]));

        mock_metrics_client
            .expect_get_node_metadata_extended()
            .times(1)
            .returning(|| {
                Ok(NodeMetadataExtended {
                    pid: 1003,
                    peer_id: get_test_peer_id(2),
                    root_dir: PathBuf::from("/var/antctl/services/antnode3"),
                    log_dir: PathBuf::from("/var/log/antnode/antnode3"),
                })
            });

        setup_stuck_progress_mock(&mut mock_metrics_client, 75);

        let service_data = create_test_service_data(3);
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

        mock_fs_client
            .expect_listen_addrs()
            .times(1..=2)
            .returning(|_| Ok(vec![]));

        mock_metrics_client
            .expect_get_node_metadata_extended()
            .times(1..=2)
            .returning(|| {
                Ok(NodeMetadataExtended {
                    pid: 1004,
                    peer_id: get_test_peer_id(3),
                    root_dir: PathBuf::from("/var/antctl/services/antnode4"),
                    log_dir: PathBuf::from("/var/log/antnode/antnode4"),
                })
            });

        setup_staged_progress_mock(&mut mock_metrics_client, vec![100]);

        let service_data = create_test_service_data(4);
        let service_data = Arc::new(RwLock::new(service_data));
        let service = NodeService::new(
            service_data,
            Box::new(mock_fs_client),
            Box::new(mock_metrics_client),
        );
        services.push(service);
    }

    let registry = create_test_registry();
    let mut batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        registry,
        VerbosityLevel::Normal,
    );
    batch_manager.set_progress_timeout(Duration::from_secs(5));

    let batch_result = batch_manager.start_all(1000, true).await;

    assert_eq!(batch_result.errors.len(), 1);
    for (service_name, errors) in &batch_result.errors {
        assert_eq!(service_name, "antnode3");
        for error in errors {
            if error.to_string().contains("timed out") {
                println!("Service {service_name} timed out as expected: {error}");
            } else {
                panic!("Unexpected error for service {service_name}: {error}");
            }
        }
    }

    Ok(())
}

#[tokio::test]
async fn start_all_should_handle_progress_failures() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Both services start successfully but metrics fail
    for i in 1..=2 {
        mock_service_control
            .expect_start()
            .with(eq(format!("antnode{i}")), eq(false))
            .times(1)
            .returning(|_, _| Ok(()));

        // Services might try to get PID before failing
        mock_service_control
            .expect_get_process_pid()
            .with(eq(PathBuf::from(format!(
                "/var/antctl/services/antnode{i}/antnode"
            ))))
            .times(0..=1)
            .returning(move |_| Ok(1000 + i));
    }

    mock_service_control
        .expect_wait()
        .with(eq(1000))
        .times(2)
        .returning(|_| ());

    // Create services that will fail when metrics are requested during progress monitoring
    let mut services = Vec::new();
    for i in 1..=2 {
        let mut mock_fs_client = MockFileSystemClient::new();
        let mut mock_metrics_client = MockMetricsClient::new();

        // Expect exactly 1 call to get_node_metrics, which will fail
        mock_metrics_client
            .expect_get_node_metrics()
            .times(1..=2)
            .returning(|| {
                Err(
                    ant_service_management::metric::MetricsActionError::ConnectionError(
                        "Mock RPC connection failure during progress monitoring".to_string(),
                    ),
                )
            });

        mock_fs_client
            .expect_critical_failure()
            .with(eq(PathBuf::from(format!(
                "/var/antctl/services/antnode{i}"
            ))))
            .times(1..=2)
            .returning(|_| {
                Ok(Some(CriticalFailure {
                    date_time: Utc::now(),
                    reason: "Unreachable".to_string(),
                }))
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

    // Should have errors for both services since they fail RPC calls
    assert_eq!(batch_result.errors.len(), 2);
    assert!(batch_result.errors.contains_key("antnode1"));
    assert!(batch_result.errors.contains_key("antnode2"));

    let expected_error = crate::error::Error::ServiceStartupFailed {
        service_name: "antnode1".to_string(),
        reason: "Unreachable".to_string(),
    };
    let actual_error = batch_result.get_errors("antnode1").unwrap();
    assert_eq!(format!("{actual_error:?}",), format!("{expected_error:?}",));

    let expected_error = crate::error::Error::ServiceStartupFailed {
        service_name: "antnode2".to_string(),
        reason: "Unreachable".to_string(),
    };
    let actual_error = batch_result.get_errors("antnode2").unwrap();
    assert_eq!(format!("{actual_error:?}",), format!("{expected_error:?}",));

    for service in &batch_manager.services {
        let service_data = service.service_data.read().await;
        assert!(service_data.last_critical_failure.is_some());
        let critical_failure = service_data.last_critical_failure.as_ref().unwrap();
        assert_eq!(critical_failure.reason, "Unreachable");
    }

    Ok(())
}

#[tokio::test]
async fn start_all_should_handle_intermittent_progress_failures() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Service starts successfully
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
        .times(0..=1)
        .returning(|_| Ok(1001));

    // Create single service that fails during progress monitoring
    let mut services = Vec::new();
    let mut mock_fs_client = MockFileSystemClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // Expect exactly 1 call to get_node_metrics, which will fail
    mock_metrics_client
        .expect_get_node_metrics()
        .times(1..=2)
        .returning(|| {
            Err(
                ant_service_management::metric::MetricsActionError::ConnectionError(
                    "Mock RPC connection failure during progress monitoring".to_string(),
                ),
            )
        });

    // A failure should then trigger us to read the critical failure
    mock_fs_client
        .expect_critical_failure()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1")))
        .times(1..=2)
        .returning(|_| {
            Ok(Some(CriticalFailure {
                date_time: Utc::now(),
                reason: "Unreachable".to_string(),
            }))
        });

    let service_data = create_test_service_data(1);
    let service_data = Arc::new(RwLock::new(service_data));

    let service = NodeService::new(
        service_data,
        Box::new(mock_fs_client),
        Box::new(mock_metrics_client),
    );
    services.push(service);

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let batch_result = batch_manager.start_all(1000, true).await;

    // Should have error for the service that failed progress
    assert_eq!(batch_result.errors.len(), 1);
    assert!(batch_result.errors.contains_key("antnode1"));
    let expected_error = crate::error::Error::ServiceStartupFailed {
        service_name: "antnode1".to_string(),
        reason: "Unreachable".to_string(),
    };
    let actual_error = batch_result.get_errors("antnode1").unwrap();
    assert_eq!(format!("{actual_error:?}",), format!("{expected_error:?}",));

    let service1_data = batch_manager.services[0].service_data.read().await;
    assert!(service1_data.last_critical_failure.is_some());
    let critical_failure = service1_data.last_critical_failure.as_ref().unwrap();
    assert_eq!(critical_failure.reason, "Unreachable");

    Ok(())
}

#[tokio::test]
async fn start_all_should_handle_staged_progress() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up service control expectations
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

    let services = create_test_services_with_mocks(1)?;

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let batch_result = batch_manager.start_all(1000, true).await;
    assert!(batch_result.errors.is_empty());

    // Verify service reaches running state
    let service = &batch_manager.services[0];
    assert_matches!(service.status().await, ServiceStatus::Running);
    assert!(service.pid().await.is_some());

    Ok(())
}

#[tokio::test]
async fn start_all_should_wait_for_all_services_to_complete() -> Result<()> {
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

    mock_service_control
        .expect_wait()
        .with(eq(1000))
        .times(3)
        .returning(|_| ());

    let services = create_test_services_with_mocks(3)?;

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let batch_result = batch_manager.start_all(1000, true).await;
    assert!(batch_result.errors.is_empty());

    // All services should be running
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Running);
        assert!(service.pid().await.is_some());
    }

    Ok(())
}

#[tokio::test]
async fn start_all_should_timeout_stuck_services() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up expectations for service that will get stuck
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

    // Service doesn't reach PID check because it times out during progress monitoring

    // Create a service that gets stuck at 50% progress (will timeout)

    let mut services = Vec::new();

    let mut mock_fs_client = MockFileSystemClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // Set up minimal file system mock (called once during initial collection)
    mock_fs_client
        .expect_listen_addrs()
        .times(1)
        .returning(|_| Ok(vec![]));

    // Set up minimal metadata mock (called once during initial collection)
    mock_metrics_client
        .expect_get_node_metadata_extended()
        .times(1)
        .returning(|| {
            Ok(NodeMetadataExtended {
                pid: 1001,
                peer_id: get_test_peer_id(0),
                root_dir: PathBuf::from("/var/antctl/services/antnode1"),
                log_dir: PathBuf::from("/var/log/antnode/antnode1"),
            })
        });

    setup_stuck_progress_mock(&mut mock_metrics_client, 50);

    let service_data = create_test_service_data(1);
    let service_data = Arc::new(RwLock::new(service_data));

    let service = NodeService::new(
        service_data,
        Box::new(mock_fs_client),
        Box::new(mock_metrics_client),
    );

    services.push(service);

    let registry = create_test_registry();
    let mut batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        registry,
        VerbosityLevel::Normal,
    );
    batch_manager.set_progress_timeout(Duration::from_secs(2));

    // Use a very short timeout for testing
    let batch_result = batch_manager.start_all(1000, true).await;

    // Should have timeout error
    assert_eq!(batch_result.errors.len(), 1);
    assert!(batch_result.errors.contains_key("antnode1"));

    // Verify the error is a timeout error
    let error = batch_result.get_errors("antnode1").unwrap();
    assert!(error.to_string().contains("timed out"));

    Ok(())
}
