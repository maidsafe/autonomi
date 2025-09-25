// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_service_management::{
    ReachabilityProgress, ServiceStatus, fs::CriticalFailure, metric::ReachabilityStatusValues,
};
use chrono::Utc;
use color_eyre::{Result, eyre::eyre};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use node_launchpad::action::{NodeManagementCommand, NodeManagementResponse};
use node_launchpad::components::node_table::lifecycle::LifecycleState;
use node_launchpad::mode::Scene;
use node_launchpad::node_stats::{AggregatedNodeStats, IndividualNodeStats};
use node_launchpad::test_utils::{
    JourneyBuilder, MockNodeResponsePlan, TestAppBuilder, make_node_service_data,
};
use std::time::Duration;

#[tokio::test]
async fn journey_add_node_shows_transition_and_metrics() -> Result<()> {
    let _log_guard = ant_logging::LogBuilder::init_single_threaded_tokio_test();
    // Prepare final node snapshot
    let node_template = make_node_service_data(0, ServiceStatus::Added);

    // Build app with injected dependencies
    let test_app = TestAppBuilder::new()
        .with_nodes_to_start(1)
        .with_metrics_events([AggregatedNodeStats::default()])
        .build()
        .await?;
    let response_plan =
        MockNodeResponsePlan::immediate(NodeManagementResponse::AddNode { error: None })
            .then_registry_snapshot(vec![node_template.clone()])
            .with_delay(Duration::from_millis(10));

    let mut journey = JourneyBuilder::from_context("Add node transition", test_app)?
        .with_node_action_response(NodeManagementCommand::AddNode, response_plan)
        .press('+')
        .step()
        .wait(Duration::from_millis(5))
        .step()
        .expect_node_state(&node_template.service_name, LifecycleState::Added, false)
        .wait_for_condition(
            "Wait for node to appear as added",
            {
                let node_id = node_template.service_name.clone();
                move |app| {
                    let model = node_launchpad::test_utils::node_view_model(app, &node_id)?;
                    Ok(matches!(model.lifecycle, LifecycleState::Added))
                }
            },
            Duration::from_millis(1000),
            Duration::from_millis(25),
        )
        .expect_text("Added")
        .build()?;

    journey.run().await?;

    Ok(())
}

#[tokio::test]
async fn journey_add_and_start_node_reports_unreachable_failure() -> Result<()> {
    let _log_guard = ant_logging::LogBuilder::init_single_threaded_tokio_test();
    let node_template = make_node_service_data(0, ServiceStatus::Added);
    let mut running_node = node_template.clone();
    running_node.status = ServiceStatus::Running;

    let unreachable_metrics = aggregated_stats(
        &node_template.service_name,
        ReachabilityProgress::Complete,
        false,
    );
    let unreachable_status = unreachable_metrics
        .individual_stats
        .first()
        .map(|stats| stats.reachability_status.clone())
        .unwrap_or_default();

    let test_app = TestAppBuilder::new()
        .with_nodes_to_start(1)
        .with_metrics_events([AggregatedNodeStats::default()])
        .build()
        .await?;

    let add_plan = MockNodeResponsePlan::immediate(NodeManagementResponse::AddNode { error: None })
        .then_registry_snapshot(vec![node_template.clone()])
        .with_delay(Duration::from_millis(10));

    let start_plan = MockNodeResponsePlan::immediate(NodeManagementResponse::StartNodes {
        service_names: vec![node_template.service_name.clone()],
        error: None,
    })
    .then_registry_snapshot(vec![running_node.clone()])
    .then_metrics([unreachable_metrics])
    .with_delay(Duration::from_millis(30));

    let unreachable_state = LifecycleState::Unreachable {
        reason: Some("Error (Unreachable)".to_string()),
    };

    let mut journey = JourneyBuilder::from_context("Add and start unreachable node", test_app)?
        .with_node_action_response(NodeManagementCommand::AddNode, add_plan)
        .with_node_action_response(NodeManagementCommand::StartNodes, start_plan)
        .press('+')
        .step()
        .wait(Duration::from_millis(5))
        .step()
        .expect_node_state(&node_template.service_name, LifecycleState::Added, false)
        .step()
        .press_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
        .assert_spinner(&node_template.service_name, true)
        .expect_node_state(&node_template.service_name, LifecycleState::Starting, true)
        .step()
        .wait_for_condition(
            "Wait for node to be flagged unreachable",
            {
                let node_id = node_template.service_name.clone();
                move |app| {
                    let model = node_launchpad::test_utils::node_view_model(app, &node_id)?;
                    Ok(matches!(
                        model.lifecycle,
                        LifecycleState::Unreachable { reason: Some(ref reason) }
                            if reason == "Error (Unreachable)"
                    ))
                }
            },
            Duration::from_millis(600),
            Duration::from_millis(25),
        )
        .step()
        .expect_node_state(&node_template.service_name, unreachable_state, false)
        .assert_spinner(&node_template.service_name, false)
        .expect_reachability(
            &node_template.service_name,
            ReachabilityProgress::Complete,
            unreachable_status,
        )
        .assert_app_state("Node reports Error (Unreachable) failure", {
            let node_id = node_template.service_name.clone();
            move |app| {
                let model = node_launchpad::test_utils::node_view_model(app, &node_id)?;
                if model
                    .last_failure
                    .as_deref()
                    .is_some_and(|failure| failure == "Error (Unreachable)")
                {
                    Ok(())
                } else {
                    Err(eyre!(
                        "Expected failure 'Error (Unreachable)' but found {:?}",
                        model.last_failure
                    ))
                }
            }
        })
        .expect_text("Error (Unreachable)")
        .build()?;

    journey.run().await?;

    Ok(())
}

