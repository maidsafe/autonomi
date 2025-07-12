// Copyright (C) 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::helpers::*;
use ant_service_management::{ServiceStateActions, ServiceStatus};
use assert_matches::assert_matches;
use color_eyre::eyre::Result;
use mockall::predicate::*;
use std::path::PathBuf;

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

    // Create services with proper mock setup
    let services = create_test_services_with_rpc_mocks(2).await?;

    let batch_manager = setup_batch_service_manager(services, mock_service_control).await;

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
    let services = create_test_services_with_rpc_mocks(2).await?;
    for service in &services {
        let mut service_data = service.service_data.write().await;
        service_data.status = ServiceStatus::Stopped;
    }

    let batch_manager = setup_batch_service_manager(services, mock_service_control).await;

    let batch_result = batch_manager.start_all(1000).await;
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

    let batch_manager = setup_batch_service_manager(services, mock_service_control).await;

    let batch_result = batch_manager.start_all(1000).await;
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
    let services = create_test_services_with_rpc_mocks(2).await?;
    for service in &services {
        let mut service_data = service.service_data.write().await;
        service_data.status = ServiceStatus::Running;
        service_data.pid = Some(1001); // Set a PID even though process isn't actually running
    }

    let batch_manager = setup_batch_service_manager(services, mock_service_control).await;

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

    let services = create_test_services_with_failing_rpc_mocks(2);

    let batch_manager = setup_batch_service_manager(services, mock_service_control).await;

    // This should return a BatchResult with errors
    let result = batch_manager.start_all(1000).await;
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
    let services = create_test_services_with_rpc_mocks(2).await?;
    for service in &services {
        let mut service_data = service.service_data.write().await;
        service_data.user_mode = true;
    }

    let batch_manager = setup_batch_service_manager(services, mock_service_control).await;

    let batch_result = batch_manager.start_all(1000).await;
    assert!(batch_result.errors.is_empty());

    // Verify services are in running state
    for service in &batch_manager.services {
        assert_matches!(service.status().await, ServiceStatus::Running);
        assert!(service.pid().await.is_some());
    }

    Ok(())
}
