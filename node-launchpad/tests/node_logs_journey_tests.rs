// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_service_management::ServiceStatus;
use color_eyre::{Result, eyre::eyre};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use node_launchpad::action::{NodeManagementCommand, NodeManagementResponse};
use node_launchpad::app::App;
use node_launchpad::components::node_table::lifecycle::LifecycleState;
use node_launchpad::components::popup::node_logs::NodeLogsPopup;
use node_launchpad::log_management::LOG_DISPLAY_LINE_LIMIT;
use node_launchpad::mode::Scene;
use node_launchpad::test_utils::{
    JourneyBuilder, KeySequence, MockNodeResponsePlan, TestAppBuilder, make_node_service_data,
};
use std::fs::{self, File};
use std::io::Write;
use std::time::Duration;

fn create_log_file<I, S>(dir: &std::path::Path, lines: I) -> Result<std::path::PathBuf>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    fs::create_dir_all(dir)?;
    let log_path = dir.join("node.log");
    let mut file = File::create(&log_path)?;
    for line in lines {
        writeln!(file, "{}", line.as_ref())?;
    }
    file.flush()?;
    Ok(log_path)
}

const SAMPLE_NODE_LOG_LINES: &[&str] = &[
    r#"[2025-09-25T11:12:50.171384Z INFO antnode 333] "#,
    r#"Running antnode v0.4.4"#,
    r#"======================"#,
    r#"[2025-09-25T11:12:50.171463Z INFO antnode 334] Antnode started with opt: Opt { alpha: false, crate_version: false, enable_metrics_server: false, evm_network: Some(EvmArbitrumOne), ip: 0.0.0.0, log_output_dest: Path("/home/autonomi/Library/Application Support/autonomi/node/antnode1/logs"), log_format: None, max_log_files: None, max_archived_log_files: None, metrics_server_port: 1234, network_id: None, no_upnp: true, package_version: false, peers: InitialPeersConfig { first: false, addrs: [], network_contacts_url: [], local: false, ignore_cache: false, bootstrap_cache_dir: None }, protocol_version: false, port: 1234, rewards_address: Some(0x03b770d9cd32077cc0bf330c13c114a87643b124), relay: false, root_dir: Some("/home/autonomi/Library/Application Support/autonomi/node/antnode1"), rpc: None, skip_reachability_check: false, version: false, write_older_cache_files: false }"#,
    r#"[2025-09-25T11:12:50.171532Z INFO antnode 335] EVM network: ArbitrumOne"#,
    r#"[2025-09-25T11:12:50.171540Z DEBUG ant_build_info 105] version: 0.4.4"#,
    r#"[2025-09-25T11:12:50.171542Z DEBUG ant_build_info 106] network version: ant/1.0/1"#,
    r#"[2025-09-25T11:12:50.171543Z DEBUG ant_build_info 107] package version: 2025.9.1.2"#,
    r#"[2025-09-25T11:12:50.171544Z DEBUG ant_build_info 108] git info: auto_conn_det_manager / f4c4693e4 / 2025-09-25"#,
    r#"[2025-09-25T11:12:50.171546Z INFO antnode 339] antnode built with git version: auto_conn_det_manager / f4c4693e4 / 2025-09-25"#,
    r#"[2025-09-25T11:12:50.173497Z INFO ant_bootstrap::initial_peers 188] Fetching bootstrap address from mainnet contacts"#,
    r#"[2025-09-25T11:12:50.173504Z INFO ant_bootstrap::contacts 112] Starting peer fetcher from 6 endpoints: [Url { scheme: \"https\", cannot_be_a_base: false, username: \"\", password: None, host: Some(Domain(\"sn-testnet.s3.eu-west-2.amazonaws.com\")), port: None, path: \"/network-contacts\", query: None, fragment: None }, Url { scheme: \"http\", cannot_be_a_base: false, username: \"\", password: None, host: Some(Ipv4(192.168.1.1)), port: None, path: \"/bootstrap_cache.json\", query: None, fragment: None }, Url { scheme: \"http\", cannot_be_a_base: false, username: \"\", password: None, host: Some(Ipv4(192.168.1.1)), port: None, path: \"/bootstrap_cache.json\", query: None, fragment: None }, Url { scheme: \"http\", cannot_be_a_base: false, username: \"\", password: None, host: Some(Ipv4(192.168.1.1)), port: None, path: \"/bootstrap_cache.json\", query: None, fragment: None }, Url { scheme: \"http\", cannot_be_a_base: false, username: \"\", password: None, host: Some(Ipv4(192.168.1.1)), port: None, path: \"/bootstrap_cache.json\", query: None, fragment: None }, Url { scheme: \"http\", cannot_be_a_base: false, username: \"\", password: None, host: Some(Ipv4(192.168.1.1)), port: None, path: \"/bootstrap_cache.json\", query: None, fragment: None }]"#,
    r#"[2025-09-25T11:12:51.205191Z INFO ant_bootstrap::contacts 246] Successfully parsed JSON response with 1500 peers"#,
    r#"[2025-09-25T11:12:51.205249Z INFO ant_bootstrap::contacts 261] Successfully parsed 1500 valid peers from JSON"#,
    r#"[2025-09-25T11:12:51.205342Z INFO ant_bootstrap::contacts 135] Successfully fetched 1500 bootstrap addrs from http://192.168.1.1/bootstrap_cache.json"#,
    r#"[2025-09-25T11:12:51.205626Z INFO ant_bootstrap::initial_peers 205] Found 100 bootstrap addresses. Returning early."#,
    r#"[2025-09-25T11:13:12.350220Z INFO ant_node::networking::reachability_check::dialer 366] Dial attempt initiated for peer with address: /ip4/192.168.1.1/udp/1234/quic-v1/p2p/12D4567....xyz"#,
];