#[tokio::test]
async fn journey_stopped_unreachable_failure_node_shows_error_status() -> Result<()> {
    let _log_guard = ant_logging::LogBuilder::init_single_threaded_tokio_test();
    let mut failed_node = make_node_service_data(0, ServiceStatus::Stopped);
    failed_node.last_critical_failure = Some(CriticalFailure {
        date_time: Utc::now(),
        reason: "Unreachable".to_string(),
    });

    let test_app = TestAppBuilder::new()
        .with_initial_node(failed_node.clone())
        .build()
        .await?;

    let mut journey = JourneyBuilder::from_context("Stopped unreachable node", test_app)?
        .expect_node_state(&failed_node.service_name, LifecycleState::Stopped, false)
        .assert_app_state("Last failure recorded", {
            let node_id = failed_node.service_name.clone();
            move |app| {
                let model = node_launchpad::test_utils::node_view_model(app, &node_id)?;
                if model
                    .last_failure
                    .as_deref()
                    .is_some_and(|failure| failure == "Error (Unreachable)")
                {
                    Ok(())
                } else {
                    Err(eyre!(
                        "Expected last failure 'Error (Unreachable)' but found {:?}",
                        model.last_failure
                    ))
                }
            }
        })
        .expect_text("Error (Unreachable)")
        .step()
        .build()?;

    journey.run().await?;

    Ok(())
}

#[tokio::test]
async fn journey_start_node_failure_surfaces_error_and_clears_transition() -> Result<()> {
    let _log_guard = ant_logging::LogBuilder::init_single_threaded_tokio_test();
    let stopped_node = make_node_service_data(0, ServiceStatus::Stopped);
    let node_name = stopped_node.service_name.clone();

    let test_app = TestAppBuilder::new()
        .with_initial_node(stopped_node.clone())
        .build()
        .await?;
    let response_plan = MockNodeResponsePlan::immediate(NodeManagementResponse::StartNodes {
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
    let _log_guard = ant_logging::LogBuilder::init_single_threaded_tokio_test();
    let node_template = make_node_service_data(0, ServiceStatus::Running);

    let test_app = TestAppBuilder::new()
        .with_nodes_to_start(1)
        .with_metrics_events([AggregatedNodeStats::default()])
        .build()
        .await?;
    let response_plan = MockNodeResponsePlan::immediate(NodeManagementResponse::AddNode {
        error: Some("disk full".to_string()),
    })
    .then_registry_snapshot(vec![node_template.clone()])
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
    let _log_guard = ant_logging::LogBuilder::init_single_threaded_tokio_test();
    let stopped_node = make_node_service_data(0, ServiceStatus::Stopped);
    let mut running_node = stopped_node.clone();
    running_node.status = ServiceStatus::Running;

    let test_app = TestAppBuilder::new()
        .with_initial_node(stopped_node.clone())
        .build()
        .await?;
    let running_metrics = aggregated_stats(
        &running_node.service_name,
        ReachabilityProgress::Complete,
        true,
    );

    let response_plan = MockNodeResponsePlan::immediate(NodeManagementResponse::StartNodes {
        service_names: vec![running_node.service_name.clone()],
        error: None,
    })
    .then_metrics([running_metrics])
    .then_registry_snapshot(vec![running_node.clone()])
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
    let _log_guard = ant_logging::LogBuilder::init_single_threaded_tokio_test();
    let running_node = make_node_service_data(0, ServiceStatus::Running);
    let mut stopped_node = running_node.clone();
    stopped_node.status = ServiceStatus::Stopped;

    let test_app = TestAppBuilder::new()
        .with_initial_node(running_node.clone())
        .build()
        .await?;
    let response_plan = MockNodeResponsePlan::immediate(NodeManagementResponse::StopNodes {
        service_names: vec![running_node.service_name.clone()],
        error: None,
    })
    .then_registry_snapshot(vec![stopped_node.clone()])
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
    let _log_guard = ant_logging::LogBuilder::init_single_threaded_tokio_test();
    let running_node = make_node_service_data(0, ServiceStatus::Running);

    let test_app = TestAppBuilder::new()
        .with_initial_node(running_node.clone())
        .build()
        .await?;
    let response_plan = MockNodeResponsePlan::immediate(NodeManagementResponse::StopNodes {
        service_names: vec![running_node.service_name.clone()],
        error: Some("could not stop".to_string()),
    })
    .then_registry_snapshot(vec![running_node.clone()])
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
    let _log_guard = ant_logging::LogBuilder::init_single_threaded_tokio_test();
    let running_node = make_node_service_data(0, ServiceStatus::Running);

    let test_app = TestAppBuilder::new()
        .with_initial_node(running_node.clone())
        .build()
        .await?;

    // Lock the node by marking a transition we never clear
    let mut journey = JourneyBuilder::from_context("Toggle locked node", test_app)?
        .step()
        .press_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE))
        .expect_node_state(&running_node.service_name, LifecycleState::Running, false)
        .step()
        .build()?;

    journey.run().await?;

    Ok(())
}

