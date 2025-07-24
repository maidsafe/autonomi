// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// Test to demonstrate that the nested span issue is fixed

use ant_logging::{LogBuilder, LogOutputDest};
use std::time::Duration;
use tempfile::TempDir;
use tracing::info;

#[tokio::test]
#[ignore] // TODO: Fix path and potential issues with temp folder needing root
async fn test_nested_spans_are_fixed() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let log_dir = temp_dir.path().to_path_buf();

    // Test multi-node logging with intentionally nested spans
    let mut log_builder = LogBuilder::new(vec![(
        "test_nested_span_fix".to_string(),
        tracing::Level::INFO,
    )]);
    log_builder.output_dest(LogOutputDest::Path(log_dir.clone()));

    let multi_node_log_handle = log_builder
        .initialize_with_multi_nodes_logging_for_unique_spans_at(3, Some(log_dir.clone()))
        .expect("Failed to initialize multi-node logging");

    // Create nested spans intentionally - this used to cause the issue
    {
        let node_span_1 = tracing::info_span!("node", node_id = 1);
        let _enter_1 = node_span_1.enter();
        info!("Message from node 1 - outer");

        {
            let node_span_2 = tracing::info_span!("node", node_id = 2);
            let _enter_2 = node_span_2.enter();
            info!("Message from node 2 - middle");

            {
                let node_span_3 = tracing::info_span!("node", node_id = 3);
                let _enter_3 = node_span_3.enter();
                info!("Message from node 3 - inner");
            }
        }
    }

    // Allow time for logs to be written and flushed
    tokio::time::sleep(Duration::from_millis(200)).await;
    drop(multi_node_log_handle);
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Verify that each node only shows ONE "/node" span in their logs (not nested)
    for i in 1..=3 {
        let node_dir = log_dir.join(format!("node_{i}"));
        assert!(node_dir.exists(), "Node {i} directory should exist");

        if let Ok(node_content) = read_log_content(&node_dir) {
            println!("Node {i} logs: {node_content}");

            // Each line should only have one "/node" in the span path
            for line in node_content.lines() {
                let node_count = line.matches("/node").count();
                assert_eq!(
                    node_count, 1,
                    "Node {i} should have exactly 1 '/node' span, but found {node_count} in line: '{line}'"
                );
            }

            // Verify this node's message exists
            assert!(
                node_content.contains(&format!("Message from node {i}")),
                "Node {i} logs should contain its message"
            );
        }
    }
}

/// Helper function to read log content from a node directory  
fn read_log_content(node_dir: &std::path::PathBuf) -> Result<String, Box<dyn std::error::Error>> {
    let mut content = String::new();

    for entry in std::fs::read_dir(node_dir)? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|ext| ext == "log") {
            let file_content = std::fs::read_to_string(entry.path())?;
            content.push_str(&file_content);
        }
    }

    if content.is_empty() {
        return Err("No log content found".into());
    }

    Ok(content)
}
