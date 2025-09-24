// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_service_management::{
    NodeRegistryManager, ReachabilityProgress, ServiceStatus, metric::ReachabilityStatusValues,
};
use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use node_launchpad::action::{
    Action, NodeManagementCommand, NodeManagementResponse, NodeTableActions,
};
use node_launchpad::components::node_table::lifecycle::LifecycleState;
use node_launchpad::mode::Scene;
use node_launchpad::node_stats::{AggregatedNodeStats, IndividualNodeStats};
use node_launchpad::test_utils::{
    JourneyBuilder, MockNodeManagement, MockNodeRegistry, MockResponsePlan, TestAppBuilder,
};
use std::{sync::Arc, time::Duration};

#[tokio::test]
async fn journey_add_node_shows_transition_and_metrics() -> Result<()> {
    // Prepare mock registry and final node snapshot
    let registry = MockNodeRegistry::empty()?;
    let node_template = registry.create_test_node_service_data(0, ServiceStatus::Running);
    let registry_path = registry.get_registry_path().clone();
    let node_registry_manager = NodeRegistryManager::load(&registry_path).await?;

    // Prepare scripted metrics events: initial, in-progress, complete
    let metrics_in_progress = aggregated_stats(
        &node_template.service_name,
        ReachabilityProgress::InProgress(20),
        false,
    );
    let metrics_complete = aggregated_stats(
        &node_template.service_name,
        ReachabilityProgress::Complete,
        true,
    );
    // Prepare mock node management
    let (mock_management, management_handle) = MockNodeManagement::new();

    // Build app with injected dependencies
    let test_app = TestAppBuilder::new()
        .with_mock_registry(registry)
        .with_mock_node_management(Arc::clone(&mock_management), management_handle)
        .with_node_registry(node_registry_manager.clone())
        .with_nodes_to_start(1)
        .with_metrics_script(vec![AggregatedNodeStats::default()])
        .build()
        .await?;
    drop(mock_management);

    let response_plan = MockResponsePlan::with_follow_up(
        NodeManagementResponse::AddNode { error: None },
        vec![
            Action::StoreAggregatedNodeStats(metrics_in_progress.clone()),
            Action::StoreAggregatedNodeStats(metrics_complete.clone()),
            Action::NodeTableActions(NodeTableActions::RegistryFileUpdated {
                all_nodes_data: vec![node_template.clone()],
            }),
        ],
    )
    .with_delay(Duration::from_millis(10));

    let mut journey = JourneyBuilder::from_context("Add node transition", test_app)?
        .with_node_action_response(NodeManagementCommand::AddNode, response_plan)
        .press('+')
        .step()
        .wait(Duration::from_millis(10))
        .step()
        .expect_node_state(&node_template.service_name, LifecycleState::Running, false)
        .build()?;

    journey.run().await?;

    Ok(())
}

#[tokio::test]
async fn journey_start_node_failure_surfaces_error_and_clears_transition() -> Result<()> {
    let mut registry = MockNodeRegistry::empty()?;
    let stopped_node = registry.create_test_node_service_data(0, ServiceStatus::Stopped);
    registry.add_node(stopped_node.clone())?;
    let node_name = stopped_node.service_name.clone();
    let registry_path = registry.get_registry_path().clone();
    let node_registry_manager = NodeRegistryManager::load(&registry_path).await?;

    let (mock_management, management_handle) = MockNodeManagement::new();

    let test_app = TestAppBuilder::new()
        .with_mock_registry(registry)
        .with_mock_node_management(Arc::clone(&mock_management), management_handle)
        .with_node_registry(node_registry_manager.clone())
        .build()
        .await?;
    drop(mock_management);

    let response_plan = MockResponsePlan::immediate(NodeManagementResponse::StartNodes {
        service_names: vec![node_name.clone()],
        error: Some("failed to start".to_string()),
    });

    let mut journey = JourneyBuilder::from_context("Start node failure", test_app)?
        .with_node_action_response(NodeManagementCommand::StartNodes, response_plan)
        .expect_node_state(&node_name, LifecycleState::Stopped, false)
        .step()
        .press_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
        .wait(Duration::from_millis(50))
        .step()
        .expect_error_popup_contains("failed to start")
        .assert_spinner(&node_name, false)
        .expect_node_state(&node_name, LifecycleState::Stopped, false)
        .step()
        .build()?;

    journey.run().await?;

    Ok(())
}

