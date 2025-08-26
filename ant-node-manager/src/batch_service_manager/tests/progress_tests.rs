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
    NodeService, ServiceStateActions, ServiceStatus,
    metric::{NodeMetrics, ReachabilityStatusValues},
};
use assert_matches::assert_matches;
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

    // Create services with incremental progress
    let scenarios = vec![
        MockMetricsProgressScenario::Incremental {
            start: 0,
            increment: 50,
            max: 100,
        },
        MockMetricsProgressScenario::Incremental {
            start: 25,
            increment: 25,
            max: 100,
        },
    ];
    let services = create_test_services_with_progressive_mocks(2, scenarios)?;

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let batch_result = batch_manager.start_all(1000).await;
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
        .with(eq(500))
        .times(4)
        .returning(|_| ());

    let scenarios = vec![
        MockMetricsProgressScenario::Incremental {
            start: 0,
            increment: 25,
            max: 100,
        },
        MockMetricsProgressScenario::Staged(vec![10, 50, 80, 100]),
        MockMetricsProgressScenario::StuckAt(75),
        MockMetricsProgressScenario::Immediate,
    ];

    let services = create_test_services_with_progressive_mocks(4, scenarios)?;

    let registry = create_test_registry();
    let mut batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        registry,
        VerbosityLevel::Normal,
    );
    batch_manager.set_progress_timeout(Duration::from_secs(5));

    let batch_result = batch_manager.start_all(500).await;

    assert_eq!(batch_result.errors.len(), 1);
    // only the stuck service should have a timeout error
    for (service_name, errors) in &batch_result.errors {
        assert_eq!(service_name, "antnode3");
        for error in errors {
            if error.to_string().contains("timed out") {
                println!("Service {service_name} timed out as expected: {error}");
            } else {
                // If there are non-timeout errors, the test should fail
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
        let mock_fs_client = MockFileSystemClient::new();
        let mut mock_metrics_client = MockMetricsClient::new();

        // Expect exactly 1 call to get_node_metrics, which will fail
        mock_metrics_client
            .expect_get_node_metrics()
            .times(1)
            .returning(|| {
                Err(
                    ant_service_management::metric::MetricsActionError::ConnectionError(
                        "Mock RPC connection failure during progress monitoring".to_string(),
                    ),
                )
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

    let batch_result = batch_manager.start_all(1000).await;

    // Should have errors for both services since they fail RPC calls
    assert_eq!(batch_result.errors.len(), 2);
    assert!(batch_result.errors.contains_key("antnode1"));
    assert!(batch_result.errors.contains_key("antnode2"));

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
    let mock_fs_client = MockFileSystemClient::new();
    let mut mock_metrics_client = MockMetricsClient::new();

    // Expect exactly 1 call to get_node_metrics, which will fail
    mock_metrics_client
        .expect_get_node_metrics()
        .times(1)
        .returning(|| {
            Err(
                ant_service_management::metric::MetricsActionError::ConnectionError(
                    "Mock RPC connection failure during progress monitoring".to_string(),
                ),
            )
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

    let batch_result = batch_manager.start_all(1000).await;

    // Should have error for the service that failed progress
    assert_eq!(batch_result.errors.len(), 1);
    assert!(batch_result.errors.contains_key("antnode1"));

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

    // Create service with staged progress: 0% -> 25% -> 50% -> 75% -> 100%
    let scenarios = vec![MockMetricsProgressScenario::Staged(vec![
        0, 25, 50, 75, 100,
    ])];
    let services = create_test_services_with_progressive_mocks(1, scenarios)?;

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let batch_result = batch_manager.start_all(1000).await;
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
        .with(eq(100))
        .times(3)
        .returning(|_| ());

    // Create services with different completion times
    let scenarios = vec![
        MockMetricsProgressScenario::Staged(vec![0, 100]), // Fast completion
        MockMetricsProgressScenario::Staged(vec![0, 30, 60, 100]), // Medium completion
        MockMetricsProgressScenario::Staged(vec![0, 20, 40, 60, 80, 100]), // Slow completion
    ];
    let services = create_test_services_with_progressive_mocks(3, scenarios)?;

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let batch_result = batch_manager.start_all(100).await;
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

    // Set up minimal file system mock (won't be reached due to timeout)
    mock_fs_client.expect_node_info().times(0);

    // Set up minimal metadata mock (won't be reached due to timeout)
    mock_metrics_client
        .expect_get_node_metadata_extended()
        .times(0);

    mock_metrics_client
        .expect_get_node_metrics()
        .times(1..)
        .returning(|| {
            Ok(NodeMetrics {
                reachability_status: ReachabilityStatusValues {
                    progress_percent: 50, // Always stuck at 50%
                    upnp: false,
                    public: true,
                    private: false,
                },
                connected_peers: 10,
            })
        });

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
    let batch_result = batch_manager.start_all(1000).await;

    // Should have timeout error
    assert_eq!(batch_result.errors.len(), 1);
    assert!(batch_result.errors.contains_key("antnode1"));

    // Verify the error is a timeout error
    let error = batch_result.get_errors("antnode1").unwrap();
    assert!(error.to_string().contains("timed out"));

    Ok(())
}
