// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Screen rendering journey tests
//!
//! Tests that validate exact screen rendering through journey-based interactions.
//! These tests use isolated test configurations to ensure reproducible results.

use node_launchpad::{
    mode::Scene,
    test_utils::{JourneyBuilder, TEST_STORAGE_DRIVE, TEST_WALLET_ADDRESS},
};

#[tokio::test]
async fn journey_status_screen_renders_correctly() {
    // Create journey that starts at Status and validates the exact screen
    let expected_status_screen = &[
        " Autonomi Node Launchpad (v0.5.10)                                                                                                [S]tatus | [O]ptions | [H]elp ",
        "┌ Device Status ───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐",
        "│ Storage Allocated       35 GB                                                                                                                                │",
        "│ Memory Use              0 MB                                                                                                                                 │",
        "│ Connection              Automatic                                                                                                                            │",
        "│ Attos Earned            0                                                                                                                                    │",
        "└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘",
        "┌ Nodes (0) ───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐",
        "│                                                                                                                                                              │",
        "│ Press [+] to Add and Start your first node on this device                                                                                                    │",
        "│                                                                                                                                                              │",
        "│ Each node will use 35GB of storage and a small amount of memory, CPU, and Network bandwidth. Most computers can run many nodes at once, but we recommend you │",
        "│ add them gradually                                                                                                                                           │",
        "│                                                                                                                                                              │",
        "│                                                                                                                                                              │",
        "│                                                                                                                                                              │",
        "│                                                                                                                                                              │",
        "│                                                                                                                                                              │",
        "│                                                                                                                                                              │",
        "│                                                                                                                                                              │",
        "│                                                                                                                                                              │",
        "│                                                                                                                                                              │",
        "│                                                                                                                                                              │",
        "│                                                                                                                                                              │",
        "│                                                                                                                                                              │",
        "│                                                                                                                                                              │",
        "└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘",
        "┌──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐",
        "│ [+] Add [-] Remove [Ctrl+S] Start/Stop Node [L] Open Logs                                                               [Ctrl+G] Start All [Ctrl+X] Stop All │",
        "└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘",
    ];

    JourneyBuilder::new_with_nodes("Status Screen Rendering", 0)
        .await
        .expect("Failed to create journey")
        .start_from(Scene::Status)
        .expect_scene(Scene::Status)
        .expect_screen(expected_status_screen)
        .run()
        .await
        .expect("Status screen rendering journey failed");
}

#[tokio::test]
async fn journey_options_screen_via_navigation() {
    let expected_options_screen = vec![
        " Autonomi Node Launchpad (v0.5.10)                                                                                                [S]tatus | [O]ptions | [H]elp ".to_string(),
        "┌ Device Options ──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐".to_string(),
        format!("│ Storage Drive:     {TEST_STORAGE_DRIVE:<114} Change Drive  [Ctrl+D] │"),
        "│ Connection Mode:   Automatic                                                                                                           Change Mode  [Ctrl+K] │".to_string(),
        "│ Port Range:        Auto                                                                                                                                      │".to_string(),
        "└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘".to_string(),
        "┌ Wallet ──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐".to_string(),
        format!("│ Wallet Address:    {TEST_WALLET_ADDRESS:<114} Change Wallet  [Ctrl+B] │"),
        "└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘".to_string(),
        "┌ Access Logs ─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐".to_string(),
        "│ Open the logs folder on this device                                                                                                    Access Logs  [Ctrl+L] │".to_string(),
        "└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘".to_string(),
        "┌ Update Nodes ────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐".to_string(),
        "│ Upgrade all nodes                                                                                                                    Begin Upgrade  [Ctrl+U] │".to_string(),
        "│ Reset all nodes on this device                                                                                                         Begin Reset  [Ctrl+R] │".to_string(),
        "└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘".to_string(),
        "┌──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐".to_string(),
        "│ Close Launchpad (your nodes will keep running in the background)                                                                                    Quit [Q] │".to_string(),
        "└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘".to_string(),
        "                                                                                                                                                                ".to_string(),
        "                                                                                                                                                                ".to_string(),
        "                                                                                                                                                                ".to_string(),
        "                                                                                                                                                                ".to_string(),
        "                                                                                                                                                                ".to_string(),
        "                                                                                                                                                                ".to_string(),
        "                                                                                                                                                                ".to_string(),
        "                                                                                                                                                                ".to_string(),
        "                                                                                                                                                                ".to_string(),
        "                                                                                                                                                                ".to_string(),
        "                                                                                                                                                                ".to_string(),
    ];

    // Test navigation journey: Start at Status, navigate to Options, verify screen
    JourneyBuilder::new_with_nodes("Navigate to Options Screen", 0)
        .await
        .expect("Failed to create journey")
        .start_from(Scene::Status)
        .expect_scene(Scene::Status)
        .expect_text("Nodes (0)")
        .step()
        // Navigate to Options by pressing 'o'
        .press('o')
        .expect_scene(Scene::Options)
        .expect_screen(
            &expected_options_screen
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>(),
        )
        .run()
        .await
        .expect("Options screen navigation journey failed");
}

#[tokio::test]
async fn journey_status_to_options_and_back_with_screen_validation() {
    // Test the complete round trip with screen validation at each step
    JourneyBuilder::new("Status ↔ Options Round Trip with Screen Validation")
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
        .expect_text("Wallet Address:")
        .expect_text("Change Drive  [Ctrl+D]")
        .step()
        // Navigate back to Status
        .press('s')
        .expect_scene(Scene::Status)
        .expect_text("Nodes (0)")
        .expect_text("Press [+] to Add and Start your first node")
        .run()
        .await
        .expect("Round trip journey with screen validation failed");
}