fn aggregated_stats(
    service_name: &str,
    progress: ReachabilityProgress,
    public: bool,
) -> AggregatedNodeStats {
    let mut stats = IndividualNodeStats {
        service_name: service_name.to_string(),
        reachability_status: ReachabilityStatusValues {
            progress: progress.clone(),
            public,
            private: false,
            upnp: public,
        },
        ..Default::default()
    };
    stats.bandwidth_inbound = 100;
    stats.bandwidth_outbound = 50;
    stats.memory_usage_mb = 10;

    AggregatedNodeStats {
        total_rewards_wallet_balance: 0,
        total_memory_usage_mb: 0,
        individual_stats: vec![stats],
        failed_to_connect: vec![],
    }
}

#[tokio::test]
async fn journey_add_node_failure_shows_error_popup() -> Result<()> {
    let registry = MockNodeRegistry::empty()?;
    let node_template = registry.create_test_node_service_data(0, ServiceStatus::Running);
    let registry_path = registry.get_registry_path().clone();
    let node_registry_manager = NodeRegistryManager::load(&registry_path).await?;

    let (mock_management, management_handle) = MockNodeManagement::new();

    let test_app = TestAppBuilder::new()
        .with_mock_registry(registry)
        .with_mock_node_management(Arc::clone(&mock_management), management_handle)
        .with_node_registry(node_registry_manager.clone())
        .with_nodes_to_start(1)
        .with_metrics_script(vec![AggregatedNodeStats::default()])
        .build()
        .await?;
    drop(mock_management);

    let response_plan = MockResponsePlan::with_follow_up(
        NodeManagementResponse::AddNode {
            error: Some("disk full".to_string()),
        },
        vec![Action::NodeTableActions(
            NodeTableActions::RegistryFileUpdated {
                all_nodes_data: vec![node_template.clone()],
            },
        )],
    )
    .with_delay(Duration::from_millis(10));

    let mut journey = JourneyBuilder::from_context("Add node failure", test_app)?
        .with_node_action_response(NodeManagementCommand::AddNode, response_plan)
        .press('+')
        .step()
        .wait(Duration::from_millis(40))
        .expect_error_popup_contains("disk full")
        .assert_spinner(&node_template.service_name, false)
        .expect_node_state(&node_template.service_name, LifecycleState::Running, false)
        .step()
        .build()?;

    journey.run().await?;

    Ok(())
}

#[tokio::test]
async fn journey_start_node_success_updates_state() -> Result<()> {
    let mut registry = MockNodeRegistry::empty()?;
    let stopped_node = registry.create_test_node_service_data(0, ServiceStatus::Stopped);
    registry.add_node(stopped_node.clone())?;
    let mut running_node = stopped_node.clone();
    running_node.status = ServiceStatus::Running;

    let registry_path = registry.get_registry_path().clone();
    let node_registry_manager = NodeRegistryManager::load(&registry_path).await?;

    let (mock_management, management_handle) = MockNodeManagement::new();

    let test_app = TestAppBuilder::new()
        .with_mock_registry(registry)
        .with_mock_node_management(Arc::clone(&mock_management), management_handle)
        .with_node_registry(node_registry_manager.clone())
        .build()
        .await?;
    drop(mock_management);

    let running_metrics = aggregated_stats(
        &running_node.service_name,
        ReachabilityProgress::Complete,
        true,
    );

    let response_plan = MockResponsePlan::with_follow_up(
        NodeManagementResponse::StartNodes {
            service_names: vec![running_node.service_name.clone()],
            error: None,
        },
        vec![
            Action::StoreAggregatedNodeStats(running_metrics),
            Action::NodeTableActions(NodeTableActions::RegistryFileUpdated {
                all_nodes_data: vec![running_node.clone()],
            }),
        ],
    )
    .with_delay(Duration::from_millis(10));

    let mut journey = JourneyBuilder::from_context("Start node success", test_app)?
        .with_node_action_response(NodeManagementCommand::StartNodes, response_plan)
        .expect_node_state(&running_node.service_name, LifecycleState::Stopped, false)
        .step()
        .press_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
        .wait(Duration::from_millis(50))
        .expect_node_state(&running_node.service_name, LifecycleState::Running, false)
        .assert_spinner(&running_node.service_name, false)
        .step()
        .build()?;

    journey.run().await?;

    Ok(())
}

