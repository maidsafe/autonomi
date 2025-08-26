// Copyright (C) 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::helpers::*;
use crate::batch_service_manager::{BatchServiceManager, VerbosityLevel};
use ant_service_management::{ServiceStateActions, ServiceStatus};
use assert_matches::assert_matches;
use color_eyre::eyre::Result;
use mockall::predicate::*;
use std::path::PathBuf;

#[tokio::test]
async fn stop_all_should_stop_running_services() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up expectations for checking if processes are running
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

    // Set up expectations for stopping services
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

    let batch_result = batch_manager.stop_all(None).await;
    assert!(batch_result.errors.is_empty());

    // Verify services are in stopped state
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Stopped);
        assert!(service.pid().await.is_none());
    }

    Ok(())
}

#[tokio::test]
async fn stop_all_should_not_error_for_installed_services() -> Result<()> {
    let mock_service_control = MockServiceControl::new();

    // No service control calls should be made for added services

    // Create services in Added state (default)
    let services = create_test_services_simple(2);

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    // This should not error
    let batch_result = batch_manager.stop_all(None).await;
    assert!(batch_result.errors.is_empty());

    // Verify services remain in Added state
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Added);
    }

    Ok(())
}

#[tokio::test]
async fn stop_all_should_handle_already_stopped_services() -> Result<()> {
    let mock_service_control = MockServiceControl::new();

    // No service control calls should be made for stopped services

    // Create services and set them to stopped state
    let services = create_test_services_simple(2);
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

    let batch_result = batch_manager.stop_all(None).await;
    assert!(batch_result.errors.is_empty());

    // Verify services remain in stopped state
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Stopped);
    }

    Ok(())
}

#[tokio::test]
async fn stop_all_should_handle_removed_services() -> Result<()> {
    let mock_service_control = MockServiceControl::new();

    // No service control calls should be made for removed services

    // Create services and set them to removed state
    let services = create_test_services_simple(2);
    for service in &services {
        let mut service_data = service.service_data.write().await;
        service_data.status = ServiceStatus::Removed;
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let batch_result = batch_manager.stop_all(None).await;
    assert!(batch_result.errors.is_empty());

    // Verify services remain in removed state
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Removed);
    }

    Ok(())
}

#[tokio::test]
async fn stop_all_should_stop_user_mode_services() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up expectations for checking if processes are running
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

    // Set up expectations for stopping user mode services
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

    // Create services and set them to running state with user mode
    let services = create_test_services_simple(2);
    for service in &services {
        let mut service_data = service.service_data.write().await;
        service_data.status = ServiceStatus::Running;
        service_data.user_mode = true;
        service_data.pid = Some(1001);
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let batch_result = batch_manager.stop_all(None).await;
    assert!(batch_result.errors.is_empty());

    // Verify services are in stopped state
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Stopped);
        assert!(service.pid().await.is_none());
    }

    Ok(())
}

#[tokio::test]
async fn stop_all_with_interval_should_delay_between_stops() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up expectations for checking if processes are running
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

    // Set up expectations for stopping services
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

    let start_time = std::time::Instant::now();
    let batch_result = batch_manager.stop_all(Some(100)).await; // 100ms interval
    assert!(batch_result.errors.is_empty());
    let elapsed = start_time.elapsed();

    // Should have taken at least 100ms due to the interval
    assert!(elapsed >= std::time::Duration::from_millis(100));

    // Verify services are in stopped state
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Stopped);
        assert!(service.pid().await.is_none());
    }

    Ok(())
}
