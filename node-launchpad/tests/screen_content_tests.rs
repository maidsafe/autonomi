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

use color_eyre::eyre;
use node_launchpad::{
    mode::Scene,
    test_utils::{JourneyBuilder, TEST_LAUNCHPAD_VERSION, TEST_STORAGE_DRIVE, TEST_WALLET_ADDRESS},
};

#[tokio::test]
async fn journey_status_screen_renders_correctly() -> Result<(), eyre::Report> {
    // Create journey that starts at Status and validates the exact screen
    let expected_status_screen = &[
        &format!(
            " Autonomi Node Launchpad (v{TEST_LAUNCHPAD_VERSION})                                                                                                [S]tatus | [O]ptions | [H]elp "
        ),
        "┌ Device Status ───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐",
        "│ Storage Allocated       0 GB                                                                                                                                 │",
        "│ Memory Use              0 MB                                                                                                                                 │",
        "│ Connection              Automatic (UPnP)                                                                                                                     │",
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
        "│ [+] Add [-] Remove [Ctrl+S] Toggle Node [L] Open Logs                                                     [Ctrl+G] Manage [Ctrl+R] Run All [Ctrl+X] Stop All │",
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

    Ok(())
}

#[tokio::test]
async fn journey_status_screen_with_nodes_renders_correctly() -> Result<(), eyre::Report> {
    let expected_status_screen = &[
        &format!(
            " Autonomi Node Launchpad (v{TEST_LAUNCHPAD_VERSION})                                                                                                [S]tatus | [O]ptions | [H]elp "
        ),
        "┌ Device Status ───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐",
        "│ Storage Allocated       105 GB                                                                                                                               │",
        "│ Memory Use              0 MB                                                                                                                                 │",
        "│ Connection              Automatic (UPnP)                                                                                                                     │",
        "│ Attos Earned            0                                                                                                                                    │",
        "└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘",
        "┌ Nodes (3) ───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐",
        "│ Node                                                                         Version Attos Memory           Mbps Recs Peers Conns Status                     │",
        "│ antnode-1                                                                    0.1.0       0    0 MB ↓ --.- ↑ --.-    0     0     0 Running                 ⠧  │",
        "│ antnode-2                                                                    0.1.0       0    0 MB ↓ --.- ↑ --.-    0     0     0 Running                 ⠧  │",
        "│ antnode-3                                                                    0.1.0       0    0 MB ↓ --.- ↑ --.-    0     0     0 Running                 ⠧  │",
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
        "│                                                                                                                                                              │",
        "└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘",
        "┌──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐",
        "│ [+] Add [-] Remove [Ctrl+S] Toggle Node [L] Open Logs                                                     [Ctrl+G] Manage [Ctrl+R] Run All [Ctrl+X] Stop All │",
        "└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘",
    ];

    JourneyBuilder::new_with_nodes("Status Screen Rendering", 3)
        .await
        .expect("Failed to create journey")
        .start_from(Scene::Status)
        .expect_scene(Scene::Status)
        .expect_screen(expected_status_screen)
        .run()
        .await
        .expect("Status screen rendering journey failed");

    Ok(())
}

#[tokio::test]
async fn journey_options_screen_via_navigation() {
    let expected_options_screen = vec![
        format!(" Autonomi Node Launchpad (v{TEST_LAUNCHPAD_VERSION})                                                                                                [S]tatus | [O]ptions | [H]elp "),
        "┌ Device Options ──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐".to_string(),
        format!("│ Storage Drive:     {TEST_STORAGE_DRIVE:<114} Change Drive  [Ctrl+D] │"),
        "│ UPnP:              Enabled                                                                                                             Toggle UPnP  [Ctrl+U] │".to_string(),
        "│ Port Range:        Auto                                                                                                                 Edit Range  [Ctrl+P] │".to_string(),
        "└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘".to_string(),
        "┌ Wallet ──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐".to_string(),
        format!("│ Wallet Address:    {TEST_WALLET_ADDRESS:<113} Change Wallet  [Ctrl+B] │"),
        "└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘".to_string(),
        "┌ Access Logs ─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐".to_string(),
        "│ Open the logs folder on this device                                                                                                    Access Logs  [Ctrl+L] │".to_string(),
        "└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘".to_string(),
        "┌ Update Nodes ────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐".to_string(),
        "│ Upgrade all nodes                                                                                                                    Begin Upgrade  [Ctrl+G] │".to_string(),
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
async fn journey_help_screen_via_navigation() {
    let _expected_help_screen = vec![
        format!(" Autonomi Node Launchpad (v{TEST_LAUNCHPAD_VERSION})                                                                                                [S]tatus | [O]ptions | [H]elp "),
        "┌ Get Help & Support ──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐".to_string(),
        "│                                                                                                                                                              │".to_string(),
        "│ Read the quick start guides:                                                    Download the latest launchpad:                                               │".to_string(),
        "│ ]8;;https://autonomi.com/getstartedau]8;; ]8;;https://autonomi.com/getstartedto]8;; ]8;;https://autonomi.com/getstartedno]8;; ]8;;https://autonomi.com/getstartedmi]8;; ]8;;https://autonomi.com/getstarted.c]8;; ]8;;https://autonomi.com/getstartedom]8;; ]8;;https://autonomi.com/getstarted/g]8;; ]8;;https://autonomi.com/getstartedet]8;; ]8;;https://autonomi.com/getstartedst]8;; ]8;;https://autonomi.com/getstartedar]8;; ]8;;https://autonomi.com/getstartedte]8;; ]8;;https://autonomi.com/getstartedd]8;;                                                         ]8;;https://autonomi.com/downloadsau]8;; ]8;;https://autonomi.com/downloadsto]8;; ]8;;https://autonomi.com/downloadsno]8;; ]8;;https://autonomi.com/downloadsmi]8;; ]8;;https://autonomi.com/downloads.c]8;; ]8;;https://autonomi.com/downloadsom]8;; ]8;;https://autonomi.com/downloads/d]8;; ]8;;https://autonomi.com/downloadsow]8;; ]8;;https://autonomi.com/downloadsnl]8;; ]8;;https://autonomi.com/downloadsoa]8;; ]8;;https://autonomi.com/downloadsds]8;;                                                        │".to_string(),
        "│                                                                                                                                                             │".to_string(),
        "│ Get Direct Support:                                                             Terms & Conditions:                                                         │".to_string(),
        "│ autonomi.com/support                                                            autonomi.com/terms                                                          │".to_string(),
        "│                                                                                                                                                             │".to_string(),
        "└─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘".to_string(),
        "                                                                                                                                                               ".to_string(),
        "                                                                                                                                                               ".to_string(),
        "                                                                                                                                                               ".to_string(),
        "                                                                                                                                                               ".to_string(),
        "                                                                                                                                                               ".to_string(),
        "                                                                                                                                                               ".to_string(),
        "                                                                                                                                                               ".to_string(),
        "                                                                                                                                                               ".to_string(),
        "                                                                                                                                                               ".to_string(),
        "                                                                                                                                                               ".to_string(),
        "                                                                                                                                                               ".to_string(),
        "                                                                                                                                                               ".to_string(),
        "                                                                                                                                                               ".to_string(),
        "                                                                                                                                                               ".to_string(),
        "                                                                                                                                                               ".to_string(),
        "                                                                                                                                                               ".to_string(),
        "                                                                                                                                                               ".to_string(),
        "                                                                                                                                                               ".to_string(),
        "                                                                                                                                                               ".to_string(),
        "                                                                                                                                                               ".to_string(),
    ];

    // Test navigation journey: Start at Status, navigate to Help, verify screen
    JourneyBuilder::new_with_nodes("Navigate to Help Screen", 0)
        .await
        .expect("Failed to create journey")
        .start_from(Scene::Status)
        .expect_scene(Scene::Status)
        .expect_text("Nodes (0)")
        .step()
        // Navigate to Help by pressing 'h'
        .press('h')
        .expect_scene(Scene::Help)
        .expect_text("Get Help & Support")
        .expect_text("Read the quick start guides:")
        .expect_text("Get Direct Support:")
        .run()
        .await
        .expect("Help screen navigation journey failed");
}