#[tokio::test]
async fn journey_stop_node_success_updates_state() -> Result<()> {
    let mut registry = MockNodeRegistry::empty()?;
    let running_node = registry.create_test_node_service_data(0, ServiceStatus::Running);
    registry.add_node(running_node.clone())?;
    let mut stopped_node = running_node.clone();
    stopped_node.status = ServiceStatus::Stopped;

    let registry_path = registry.get_registry_path().clone();
    let node_registry_manager = NodeRegistryManager::load(&registry_path).await?;

    let (mock_management, management_handle) = MockNodeManagement::new();

    let test_app = TestAppBuilder::new()
        .with_mock_registry(registry)
        .with_mock_node_management(Arc::clone(&mock_management), management_handle)
        .with_node_registry(node_registry_manager.clone())
        .build()
        .await?;
    drop(mock_management);

    let response_plan = MockResponsePlan::with_follow_up(
        NodeManagementResponse::StopNodes {
            service_names: vec![running_node.service_name.clone()],
            error: None,
        },
        vec![Action::NodeTableActions(
            NodeTableActions::RegistryFileUpdated {
                all_nodes_data: vec![stopped_node.clone()],
            },
        )],
    )
    .with_delay(Duration::from_millis(10));

    let mut journey = JourneyBuilder::from_context("Stop node success", test_app)?
        .with_node_action_response(NodeManagementCommand::StopNodes, response_plan)
        .expect_node_state(&running_node.service_name, LifecycleState::Running, false)
        .step()
        .press_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL))
        .wait(Duration::from_millis(50))
        .expect_node_state(&running_node.service_name, LifecycleState::Stopped, false)
        .assert_spinner(&running_node.service_name, false)
        .step()
        .build()?;

    journey.run().await?;

    Ok(())
}

#[tokio::test]
async fn journey_stop_node_failure_displays_error() -> Result<()> {
    let mut registry = MockNodeRegistry::empty()?;
    let running_node = registry.create_test_node_service_data(0, ServiceStatus::Running);
    registry.add_node(running_node.clone())?;

    let registry_path = registry.get_registry_path().clone();
    let node_registry_manager = NodeRegistryManager::load(&registry_path).await?;

    let (mock_management, management_handle) = MockNodeManagement::new();

    let test_app = TestAppBuilder::new()
        .with_mock_registry(registry)
        .with_mock_node_management(Arc::clone(&mock_management), management_handle)
        .with_node_registry(node_registry_manager.clone())
        .build()
        .await?;
    drop(mock_management);

    let response_plan = MockResponsePlan::with_follow_up(
        NodeManagementResponse::StopNodes {
            service_names: vec![running_node.service_name.clone()],
            error: Some("could not stop".to_string()),
        },
        vec![Action::NodeTableActions(
            NodeTableActions::RegistryFileUpdated {
                all_nodes_data: vec![running_node.clone()],
            },
        )],
    )
    .with_delay(Duration::from_millis(10));

    let mut journey = JourneyBuilder::from_context("Stop node failure", test_app)?
        .with_node_action_response(NodeManagementCommand::StopNodes, response_plan)
        .expect_node_state(&running_node.service_name, LifecycleState::Running, false)
        .step()
        .press_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL))
        .step()
        .wait(Duration::from_millis(50))
        .expect_error_popup_contains("could not stop")
        .assert_spinner(&running_node.service_name, false)
        .expect_node_state(&running_node.service_name, LifecycleState::Running, false)
        .step()
        .build()?;

    journey.run().await?;

    Ok(())
}