fn node_logs_popup_component(app: &App) -> Result<&NodeLogsPopup> {
    app.components
        .iter()
        .find_map(|component| component.as_ref().as_any().downcast_ref::<NodeLogsPopup>())
        .ok_or_else(|| eyre!("Node logs popup not found"))
}

#[tokio::test]
async fn journey_viewing_node_logs_supports_navigation_and_copy() -> Result<()> {
    let _log_guard = ant_logging::LogBuilder::init_single_threaded_tokio_test();

    let first_node = make_node_service_data(0, ServiceStatus::Running);
    let second_node = make_node_service_data(1, ServiceStatus::Running);

    let maintain_plan =
        MockNodeResponsePlan::immediate(NodeManagementResponse::MaintainNodes { error: None })
            .then_registry_snapshot(vec![first_node.clone(), second_node.clone()]);

    let _log_file_path = create_log_file(
        second_node.log_dir_path.as_path(),
        SAMPLE_NODE_LOG_LINES.iter().copied(),
    )?;

    let test_app = TestAppBuilder::new()
        .with_initial_nodes([first_node.clone(), second_node.clone()])
        .with_metrics_events([node_launchpad::node_stats::AggregatedNodeStats::default()])
        .build()
        .await?;

    let header_count = 4;
    let log_lines_expected_total = SAMPLE_NODE_LOG_LINES.len() + header_count; // headers + blank spacer
    let log_lines_header = format!(
        "Lines: {} (showing last {})",
        SAMPLE_NODE_LOG_LINES.len(),
        SAMPLE_NODE_LOG_LINES.len()
    );
    let top_header_line = format!("=== Logs for node '{}' ===", second_node.service_name);
    let top_header_line_for_up = top_header_line.clone();
    let top_header_line_for_home = top_header_line.clone();
    let down_steps: usize = 10;
    let down_target_display_index = down_steps;
    let down_target_sample_index = down_target_display_index
        .checked_sub(header_count)
        .expect("Down target must land on a log entry");
    let down_target_line = *SAMPLE_NODE_LOG_LINES
        .get(down_target_sample_index)
        .expect("Sample log lines must contain enough entries for navigation");
    let up_once_display_index = down_target_display_index - 1;
    let up_once_sample_index = up_once_display_index
        .checked_sub(header_count)
        .expect("Up once should stay on log entries");
    let up_once_line = *SAMPLE_NODE_LOG_LINES
        .get(up_once_sample_index)
        .expect("Sample log lines must contain enough entries for navigation");
    let last_log_entry_index = log_lines_expected_total - 1;
    let last_log_line = *SAMPLE_NODE_LOG_LINES
        .last()
        .expect("Sample log lines must contain entries");
    let down_target_snippet = "network version: ant/1.0/1";
    let up_once_snippet = "version: 0.4.4";
    let last_log_snippet = "Dial attempt initiated";

    let mut journey = JourneyBuilder::from_context("Node logs navigation journey", test_app)?
        .with_node_action_response(NodeManagementCommand::MaintainNodes, maintain_plan)
        .assert_text("Nodes (2)")
        .step()
        .press_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL))
        .expect_scene(Scene::ManageNodesPopUp { amount_of_nodes: 2 })
        .expect_text("Using 70GB of 700GB available space")
        .step()
        .press_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
        .expect_text("Using 105GB of 700GB available space")
        .step()
        .press_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
        .expect_text("Using 140GB of 700GB available space")
        .step()
        .press_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect_scene(Scene::Status)
        .step()
        .wait_for_condition(
            "Wait for both nodes to appear",
            {
                let first = first_node.service_name.clone();
                let second = second_node.service_name.clone();
                move |app| {
                    let first_ok = node_launchpad::test_utils::node_view_model(app, &first).is_ok();
                    let second_ok =
                        node_launchpad::test_utils::node_view_model(app, &second).is_ok();
                    Ok(first_ok && second_ok)
                }
            },
            Duration::from_millis(3_000),
            Duration::from_millis(25),
        )
        .expect_node_state(&first_node.service_name, LifecycleState::Running, false)
        .expect_node_state(&second_node.service_name, LifecycleState::Running, false)
        .step()
        .press_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .assert_app_state("Second node row is selected", {
            let selected = second_node.service_name.clone();
            move |app| {
                let status = node_launchpad::test_utils::status_component(app)?;
                let state = status.node_table().state();
                let index = state
                    .controller
                    .view
                    .state
                    .selected()
                    .ok_or_else(|| color_eyre::eyre::eyre!("No node selected"))?;
                let item = state
                    .controller
                    .view
                    .items
                    .get(index)
                    .ok_or_else(|| color_eyre::eyre::eyre!("Missing selected node"))?;
                if item.id == selected {
                    Ok(())
                } else {
                    Err(color_eyre::eyre::eyre!(
                        "Expected `{}` selected, found `{}`",
                        selected,
                        item.id
                    ))
                }
            }
        })
        .step()
        .press('l')
        .expect_scene(Scene::NodeLogsPopUp)
        .step()
        .wait(Duration::from_millis(250))
        .expect_text("Node Logs - antnode-2")
        .expect_text("node.log")
        .assert_app_state("Logs metadata loaded", {
            let node_name = second_node.service_name.clone();
            let log_lines_header = log_lines_header.clone();
            let expected_last_index = last_log_entry_index;
            move |app| {
                let popup = node_logs_popup_component(app)?;
                let logs = &popup.logs;
                if logs.is_empty() {
                    return Err(eyre!("Node logs should not be empty"));
                }
                let header_expected = format!("=== Logs for node '{node_name}' ===");
                if logs.first().map(String::as_str) != Some(header_expected.as_str()) {
                    return Err(eyre!(
                        "Expected first log line `{header_expected}`, found `{}`",
                        logs.first().unwrap()
                    ));
                }
                let file_line = logs
                    .get(1)
                    .ok_or_else(|| eyre!("Missing file metadata line"))?;
                if !file_line.contains("File:") || !file_line.ends_with("node.log") {
                    return Err(eyre!(
                        "Expected file line to end with node.log, found `{file_line}`"
                    ));
                }
                let lines_line = logs
                    .get(2)
                    .ok_or_else(|| eyre!("Missing lines summary line"))?;
                if lines_line != &log_lines_header {
                    return Err(eyre!(
                        "Expected lines summary `{log_lines_header}`, found `{lines_line}`"
                    ));
                }
                let last_line = logs
                    .last()
                    .ok_or_else(|| eyre!("Missing last log line"))?;
                if last_line != last_log_line {
                    return Err(eyre!(
                        "Expected last log line `{last_log_line}`, found `{last_line}`"
                    ));
                }
                if popup.highlighted_line_index() != Some(expected_last_index) {
                    return Err(eyre!(
                        "Expected highlighted index {expected_last_index} on open"
                    ));
                }
                Ok(())
            }
        })
        .step()
        .press(KeySequence::new().repeat(KeyCode::Up, 20))
        .assert_text(&top_header_line_for_up)
        .assert_app_state(
            "Moving up 20 times highlights the first line (header)",
            {
                let top_header_line = top_header_line_for_up.clone();
                move |app| {
                    let popup = node_logs_popup_component(app)?;
                    let index = popup
                        .highlighted_line_index()
                        .ok_or_else(|| eyre!("No log line highlighted"))?;
                    if index != 0 {
                    return Err(eyre!(
                        "Expected to be on line 0 after moving up 20 times, found {index}"
                    ));
                    }
                    let line = popup
                        .highlighted_log_line()
                        .ok_or_else(|| eyre!("No log line highlighted"))?;
                    if line != top_header_line {
                        return Err(eyre!(
                            "Expected to highlight `{top_header_line}`, found `{line}`"
                        ));
                    }
                    Ok(())
                }
            },
        )
        .step()
        .press(KeySequence::new().repeat(KeyCode::Down, down_steps))
        .assert_text(down_target_snippet)
        .assert_app_state(
            "Moving down 10 times highlights the expected log entry",
            move |app| {
                let popup = node_logs_popup_component(app)?;
                let index = popup
                    .highlighted_line_index()
                    .ok_or_else(|| eyre!("No log line highlighted"))?;
                if index != down_target_display_index {
                    return Err(eyre!(
                        "Expected to be on line {down_target_display_index} after moving down 10 times, found {index}"
                    ));
                }
                let line = popup
                    .highlighted_log_line()
                    .ok_or_else(|| eyre!("No log line highlighted"))?;
                if line != down_target_line {
                    return Err(eyre!(
                        "Expected to highlight `{down_target_line}`, found `{line}`"
                    ));
                }
                Ok(())
            },
        )
        .step()
        .press_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
        .assert_text(up_once_snippet)
        .assert_app_state(
            "Moving up once highlights the previous log entry",
            move |app| {
                let popup = node_logs_popup_component(app)?;
                let index = popup
                    .highlighted_line_index()
                    .ok_or_else(|| eyre!("No log line highlighted"))?;
                if index != up_once_display_index {
                    return Err(eyre!(
                        "Expected to be on line {up_once_display_index} after moving up once, found {index}"
                    ));
                }
                let line = popup
                    .highlighted_log_line()
                    .ok_or_else(|| eyre!("No log line highlighted"))?;
                if line != up_once_line {
                    return Err(eyre!(
                        "Expected to highlight `{up_once_line}`, found `{line}`"
                    ));
                }
                Ok(())
            },
        )
        .step()
        .press_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .assert_text(down_target_snippet)
        .assert_app_state(
            "Moving down once returns to the expected log entry",
            move |app| {
                let popup = node_logs_popup_component(app)?;
                let index = popup
                    .highlighted_line_index()
                    .ok_or_else(|| eyre!("No log line highlighted"))?;
                if index != down_target_display_index {
                    return Err(eyre!(
                        "Expected to be on line {down_target_display_index} after moving down once, found {index}"
                    ));
                }
                let line = popup
                    .highlighted_log_line()
                    .ok_or_else(|| eyre!("No log line highlighted"))?;
                if line != down_target_line {
                    return Err(eyre!(
                        "Expected to highlight `{down_target_line}`, found `{line}`"
                    ));
                }
                Ok(())
            },
        )
        .step()
        .press_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE))
        .assert_text(&top_header_line_for_home)
        .assert_app_state(
            "Home jumps to the first line",
            {
                let top_header_line = top_header_line_for_home.clone();
                move |app| {
                    let popup = node_logs_popup_component(app)?;
                    let index = popup
                        .highlighted_line_index()
                        .ok_or_else(|| eyre!("No log line highlighted"))?;
                    if index != 0 {
                        return Err(eyre!(
                            "Expected to be on line 0 after pressing Home, found {index}"
                        ));
                    }
                    let line = popup
                        .highlighted_log_line()
                        .ok_or_else(|| eyre!("No log line highlighted"))?;
                    if line != top_header_line {
                        return Err(eyre!(
                            "Expected to highlight `{top_header_line}`, found `{line}`"
                        ));
                    }
                    Ok(())
                }
            },
        )
        .step()
        .press_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE))
        .assert_text(last_log_snippet)
        .assert_app_state("End jumps to the final log entry", move |app| {
            let popup = node_logs_popup_component(app)?;
            let index = popup
                .highlighted_line_index()
                .ok_or_else(|| eyre!("No log line highlighted"))?;
            if index != last_log_entry_index {
                return Err(eyre!(
                    "Expected to be on line {last_log_entry_index} after pressing End, found {index}"
                ));
            }
            let line = popup
                .highlighted_log_line()
                .ok_or_else(|| eyre!("No log line highlighted"))?;
            if line != last_log_line {
                return Err(eyre!(
                    "Expected to highlight `{last_log_line}`, found `{line}`"
                ));
            }
            Ok(())
        })
        .step()
        .press('w')
        .assert_text(last_log_snippet)
        .assert_app_state(
            "Word wrap toggles and retains the highlighted log entry",
            move |app| {
                let popup = node_logs_popup_component(app)?;
                if !popup.is_word_wrap_enabled() {
                    return Err(eyre!("Expected word wrap to be enabled"));
                }
                let index = popup
                    .highlighted_line_index()
                    .ok_or_else(|| eyre!("No log line highlighted"))?;
                if index != last_log_entry_index {
                    return Err(eyre!(
                        "Expected to remain on line {last_log_entry_index} after toggling wrap, found {index}"
                    ));
                }
                let line = popup
                    .highlighted_log_line()
                    .ok_or_else(|| eyre!("No log line highlighted"))?;
                if line != last_log_line {
                    return Err(eyre!(
                        "Expected to highlight `{last_log_line}`, found `{line}`"
                    ));
                }
                Ok(())
            },
        )
        .expect_text("[WRAP]")
        .step()
        .press_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
        .press_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT))
        .expect_text("2 lines selected")
        .step()
        .press_key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ))
        .step()
        .press_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL))
        .expect_text(&format!("{log_lines_expected_total} lines selected"))
        .step()
        .press_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .expect_scene(Scene::Status)
        .expect_text("Nodes (2)")
        .build()?;

    journey.run().await?;

    Ok(())
}

