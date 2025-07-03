// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// Test to ensure LogFormatter and NodeSpecificFormatter produce consistent output

use ant_logging::{LogFormatter, NodeSpecificFormatter};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use tracing::info;
use tracing_subscriber::{fmt::MakeWriter, layer::SubscriberExt, Registry};

/// A test writer that captures output to a string
#[derive(Clone)]
struct TestWriter {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl TestWriter {
    fn new() -> Self {
        Self {
            buffer: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn get_output(&self) -> String {
        let buffer = self.buffer.lock().unwrap();
        String::from_utf8(buffer.clone()).unwrap()
    }
}

impl Write for TestWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for TestWriter {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[test]
fn test_formatters_consistent_output_simple_span() {
    // Test with a simple single span - both formatters should produce identical output
    let (log_formatter_output, node_formatter_output) = capture_formatter_outputs(|| {
        let span = tracing::info_span!("test_span");
        let _enter = span.enter();
        info!("Test message");
    });

    assert_eq!(
        log_formatter_output, node_formatter_output,
        "LogFormatter and NodeSpecificFormatter should produce identical output for simple spans"
    );
}

#[test]
fn test_formatters_consistent_output_nested_regular_spans() {
    // Test with nested regular spans (not node spans) - should be identical
    let (log_formatter_output, node_formatter_output) = capture_formatter_outputs(|| {
        let outer_span = tracing::info_span!("outer");
        let _outer_enter = outer_span.enter();

        let inner_span = tracing::info_span!("inner");
        let _inner_enter = inner_span.enter();

        info!("Nested message");
    });

    assert_eq!(
        log_formatter_output, node_formatter_output,
        "LogFormatter and NodeSpecificFormatter should produce identical output for nested regular spans"
    );
}

#[test]
fn test_formatters_consistent_output_single_node_span() {
    // Test with a single node span - should be identical
    let (log_formatter_output, node_formatter_output) = capture_formatter_outputs(|| {
        let node_span = tracing::info_span!("node", node_id = 1);
        let _enter = node_span.enter();
        info!("Node message");
    });

    assert_eq!(
        log_formatter_output, node_formatter_output,
        "LogFormatter and NodeSpecificFormatter should produce identical output for single node spans"
    );
}

#[test]
fn test_formatters_different_output_nested_node_spans() {
    // Test with nested node spans - this is where they should differ
    let (log_formatter_output, node_formatter_output) = capture_formatter_outputs(|| {
        let node_span_1 = tracing::info_span!("node", node_id = 1);
        let _enter_1 = node_span_1.enter();

        let node_span_2 = tracing::info_span!("node", node_id = 2);
        let _enter_2 = node_span_2.enter();

        info!("Nested node message");
    });

    // These should be different - LogFormatter will show /node/node, NodeSpecificFormatter should show /node
    assert_ne!(
        log_formatter_output, node_formatter_output,
        "LogFormatter and NodeSpecificFormatter should produce different output for nested node spans"
    );

    // Verify the specific behavior
    assert!(
        log_formatter_output.contains("/node/node"),
        "LogFormatter should show nested node spans as /node/node"
    );

    assert!(
        node_formatter_output.contains("/node") && !node_formatter_output.contains("/node/node"),
        "NodeSpecificFormatter should show only single /node for nested node spans"
    );
}

#[test]
fn test_formatters_mixed_spans() {
    // Test with a mix of regular spans and node spans
    let (log_formatter_output, node_formatter_output) = capture_formatter_outputs(|| {
        let outer_span = tracing::info_span!("outer_task");
        let _outer_enter = outer_span.enter();

        let node_span = tracing::info_span!("node", node_id = 1);
        let _node_enter = node_span.enter();

        let inner_span = tracing::info_span!("inner_task");
        let _inner_enter = inner_span.enter();

        info!("Mixed span message");
    });

    // Both should show the spans up to and including the node span
    // LogFormatter: /outer_task/node/inner_task
    // NodeSpecificFormatter: /outer_task/node (stops at first node span)

    assert!(
        log_formatter_output.contains("/outer_task/node/inner_task"),
        "LogFormatter should show all spans including those after node span. Got: '{}'",
        log_formatter_output
    );

    assert!(
        node_formatter_output.contains("/outer_task/node")
            && !node_formatter_output.contains("/inner_task"),
        "NodeSpecificFormatter should stop at the first node span. Got: '{}'",
        node_formatter_output
    );
}

/// Helper function to extract just the span and message part from formatter output
fn extract_span_and_message(output: &str) -> String {
    // Extract everything after the timestamp and level
    // Format: [timestamp LEVEL module line/spans] message
    if let Some(start) = output.find("] ") {
        // Find the spans part - everything between the line number and the closing ]
        if let Some(level_end) = output.find(" formatter_consistency ") {
            if let Some(spans_start) = output[level_end..start].find('/') {
                let spans_part = &output[level_end + spans_start..start];
                let message_part = &output[start + 2..];
                format!("{spans_part}] {message_part}")
            } else {
                // No spans
                let message_part = &output[start + 2..];
                format!("] {message_part}")
            }
        } else {
            output.to_string()
        }
    } else {
        output.to_string()
    }
}

/// Helper function to capture output from both formatters for the same operation
fn capture_formatter_outputs<F>(test_operation: F) -> (String, String)
where
    F: Fn() + Send + Sync + 'static,
{
    // Capture LogFormatter output
    let log_formatter_output = {
        let test_writer = TestWriter::new();
        let layer = tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_target(false)
            .event_format(LogFormatter)
            .with_writer(test_writer.clone());

        let subscriber = Registry::default().with(layer);

        let _guard = tracing::subscriber::set_default(subscriber);
        test_operation();

        extract_span_and_message(&test_writer.get_output())
    };

    // Capture NodeSpecificFormatter output
    let node_formatter_output = {
        let test_writer = TestWriter::new();
        let layer = tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_target(false)
            .event_format(NodeSpecificFormatter)
            .with_writer(test_writer.clone());

        let subscriber = Registry::default().with(layer);

        let _guard = tracing::subscriber::set_default(subscriber);
        test_operation();

        extract_span_and_message(&test_writer.get_output())
    };

    (log_formatter_output, node_formatter_output)
}