#[tokio::test]
async fn journey_toggle_node_ignores_locked_node() -> Result<()> {
    let mut registry = MockNodeRegistry::empty()?;
    let running_node = registry.create_test_node_service_data(0, ServiceStatus::Running);
    registry.add_node(running_node.clone())?;

    let registry_path = registry.get_registry_path().clone();
    let node_registry_manager = NodeRegistryManager::load(&registry_path).await?;

    let (mock_management, management_handle) = MockNodeManagement::new();

    let test_app = TestAppBuilder::new()
        .with_mock_registry(registry)
        .with_mock_node_management(Arc::clone(&mock_management), management_handle)
        .with_node_registry(node_registry_manager.clone())
        .build()
        .await?;

    // Lock the node by marking a transition we never clear
    let mut journey = JourneyBuilder::from_context("Toggle locked node", test_app)?
        .step()
        .press_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE))
        .expect_node_state(&running_node.service_name, LifecycleState::Running, false)
        .step()
        .build()?;

    drop(mock_management);
    journey.run().await?;

    Ok(())
}

#[tokio::test]
async fn journey_toggle_node_start_transitions_to_running() -> Result<()> {
    let mut registry = MockNodeRegistry::empty()?;
    let stopped_node = registry.create_test_node_service_data(0, ServiceStatus::Stopped);
    registry.add_node(stopped_node.clone())?;
    let mut running_node = stopped_node.clone();
    running_node.status = ServiceStatus::Running;

    let registry_path = registry.get_registry_path().clone();
    let node_registry_manager = NodeRegistryManager::load(&registry_path).await?;

    let (mock_management, management_handle) = MockNodeManagement::new();

    let test_app = TestAppBuilder::new()
        .with_mock_registry(registry)
        .with_mock_node_management(Arc::clone(&mock_management), management_handle)
        .with_node_registry(node_registry_manager.clone())
        .build()
        .await?;
    drop(mock_management);

    let metrics = aggregated_stats(
        &running_node.service_name,
        ReachabilityProgress::Complete,
        true,
    );

    let response_plan = MockResponsePlan::with_follow_up(
        NodeManagementResponse::StartNodes {
            service_names: vec![running_node.service_name.clone()],
            error: None,
        },
        vec![
            Action::StoreAggregatedNodeStats(metrics),
            Action::NodeTableActions(NodeTableActions::RegistryFileUpdated {
                all_nodes_data: vec![running_node.clone()],
            }),
        ],
    )
    .with_delay(Duration::from_millis(20));

    let mut journey = JourneyBuilder::from_context("Toggle start node", test_app)?
        .with_node_action_response(NodeManagementCommand::StartNodes, response_plan)
        .expect_node_state(&running_node.service_name, LifecycleState::Stopped, false)
        .step()
        .press_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL))
        .assert_spinner(&running_node.service_name, true)
        .expect_node_state(&running_node.service_name, LifecycleState::Starting, true)
        .step()
        .wait_for_node_state(
            &running_node.service_name,
            LifecycleState::Running,
            Duration::from_millis(500),
            Duration::from_millis(20),
        )
        .step()
        .expect_node_state(&running_node.service_name, LifecycleState::Running, false)
        .assert_spinner(&running_node.service_name, false)
        .build()?;

    journey.run().await?;

    Ok(())
}

