// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Navigation journey tests
//!
//! Tests for basic navigation between scenes, popup handling, and focus management.

use node_launchpad::{mode::Scene, test_utils::JourneyBuilder};

#[tokio::test]
async fn journey_navigate_to_options_and_back() {
    let _log_guard = ant_logging::LogBuilder::init_single_threaded_tokio_test();
    JourneyBuilder::new("Navigate to Options and Back")
        .await
        .expect("Failed to create journey")
        .start_from(Scene::Status)
        .expect_scene(Scene::Status)
        .expect_text("Nodes (0)")
        .expect_text("Press [+] to Add and Start your first node")
        .step()
        // Navigate to Options
        .press('o')
        .expect_scene(Scene::Options)
        .expect_text("Device Options")
        .expect_text("Wallet Address")
        .expect_text("Change Drive  [Ctrl+D]")
        .step()
        // Navigate back to Status
        .press('s')
        .expect_scene(Scene::Status)
        .expect_text("Nodes (0)")
        .expect_text("Press [+] to Add and Start your first node")
        .run()
        .await
        .expect("Navigation journey failed");
}

#[tokio::test]
async fn journey_navigate_to_help_and_back() {
    let _log_guard = ant_logging::LogBuilder::init_single_threaded_tokio_test();
    JourneyBuilder::new("Navigate to Help and Back")
        .await
        .expect("Failed to create journey")
        .start_from(Scene::Status)
        .expect_scene(Scene::Status)
        .step()
        // Navigate to Help
        .press('h')
        .expect_scene(Scene::Help)
        .expect_text("Help")
        .step()
        // Navigate back to Status (ESC doesn't work in Help, use 's')
        .press('s')
        .expect_scene(Scene::Status)
        .run()
        .await
        .expect("Help navigation journey failed");
}

#[tokio::test]
async fn journey_cycle_through_all_main_scenes() {
    let _log_guard = ant_logging::LogBuilder::init_single_threaded_tokio_test();
    JourneyBuilder::new("Cycle Through All Main Scenes")
        .await
        .expect("Failed to create journey")
        .start_from(Scene::Status)
        .expect_scene(Scene::Status)
        .step()
        // Status -> Options
        .press('o')
        .expect_scene(Scene::Options)
        .step()
        // Options -> Help
        .press('h')
        .expect_scene(Scene::Help)
        .step()
        // Help -> Status (use 's' key)
        .press('s')
        .expect_scene(Scene::Status)
        .run()
        .await
        .expect("Scene cycling journey failed");
}
