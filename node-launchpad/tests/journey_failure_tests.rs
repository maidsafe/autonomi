// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Negative-path coverage for the journey testing framework.
//!
//! Each test exercises one of the journey assertion helpers and verifies that an
//! incorrect expectation produces a meaningful error message.

use ant_service_management::{ReachabilityProgress, metric::ReachabilityStatusValues};
use color_eyre::{Result, eyre::eyre};
use node_launchpad::{
    action::Action,
    components::{
        Component, node_table::lifecycle::LifecycleState, popup::error_popup::ErrorPopup,
    },
    mode::Scene,
    test_utils::{JourneyBuilder, TestAppBuilder, status_component_mut},
};
use std::time::Duration;

#[tokio::test]
async fn journey_expect_scene_failure_reports_mismatch() -> Result<()> {
    let err = JourneyBuilder::new("expect_scene_failure")
        .await?
        .start_from(Scene::Status)
        .expect_scene(Scene::Options)
        .run()
        .await
        .expect_err("scene assertion should fail when scene mismatches");

    assert!(
        err.to_string().contains("Options"),
        "error should mention expected scene: {err}"
    );
    Ok(())
}

#[tokio::test]
async fn journey_expect_text_failure_reports_missing_text() -> Result<()> {
    let err = JourneyBuilder::new("expect_text_failure")
        .await?
        .start_from(Scene::Status)
        .expect_text("THIS STRING DOES NOT EXIST")
        .run()
        .await
        .expect_err("text assertion should fail when text is absent");

    assert!(
        err.to_string().contains("THIS STRING DOES NOT EXIST"),
        "error should mention missing text: {err}"
    );
    Ok(())
}

#[tokio::test]
async fn journey_expect_screen_failure_reports_mismatch() -> Result<()> {
    let err = JourneyBuilder::new("expect_screen_failure")
        .await?
        .start_from(Scene::Status)
        .expect_screen(&["completely wrong screen"])
        .run()
        .await
        .expect_err("screen assertion should fail when buffer differs");

    assert!(err.to_string().contains("completely wrong screen"));
    Ok(())
}

#[tokio::test]
async fn journey_expect_error_popup_failure_reports_absence() -> Result<()> {
    let missing_err = JourneyBuilder::new("expect_error_popup_missing_failure")
        .await?
        .start_from(Scene::Status)
        .expect_error_popup_contains("boom")
        .run()
        .await
        .expect_err("expect_error_popup_contains should fail without an error popup");

    assert!(missing_err.to_string().contains("Error popup not visible"));

    let mut context = TestAppBuilder::new().build().await?;
    {
        let status = status_component_mut(&mut context.app)?;
        let _ = status.update(Action::ShowErrorPopup(ErrorPopup::new(
            "Title",
            "Actual message",
            "Details",
        )))?;
    }

    let mismatch_err =
        JourneyBuilder::from_context("expect_error_popup_mismatch_failure", context)?
            .expect_error_popup_contains("Different message")
            .run()
            .await
            .expect_err("expect_error_popup_contains should fail when substring mismatches");

    assert!(
        mismatch_err
            .to_string()
            .contains("Error popup missing snippet")
    );
    Ok(())
}

#[tokio::test]
async fn journey_expect_node_state_failure_reports_mismatch() -> Result<()> {
    let lifecycle_err = JourneyBuilder::new_with_nodes("expect_node_state_failure", 1)
        .await?
        .expect_node_state("antnode-1", LifecycleState::Stopped, false)
        .run()
        .await
        .expect_err("node lifecycle assertion should fail when lifecycle mismatches");

    assert!(lifecycle_err.to_string().contains("Stopped"));

    let lock_err = JourneyBuilder::new_with_nodes("expect_node_state_lock_failure", 1)
        .await?
        .expect_node_state("antnode-1", LifecycleState::Running, true)
        .run()
        .await
        .expect_err("node state should fail when locked flag mismatches");

    assert!(lock_err.to_string().contains("lock state mismatch"));
    Ok(())
}