#[tokio::test]
async fn journey_toggle_node_stop_transitions_to_stopped() -> Result<()> {
    let mut registry = MockNodeRegistry::empty()?;
    let running_node = registry.create_test_node_service_data(0, ServiceStatus::Running);
    registry.add_node(running_node.clone())?;
    let mut stopped_node = running_node.clone();
    stopped_node.status = ServiceStatus::Stopped;

    let registry_path = registry.get_registry_path().clone();
    let node_registry_manager = NodeRegistryManager::load(&registry_path).await?;

    let (mock_management, management_handle) = MockNodeManagement::new();

    let test_app = TestAppBuilder::new()
        .with_mock_registry(registry)
        .with_mock_node_management(Arc::clone(&mock_management), management_handle)
        .with_node_registry(node_registry_manager.clone())
        .build()
        .await?;
    drop(mock_management);

    let response_plan = MockResponsePlan::with_follow_up(
        NodeManagementResponse::StopNodes {
            service_names: vec![running_node.service_name.clone()],
            error: None,
        },
        vec![Action::NodeTableActions(
            NodeTableActions::RegistryFileUpdated {
                all_nodes_data: vec![stopped_node.clone()],
            },
        )],
    )
    .with_delay(Duration::from_millis(20));

    let mut journey = JourneyBuilder::from_context("Toggle stop node", test_app)?
        .with_node_action_response(NodeManagementCommand::StopNodes, response_plan)
        .expect_node_state(&running_node.service_name, LifecycleState::Running, false)
        .step()
        .press_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL))
        .assert_spinner(&running_node.service_name, true)
        .expect_node_state(&running_node.service_name, LifecycleState::Stopping, true)
        .step()
        .wait_for_node_state(
            &running_node.service_name,
            LifecycleState::Stopped,
            Duration::from_millis(500),
            Duration::from_millis(20),
        )
        .step()
        .expect_node_state(&running_node.service_name, LifecycleState::Stopped, false)
        .assert_spinner(&running_node.service_name, false)
        .build()?;

    journey.run().await?;

    Ok(())
}

#[tokio::test]
async fn journey_remove_node_success_enters_refreshing_state() -> Result<()> {
    let mut registry = MockNodeRegistry::empty()?;
    let running_node = registry.create_test_node_service_data(0, ServiceStatus::Running);
    registry.add_node(running_node.clone())?;

    let registry_path = registry.get_registry_path().clone();
    let node_registry_manager = NodeRegistryManager::load(&registry_path).await?;

    let (mock_management, management_handle) = MockNodeManagement::new();

    let test_app = TestAppBuilder::new()
        .with_mock_registry(registry)
        .with_mock_node_management(Arc::clone(&mock_management), management_handle)
        .with_node_registry(node_registry_manager.clone())
        .build()
        .await?;
    drop(mock_management);

    let node_name = running_node.service_name.clone();

    let response_plan = MockResponsePlan::with_follow_up(
        NodeManagementResponse::RemoveNodes {
            service_names: vec![node_name.clone()],
            error: None,
        },
        vec![Action::NodeTableActions(
            NodeTableActions::RegistryFileUpdated {
                all_nodes_data: vec![],
            },
        )],
    )
    .with_delay(Duration::from_millis(20));

    let mut journey = JourneyBuilder::from_context("Remove node success", test_app)?
        .with_node_action_response(NodeManagementCommand::RemoveNodes, response_plan)
        .expect_node_state(&node_name, LifecycleState::Running, false)
        .step()
        .press('-')
        .expect_scene(Scene::RemoveNodePopUp)
        .step()
        .press_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect_scene(Scene::Status)
        .assert_spinner(&node_name, true)
        .expect_node_state(&node_name, LifecycleState::Removing, true)
        .step()
        .wait_for_node_state(
            &node_name,
            LifecycleState::Refreshing,
            Duration::from_millis(500),
            Duration::from_millis(20),
        )
        .step()
        .expect_node_state(&node_name, LifecycleState::Refreshing, false)
        .build()?;

    journey.run().await?;

    Ok(())
}

