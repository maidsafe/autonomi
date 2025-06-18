// Integration test for multi-node logging functionality

use ant_logging::{LogBuilder, LogOutputDest};
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;
use tracing::{info, Instrument};

#[tokio::test]
async fn test_multi_node_logging_e2e() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let log_dir = temp_dir.path().to_path_buf();

    // Test multi-node logging with 2 nodes
    let mut log_builder = LogBuilder::new(vec![(
        "test_multi_node_e2e".to_string(),
        tracing::Level::INFO,
    )]);
    log_builder.output_dest(LogOutputDest::Path(log_dir.clone()));

    let (_reload_handle, guards) = log_builder
        .initialize_with_multi_node_logging(2)
        .expect("Failed to initialize multi-node logging");

    // Log messages from different nodes
    let node_1_span = tracing::info_span!("node_1");
    let task1 = async {
        info!("Message from node 1");
        info!("Another message from node 1");

        // Test nested spans
        let inner_span = tracing::info_span!("inner_task");
        let inner_task = async {
            info!("Inner message from node 1");
        }
        .instrument(inner_span);
        inner_task.await;
    }
    .instrument(node_1_span);

    let node_2_span = tracing::info_span!("node_2");
    let task2 = async {
        info!("Message from node 2");
    }
    .instrument(node_2_span);

    // Run tasks concurrently
    tokio::join!(task1, task2);

    // Allow time for logs to be written and flushed
    tokio::time::sleep(Duration::from_millis(200)).await;
    drop(guards);
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Verify node directories were created
    let node_1_dir = log_dir.join("node_1");
    let node_2_dir = log_dir.join("node_2");

    assert!(node_1_dir.exists(), "Node 1 directory should exist");
    assert!(node_2_dir.exists(), "Node 2 directory should exist");

    // Verify each node has its own log file with correct content
    let node_1_content = read_log_content(&node_1_dir).expect("Failed to read node 1 logs");
    let node_2_content = read_log_content(&node_2_dir).expect("Failed to read node 2 logs");

    // Check node 1 logs contain all its messages
    assert!(
        node_1_content.contains("Message from node 1"),
        "Node 1 logs should contain its messages"
    );
    assert!(
        node_1_content.contains("Another message from node 1"),
        "Node 1 logs should contain all its messages"
    );
    assert!(
        node_1_content.contains("Inner message from node 1"),
        "Node 1 logs should contain nested span messages"
    );
    assert!(
        !node_1_content.contains("Message from node 2"),
        "Node 1 logs should not contain node 2 messages"
    );

    // Check node 2 logs contain only its messages
    assert!(
        node_2_content.contains("Message from node 2"),
        "Node 2 logs should contain its messages"
    );
    assert!(
        !node_2_content.contains("Message from node 1"),
        "Node 2 logs should not contain node 1 messages"
    );

    // Verify proper log formatting
    assert!(
        node_1_content.contains("test_multi_node_e2e"),
        "Should contain target name"
    );
    assert!(
        node_1_content.contains("node_1"),
        "Should contain span information"
    );
    assert!(
        node_2_content.contains("node_2"),
        "Should contain span information"
    );
}

/// Helper function to read log content from a node directory
fn read_log_content(node_dir: &PathBuf) -> Result<String, Box<dyn std::error::Error>> {
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