#[tokio::test]
async fn journey_toggle_node_start_transitions_to_running() -> Result<()> {
    let _log_guard = ant_logging::LogBuilder::init_single_threaded_tokio_test();
    let stopped_node = make_node_service_data(0, ServiceStatus::Stopped);
    let mut running_node = stopped_node.clone();
    running_node.status = ServiceStatus::Running;

    let test_app = TestAppBuilder::new()
        .with_initial_node(stopped_node.clone())
        .build()
        .await?;
    let metrics = aggregated_stats(
        &running_node.service_name,
        ReachabilityProgress::Complete,
        true,
    );

    let response_plan = MockNodeResponsePlan::immediate(NodeManagementResponse::StartNodes {
        service_names: vec![running_node.service_name.clone()],
        error: None,
    })
    .then_metrics([metrics])
    .then_registry_snapshot(vec![running_node.clone()])
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
    let _log_guard = ant_logging::LogBuilder::init_single_threaded_tokio_test();
    let running_node = make_node_service_data(0, ServiceStatus::Running);
    let mut stopped_node = running_node.clone();
    stopped_node.status = ServiceStatus::Stopped;

    let test_app = TestAppBuilder::new()
        .with_initial_node(running_node.clone())
        .build()
        .await?;
    let response_plan = MockNodeResponsePlan::immediate(NodeManagementResponse::StopNodes {
        service_names: vec![running_node.service_name.clone()],
        error: None,
    })
    .then_registry_snapshot(vec![stopped_node.clone()])
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
    let _log_guard = ant_logging::LogBuilder::init_single_threaded_tokio_test();
    let running_node = make_node_service_data(0, ServiceStatus::Running);

    let test_app = TestAppBuilder::new()
        .with_initial_node(running_node.clone())
        .build()
        .await?;
    let node_name = running_node.service_name.clone();

    let response_plan = MockNodeResponsePlan::immediate(NodeManagementResponse::RemoveNodes {
        service_names: vec![node_name.clone()],
        error: None,
    })
    .then_registry_snapshot(vec![])
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
        .wait_for_condition(
            "Wait for node to disappear after removal",
            {
                let node_id = node_name.clone();
                move |app| Ok(node_launchpad::test_utils::node_view_model(app, &node_id).is_err())
            },
            Duration::from_millis(500),
            Duration::from_millis(20),
        )
        .build()?;

    journey.run().await?;

    Ok(())
}