#[tokio::test]
async fn journey_remove_node_failure_shows_error_and_keeps_node() -> Result<()> {
    let mut registry = MockNodeRegistry::empty()?;
    let running_node = registry.create_test_node_service_data(0, ServiceStatus::Running);
    registry.add_node(running_node.clone())?;

    let registry_path = registry.get_registry_path().clone();
    let node_registry_manager = NodeRegistryManager::load(&registry_path).await?;

    let (mock_management, management_handle) = MockNodeManagement::new();

    let test_app = TestAppBuilder::new()
        .with_mock_registry(registry)
        .with_mock_node_management(Arc::clone(&mock_management), management_handle)
        .with_node_registry(node_registry_manager.clone())
        .build()
        .await?;
    drop(mock_management);

    let node_name = running_node.service_name.clone();
    let failure_message = "could not remove".to_string();

    let response_plan = MockResponsePlan::with_follow_up(
        NodeManagementResponse::RemoveNodes {
            service_names: vec![node_name.clone()],
            error: Some(failure_message.clone()),
        },
        vec![Action::NodeTableActions(
            NodeTableActions::RegistryFileUpdated {
                all_nodes_data: vec![running_node.clone()],
            },
        )],
    )
    .with_delay(Duration::from_millis(20));

    let mut journey = JourneyBuilder::from_context("Remove node failure", test_app)?
        .with_node_action_response(NodeManagementCommand::RemoveNodes, response_plan)
        .expect_node_state(&node_name, LifecycleState::Running, false)
        .step()
        .press('-')
        .expect_scene(Scene::RemoveNodePopUp)
        .step()
        .press_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect_scene(Scene::Status)
        .assert_spinner(&node_name, true)
        .expect_node_state(&node_name, LifecycleState::Removing, true)
        .step()
        .wait(Duration::from_millis(40))
        .step()
        .expect_error_popup_contains(&failure_message)
        .assert_spinner(&node_name, false)
        .expect_node_state(&node_name, LifecycleState::Running, false)
        .build()?;

    journey.run().await?;

    Ok(())
}

#[tokio::test]
async fn journey_maintain_nodes_failure_shows_error_popup() -> Result<()> {
    let mut registry = MockNodeRegistry::empty()?;
    let first_node = registry.create_test_node_service_data(0, ServiceStatus::Running);
    let second_node = registry.create_test_node_service_data(1, ServiceStatus::Running);
    registry.add_node(first_node.clone())?;
    registry.add_node(second_node.clone())?;

    let registry_path = registry.get_registry_path().clone();
    let node_registry_manager = NodeRegistryManager::load(&registry_path).await?;

    let (mock_management, management_handle) = MockNodeManagement::new();

    let test_app = TestAppBuilder::new()
        .with_mock_registry(registry)
        .with_mock_node_management(Arc::clone(&mock_management), management_handle)
        .with_node_registry(node_registry_manager.clone())
        .with_nodes_to_start(2)
        .build()
        .await?;
    drop(mock_management);

    let error_message = "maintenance failed".to_string();
    let response_plan = MockResponsePlan::immediate(NodeManagementResponse::MaintainNodes {
        error: Some(error_message.clone()),
    })
    .with_delay(Duration::from_millis(20));

    let mut journey = JourneyBuilder::from_context("Maintain nodes failure", test_app)?
        .with_node_action_response(NodeManagementCommand::MaintainNodes, response_plan)
        .expect_node_state(&first_node.service_name, LifecycleState::Running, false)
        .expect_node_state(&second_node.service_name, LifecycleState::Running, false)
        .step()
        .press_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL))
        .expect_scene(Scene::ManageNodesPopUp { amount_of_nodes: 2 })
        .step()
        .press_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect_scene(Scene::Status)
        .assert_spinner(&first_node.service_name, true)
        .assert_spinner(&second_node.service_name, true)
        .step()
        .wait(Duration::from_millis(40))
        .step()
        .expect_error_popup_contains(&error_message)
        .assert_spinner(&first_node.service_name, false)
        .assert_spinner(&second_node.service_name, false)
        .expect_node_state(&first_node.service_name, LifecycleState::Running, false)
        .expect_node_state(&second_node.service_name, LifecycleState::Running, false)
        .build()?;

    journey.run().await?;

    Ok(())
}
