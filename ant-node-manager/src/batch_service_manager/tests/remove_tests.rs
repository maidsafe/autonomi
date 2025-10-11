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
async fn remove_all_should_remove_added_services() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up expectations for uninstalling services
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode2"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // Create services in Added state (default)
    let services = create_test_services_simple(2);

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    let batch_result = batch_manager.remove_all(false).await;
    assert!(batch_result.errors.is_empty());

    // Verify services have been removed
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Removed);
    }

    Ok(())
}

#[tokio::test]
async fn remove_all_should_error_for_running_services() -> Result<()> {
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

    // No uninstall calls should be made for running services

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

    // This should return a BatchResult with errors due to running services
    let result = batch_manager.remove_all(false).await;
    assert!(!result.errors.is_empty());

    // Verify services remain in running state
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Running);
    }

    Ok(())
}

#[tokio::test]
async fn remove_all_should_error_for_inconsistent_state() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up expectations for checking if processes are running (they're not)
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

    // No uninstall calls should be made due to inconsistent state

    // Create services and set them to running state but processes aren't actually running
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

    // This should return a BatchResult with errors due to inconsistent state
    let result = batch_manager.remove_all(false).await;
    assert!(!result.errors.is_empty());

    // Verify services are now in stopped state (cleaned up from inconsistent state)
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Stopped);
    }

    Ok(())
}

#[tokio::test]
async fn remove_all_should_remove_and_keep_directories() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up expectations for uninstalling services
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode2"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // Create services in Added state
    let services = create_test_services_simple(2);

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    // Remove with keep_directories = true
    let batch_result = batch_manager.remove_all(true).await;
    assert!(batch_result.errors.is_empty());

    // Verify services have been removed
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Removed);
    }

    Ok(())
}

#[tokio::test]
async fn remove_all_should_remove_user_mode_services() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // Set up expectations for uninstalling user mode services
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode1"), eq(true))
        .times(1)
        .returning(|_, _| Ok(()));
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode2"), eq(true))
        .times(1)
        .returning(|_, _| Ok(()));

    // Create services and set them to user mode
    let services = create_test_services_simple(2);
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

    let batch_result = batch_manager.remove_all(false).await;
    assert!(batch_result.errors.is_empty());

    // Verify services have been removed
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Removed);
    }

    Ok(())
}

#[tokio::test]
async fn remove_all_should_handle_mixed_service_states() -> Result<()> {
    let mut mock_service_control = MockServiceControl::new();

    // First service is running (should error)
    mock_service_control
        .expect_get_process_pid()
        .with(eq(PathBuf::from("/var/antctl/services/antnode1/antnode")))
        .times(1)
        .returning(|_| Ok(1001));

    // Second service is not running, so it should be removed
    mock_service_control
        .expect_uninstall()
        .with(eq("antnode2"), eq(false))
        .times(1)
        .returning(|_, _| Ok(()));

    // Create services with different states
    let services = create_test_services_simple(2);

    // First service is running
    {
        let mut service_data = services[0].service_data.write().await;
        service_data.status = ServiceStatus::Running;
        service_data.pid = Some(1001);
    }

    // Second service is added (not running)
    {
        let mut service_data = services[1].service_data.write().await;
        service_data.status = ServiceStatus::Added;
    }

    let batch_manager = BatchServiceManager::new(
        services,
        Box::new(mock_service_control),
        create_test_registry(),
        VerbosityLevel::Normal,
    );

    // This should return a BatchResult with errors due to the first service being running
    let result = batch_manager.remove_all(false).await;
    assert!(!result.errors.is_empty());

    // Verify first service remains running, second service is removed
    assert_matches!(
        batch_manager.services[0].status().await,
        ServiceStatus::Running
    );
    assert_matches!(
        batch_manager.services[1].status().await,
        ServiceStatus::Removed
    );

    Ok(())
}