#[tokio::test]
async fn journey_remove_node_failure_shows_error_and_keeps_node() -> Result<()> {
    let _log_guard = ant_logging::LogBuilder::init_single_threaded_tokio_test();
    let running_node = make_node_service_data(0, ServiceStatus::Running);

    let test_app = TestAppBuilder::new()
        .with_initial_node(running_node.clone())
        .build()
        .await?;
    let node_name = running_node.service_name.clone();
    let failure_message = "could not remove".to_string();

    let response_plan = MockNodeResponsePlan::immediate(NodeManagementResponse::RemoveNodes {
        service_names: vec![node_name.clone()],
        error: Some(failure_message.clone()),
    })
    .then_registry_snapshot(vec![running_node.clone()])
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
async fn journey_manage_nodes_success_brings_new_node_online() -> Result<()> {
    let _log_guard = ant_logging::LogBuilder::init_single_threaded_tokio_test();
    let node_name = "antnode-1".to_string();

    let mut node_added = make_node_service_data(0, ServiceStatus::Added);
    node_added.reachability_progress = ReachabilityProgress::InProgress(25);

    let mut node_running = make_node_service_data(0, ServiceStatus::Running);
    node_running.reachability_progress = ReachabilityProgress::Complete;

    let progress_metrics =
        aggregated_stats(&node_name, ReachabilityProgress::InProgress(25), false);
    let progress_status = progress_metrics.individual_stats[0]
        .reachability_status
        .clone();
    let completion_metrics = aggregated_stats(&node_name, ReachabilityProgress::Complete, true);
    let completion_status = completion_metrics.individual_stats[0]
        .reachability_status
        .clone();

    let test_app = TestAppBuilder::new()
        .with_metrics_events([AggregatedNodeStats::default()])
        .build()
        .await?;

    let response_plan =
        MockNodeResponsePlan::immediate(NodeManagementResponse::MaintainNodes { error: None })
            .then_registry_snapshot(vec![node_added.clone()])
            .then_metrics([progress_metrics])
            .then_wait(Duration::from_millis(120))
            .then_registry_snapshot(vec![node_running.clone()])
            .then_metrics([completion_metrics]);

    let journey_builder = JourneyBuilder::from_context("Manage nodes success", test_app)?
        .with_node_action_response(NodeManagementCommand::MaintainNodes, response_plan)
        .expect_text("Press [+] to Add and Start your first node")
        .step()
        .press_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL))
        .expect_scene(Scene::ManageNodesPopUp { amount_of_nodes: 0 })
        .expect_text("Using 0GB of 700GB available space")
        .step()
        .press_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
        .expect_text("Using 35GB of 700GB available space")
        .step()
        .press_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect_scene(Scene::Status)
        .step()
        .wait_for_condition(
            "Wait for new node to appear",
            {
                let node_id = node_name.clone();
                move |app| Ok(node_launchpad::test_utils::node_view_model(app, &node_id).is_ok())
            },
            Duration::from_millis(1_000),
            Duration::from_millis(25),
        )
        .expect_node_state(&node_name, LifecycleState::Starting, false)
        .expect_reachability(
            &node_name,
            ReachabilityProgress::InProgress(25),
            progress_status,
        )
        .expect_text("Reachability 25%")
        .step()
        .wait(Duration::from_millis(150))
        .wait_for_node_state(
            &node_name,
            LifecycleState::Running,
            Duration::from_millis(1_000),
            Duration::from_millis(25),
        )
        .step()
        .expect_node_state(&node_name, LifecycleState::Running, false)
        .expect_reachability(
            &node_name,
            ReachabilityProgress::Complete,
            completion_status,
        );

    let mut journey = journey_builder.build()?;
    journey.run().await?;

    Ok(())
}

#[tokio::test]
async fn journey_maintain_nodes_failure_shows_error_popup() -> Result<()> {
    let _log_guard = ant_logging::LogBuilder::init_single_threaded_tokio_test();
    let first_node = make_node_service_data(0, ServiceStatus::Running);
    let second_node = make_node_service_data(1, ServiceStatus::Running);

    let test_app = TestAppBuilder::new()
        .with_initial_nodes([first_node.clone(), second_node.clone()])
        .with_nodes_to_start(2)
        .build()
        .await?;
    let error_message = "maintenance failed".to_string();
    let response_plan = MockNodeResponsePlan::immediate(NodeManagementResponse::MaintainNodes {
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
        .assert_spinner(&first_node.service_name, true)
        .assert_spinner(&second_node.service_name, true)
        .step()
        .wait_for_condition(
            "Status scene after manage nodes",
            |app| Ok(app.scene == Scene::Status),
            Duration::from_millis(1_000),
            Duration::from_millis(50),
        )
        .step()
        .wait(Duration::from_millis(80))
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