#[tokio::test]
async fn journey_assert_spinner_failure_reports_mismatch() -> Result<()> {
    let err = JourneyBuilder::new_with_nodes("assert_spinner_failure", 1)
        .await?
        .assert_spinner("antnode-1", true)
        .run()
        .await
        .expect_err("spinner assertion should fail when spinner state mismatches");

    assert!(err.to_string().contains("spinner"));
    Ok(())
}

#[tokio::test]
async fn journey_expect_reachability_failure_reports_mismatch() -> Result<()> {
    let progress_err = JourneyBuilder::new_with_nodes("expect_reachability_progress_failure", 1)
        .await?
        .expect_reachability(
            "antnode-1",
            ReachabilityProgress::Complete,
            ReachabilityStatusValues {
                progress: ReachabilityProgress::Complete,
                public: true,
                private: true,
                upnp: true,
            },
        )
        .run()
        .await
        .expect_err("reachability assertion should fail when progress mismatches");

    assert!(
        progress_err
            .to_string()
            .contains("reachability progress mismatch")
    );

    let status_err = JourneyBuilder::new_with_nodes("expect_reachability_status_failure", 1)
        .await?
        .expect_reachability(
            "antnode-1",
            ReachabilityProgress::NotRun,
            ReachabilityStatusValues {
                progress: ReachabilityProgress::NotRun,
                public: true,
                private: true,
                upnp: true,
            },
        )
        .run()
        .await
        .expect_err("reachability assertion should fail when status mismatches");

    assert!(
        status_err
            .to_string()
            .contains("reachability status mismatch")
    );
    Ok(())
}

#[tokio::test]
async fn journey_wait_for_node_state_failure_times_out() -> Result<()> {
    let err = JourneyBuilder::new_with_nodes("wait_for_node_state_failure", 1)
        .await?
        .wait_for_node_state(
            "antnode-1",
            LifecycleState::Stopped,
            Duration::from_millis(20),
            Duration::from_millis(5),
        )
        .run()
        .await
        .expect_err("wait_for_node_state should time out when lifecycle never matches");

    assert!(err.to_string().contains("Wait for node"));
    Ok(())
}

#[tokio::test]
async fn journey_wait_for_reachability_failure_times_out() -> Result<()> {
    let err = JourneyBuilder::new_with_nodes("wait_for_reachability_failure", 1)
        .await?
        .wait_for_reachability(
            "antnode-1",
            ReachabilityProgress::Complete,
            Duration::from_millis(20),
            Duration::from_millis(5),
        )
        .run()
        .await
        .expect_err("wait_for_reachability should time out when progress never matches");

    assert!(err.to_string().contains("Wait for reachability"));
    Ok(())
}

#[tokio::test]
async fn journey_wait_for_condition_failure_times_out() -> Result<()> {
    let timeout_err = JourneyBuilder::new("wait_for_condition_timeout_failure")
        .await?
        .wait_for_condition(
            "predicate never true",
            |_| Ok(false),
            Duration::from_millis(20),
            Duration::from_millis(5),
        )
        .run()
        .await
        .expect_err("wait_for_condition should time out when predicate stays false");

    assert!(timeout_err.to_string().contains("predicate never true"));

    let error_err = JourneyBuilder::new("wait_for_condition_error_failure")
        .await?
        .wait_for_condition(
            "predicate error",
            |_| Err(eyre!("boom")),
            Duration::from_millis(20),
            Duration::from_millis(5),
        )
        .run()
        .await
        .expect_err("wait_for_condition should surface predicate errors");

    assert!(error_err.to_string().contains("boom"));
    Ok(())
}

#[tokio::test]
async fn journey_assert_app_state_failure_propagates_error() -> Result<()> {
    let err = JourneyBuilder::new("assert_app_state_failure")
        .await?
        .assert_app_state("forced failure", |_| Err(eyre!("boom")))
        .run()
        .await
        .expect_err("assert_app_state should surface predicate errors");

    assert!(err.to_string().contains("boom"));
    Ok(())
}