#[tokio::test]
async fn journey_viewing_node_logs_truncates_to_tail_when_limit_exceeded() -> Result<()> {
    let _log_guard = ant_logging::LogBuilder::init_single_threaded_tokio_test();

    let first_node = make_node_service_data(0, ServiceStatus::Running);
    let second_node = make_node_service_data(1, ServiceStatus::Running);

    let total_log_lines = LOG_DISPLAY_LINE_LIMIT + 42;
    let large_log_lines: Vec<String> = (0..total_log_lines)
        .map(|i| format!("[test] log entry #{i:05}"))
        .collect();
    let displayed_count = LOG_DISPLAY_LINE_LIMIT.min(total_log_lines);
    let first_tail_index = total_log_lines - displayed_count;
    let expected_first_tail_line = large_log_lines
        .get(first_tail_index)
        .expect("tail start must exist")
        .clone();
    let expected_last_line = large_log_lines
        .last()
        .expect("logs must not be empty")
        .clone();
    let expected_summary_line =
        format!("Lines: {total_log_lines} (showing last {displayed_count})",);
    let header_count = 4usize;

    let _log_file_path =
        create_log_file(second_node.log_dir_path.as_path(), large_log_lines.iter())?;

    let maintain_plan =
        MockNodeResponsePlan::immediate(NodeManagementResponse::MaintainNodes { error: None })
            .then_registry_snapshot(vec![first_node.clone(), second_node.clone()]);

    let test_app = TestAppBuilder::new()
        .with_initial_nodes([first_node.clone(), second_node.clone()])
        .with_metrics_events([node_launchpad::node_stats::AggregatedNodeStats::default()])
        .build()
        .await?;

    let expected_display_total = header_count + displayed_count;
    let expected_last_index = expected_display_total - 1;

    let mut journey = JourneyBuilder::from_context("Node logs tail truncation journey", test_app)?
        .with_node_action_response(NodeManagementCommand::MaintainNodes, maintain_plan)
        .assert_text("Nodes (2)")
        .step()
        .wait_for_condition(
            "Wait for nodes to appear",
            {
                let first = first_node.service_name.clone();
                let second = second_node.service_name.clone();
                move |app| {
                    let first_ok = node_launchpad::test_utils::node_view_model(app, &first).is_ok();
                    let second_ok =
                        node_launchpad::test_utils::node_view_model(app, &second).is_ok();
                    Ok(first_ok && second_ok)
                }
            },
            Duration::from_millis(3_000),
            Duration::from_millis(25),
        )
        .expect_node_state(&first_node.service_name, LifecycleState::Running, false)
        .expect_node_state(&second_node.service_name, LifecycleState::Running, false)
        .step()
        .press_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .press('l')
        .expect_scene(Scene::NodeLogsPopUp)
        .step()
        .wait(Duration::from_millis(350))
        .assert_app_state("Logs show the tail section when exceeding limit", {
            let expected_summary = expected_summary_line.clone();
            let expected_first_tail = expected_first_tail_line.clone();
            let expected_last = expected_last_line.clone();
            move |app| {
                let popup = node_logs_popup_component(app)?;
                let logs = &popup.logs;
                if logs.len() != expected_display_total {
                    return Err(eyre!(
                        "Expected {expected_display_total} lines including headers, found {}",
                        logs.len()
                    ));
                }
                let summary_line = logs
                    .get(2)
                    .ok_or_else(|| eyre!("Missing lines summary line"))?;
                if summary_line != &expected_summary {
                    return Err(eyre!(
                        "Expected summary `{expected_summary}`, found `{summary_line}`"
                    ));
                }
                let first_displayed = logs
                    .get(header_count)
                    .ok_or_else(|| eyre!("Missing first displayed log entry"))?;
                if first_displayed != &expected_first_tail {
                    return Err(eyre!(
                        "Expected first displayed log `{expected_first_tail}`, found `{first_displayed}`"
                    ));
                }
                let last_displayed = logs
                    .last()
                    .ok_or_else(|| eyre!("Missing last displayed log entry"))?;
                if last_displayed != &expected_last {
                    return Err(eyre!(
                        "Expected last displayed log `{expected_last}`, found `{last_displayed}`"
                    ));
                }
                if popup.highlighted_line_index() != Some(expected_last_index) {
                    return Err(eyre!(
                        "Expected highlighted index {expected_last_index}, found {:?}",
                        popup.highlighted_line_index()
                    ));
                }
                if popup.highlighted_log_line() != Some(expected_last.as_str()) {
                    return Err(eyre!(
                        "Expected highlighted log `{expected_last}`, found {:?}",
                        popup.highlighted_log_line()
                    ));
                }
                Ok(())
            }
        })
        .step()
        .press_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .expect_scene(Scene::Status)
        .build()?;

    journey.run().await?;

    Ok(())
}
