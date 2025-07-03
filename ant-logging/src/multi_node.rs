// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Multi-node logging functionality for routing logs to separate files per node.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id};
use tracing::{Event, Level, Metadata, Subscriber};
use tracing_appender::non_blocking::NonBlocking;
use tracing_subscriber::fmt as tracing_fmt;
use tracing_subscriber::{
    filter::Targets,
    fmt::{
        format::Writer,
        time::{FormatTime, SystemTime},
        FmtContext, FormatEvent, FormatFields,
    },
    layer::Context,
    registry::LookupSpan,
    Layer,
};

/// Metadata stored with each node span for routing purposes
#[derive(Debug)]
struct NodeMetadata {
    node_name: String,
}

/// Visitor to extract node_id field from span attributes
struct NodeIdVisitor {
    node_id: Option<usize>,
}

impl Visit for NodeIdVisitor {
    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "node_id" {
            self.node_id = Some(value as usize);
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        if field.name() == "node_id" {
            self.node_id = Some(value as usize);
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "node_id" {
            // Try to extract from debug representation as fallback
            let debug_str = format!("{value:?}");
            if let Ok(parsed) = debug_str.parse::<usize>() {
                self.node_id = Some(parsed);
            }
        }
    }
}

/// Layer that routes events to different file appenders based on span context
pub struct NodeRoutingLayer {
    node_writers: Arc<Mutex<HashMap<String, NonBlocking>>>,
    targets_filter: Targets,
}

impl NodeRoutingLayer {
    pub fn new(targets: Vec<(String, Level)>) -> Self {
        Self {
            node_writers: Arc::new(Mutex::new(HashMap::new())),
            targets_filter: Targets::new().with_targets(targets),
        }
    }

    pub fn add_node_writer(&mut self, node_name: String, writer: NonBlocking) {
        let mut writers = self
            .node_writers
            .lock()
            .expect("Failed to acquire node writers lock");
        writers.insert(node_name, writer);
    }
}

impl<S> Layer<S> for NodeRoutingLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn enabled(&self, meta: &Metadata<'_>, ctx: Context<'_, S>) -> bool {
        use tracing_subscriber::layer::Filter;
        Filter::enabled(&self.targets_filter, meta, &ctx)
    }

    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let span = ctx.span(id).expect("Span should exist in registry");
        let span_name = span.name();

        // Extract node_id from spans named "node"
        if span_name == "node" {
            let mut visitor = NodeIdVisitor { node_id: None };
            attrs.record(&mut visitor);

            if let Some(node_id) = visitor.node_id {
                let node_name = format!("node_{node_id}");
                span.extensions_mut().insert(NodeMetadata { node_name });
            }
        }
    }

    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        // Find which node this event belongs to based on span hierarchy
        let mut target_node = None;

        if let Some(span_ref) = ctx.lookup_current() {
            let mut current = Some(span_ref);
            while let Some(span) = current {
                let span_name = span.name();

                // Check for dynamic node spans with stored metadata
                if span_name == "node" {
                    if let Some(metadata) = span.extensions().get::<NodeMetadata>() {
                        target_node = Some(metadata.node_name.clone());
                        break;
                    }
                }

                // Check for legacy node spans: node_1, node_2, etc. (backwards compatibility)
                if span_name.starts_with("node_") {
                    target_node = Some(span_name.to_string());
                    break;
                }

                // Check for node_other spans (for nodes > 20)
                if span_name == "node_other" {
                    // For node_other, we'll route to a default "node_other" directory
                    target_node = Some("node_other".to_string());
                    break;
                }

                current = span.parent();
            }
        }

        // Route to the appropriate writer
        if let Some(node_name) = target_node {
            let writers = self
                .node_writers
                .lock()
                .expect("Failed to acquire node writers lock");
            if let Some(writer) = writers.get(&node_name) {
                // Create a custom formatter that only shows the target node span
                let custom_formatter = NodeSpecificFormatter;

                // Create a temporary fmt layer to format and write the event
                let temp_layer = tracing_fmt::layer()
                    .with_ansi(false)
                    .with_writer(writer.clone())
                    .event_format(custom_formatter);

                // Forward the event to the temporary layer for proper formatting
                temp_layer.on_event(event, ctx);
            }
        }
    }
}

/// Custom formatter that only shows the target node span, avoiding nested node spans
pub struct NodeSpecificFormatter;

impl<S, N> FormatEvent<S, N> for NodeSpecificFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        // Write level and target
        let level = *event.metadata().level();
        let module = event.metadata().module_path().unwrap_or("<unknown module>");
        let lno = event.metadata().line().unwrap_or(0);
        let time = SystemTime;

        write!(writer, "[")?;
        time.format_time(&mut writer)?;
        write!(writer, " {level} {module} {lno}")?;

        // Only include spans up to and including the first "node" span
        // This prevents nested node spans from appearing in the output
        let mut all_spans = Vec::new();

        // First, collect all spans from current to root
        if let Some(span_ref) = ctx.lookup_current() {
            let mut current = Some(span_ref);
            while let Some(span) = current {
                all_spans.push(span.name());
                current = span.parent();
            }
        }

        // Now, find spans from root down to (and including) the first node span
        let mut spans_to_include = Vec::new();
        for span_name in all_spans.iter().rev() {
            spans_to_include.push(*span_name);
            
            // Stop after we include the first "node" span
            if *span_name == "node" || span_name.starts_with("node_") || *span_name == "node_other" {
                break;
            }
        }

        // Write spans in order (from outermost to innermost, but only up to the first node)
        for span_name in spans_to_include.iter() {
            write!(writer, "/{span_name}")?;
        }

        write!(writer, "] ")?;

        // Add the log message and any fields associated with the event
        ctx.field_format().format_fields(writer.by_ref(), event)?;

        writeln!(writer)
    }
}
