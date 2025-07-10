// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{get_thread_name, NODE_SPAN_ID_FIELD_NAME, NODE_SPAN_NAME, UNKNOWN_TEST_NAME};

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use tracing::{
    field::Visit,
    span::{Attributes, Id},
};
use tracing_appender::non_blocking::NonBlocking;
use tracing_core::{Event, Field, Level, Metadata, Subscriber};
use tracing_subscriber::{
    filter::Targets,
    fmt::{
        self as tracing_fmt,
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

struct SpanMetadata {
    test_name: String,
    node_id: Option<u16>, //  Optional: store the raw node_id if it exists (e.g. Client spans don't have a node_id)
    unique_span_name: String, // For routing: "node_01_testname"
    display_name: String, // For formatting: "node_XX" or "client"
}

/// Visitor to extract node_id field from span attributes
struct NodeIdVisitor {
    node_id: Option<u16>, // Changed from usize to u16
}

impl Visit for NodeIdVisitor {
    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == NODE_SPAN_ID_FIELD_NAME {
            self.node_id = Some(value as u16);
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        if field.name() == NODE_SPAN_ID_FIELD_NAME {
            self.node_id = Some(value as u16);
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == NODE_SPAN_ID_FIELD_NAME {
            // Try to extract from debug representation as fallback
            let debug_str = format!("{value:?}");
            if let Ok(parsed) = debug_str.parse::<u16>() {
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
                let node_name = format!("node_{node_id:02}");
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

                // Check for CLIENT spans
                if span_name == "client" {
                    target_node = Some("client".to_string());
                    break;
                }

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
                // Create a temporary fmt layer to format and write the event
                let temp_layer = tracing_fmt::layer()
                    .with_ansi(false)
                    .with_writer(writer.clone())
                    .event_format(SpawnedNodesLogFormatter);

                // Forward the event to the temporary layer for proper formatting
                temp_layer.on_event(event, ctx);
            }
        }
    }
}

/// Unique spans routing layer that matches exact span names to writers
pub struct UniqueSpansNodeRoutingLayer {
    node_writers: Arc<Mutex<HashMap<String, NonBlocking>>>,
    targets_filter: Targets,
}

impl UniqueSpansNodeRoutingLayer {
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

impl<S> Layer<S> for UniqueSpansNodeRoutingLayer
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

        let test_name = get_thread_name().unwrap_or(UNKNOWN_TEST_NAME.to_string());

        let mut visitor = NodeIdVisitor { node_id: None };
        attrs.record(&mut visitor);

        // Create metadata with raw extracted data
        let mut metadata = SpanMetadata {
            test_name,
            node_id: visitor.node_id,
            unique_span_name: String::new(),
            display_name: String::new(),
        };

        // Now fill computed fields based on span type using metadata values
        if span_name == NODE_SPAN_NAME {
            let node_id = metadata
                .node_id
                .expect("Node spans must have node_id field");
            metadata.unique_span_name = format!("node_{node_id:02}_{}", metadata.test_name);
            metadata.display_name = format!("node_{node_id}");
            span.extensions_mut().insert(metadata);
        } else if span_name == "client" {
            metadata.unique_span_name = format!("client_{}", metadata.test_name);
            metadata.display_name = "client".to_string();
            span.extensions_mut().insert(metadata);
        }
    }

    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        let mut target_writer_key = None;

        if let Some(span_ref) = ctx.lookup_current() {
            let mut current = Some(span_ref);

            while let Some(span) = current {
                let span_name = span.name();

                // Check for node spans FIRST (most specific)
                if span_name == NODE_SPAN_NAME && target_writer_key.is_none() {
                    if let Some(metadata) = span.extensions().get::<SpanMetadata>() {
                        target_writer_key = Some(metadata.unique_span_name.clone());
                        break;
                    }
                }

                // Check for client spans SECOND - now consistent with nodes using stored metadata
                if span_name == "client" && target_writer_key.is_none() {
                    if let Some(metadata) = span.extensions().get::<SpanMetadata>() {
                        target_writer_key = Some(metadata.unique_span_name.clone());
                        break;
                    }
                }

                current = span.parent();
            }
        }

        // Route to appropriate writer
        if let Some(writer_key) = target_writer_key {
            let writers = self
                .node_writers
                .lock()
                .expect("Failed to acquire node writers lock");

            if let Some(writer) = writers.get(&writer_key) {
                // Create a temporary fmt layer to format and write the event
                let temp_layer = tracing_fmt::layer()
                    .with_ansi(false)
                    .with_writer(writer.clone())
                    .event_format(SpawnedNodesLogFormatter);

                // Forward the event to the temporary layer for proper formatting
                temp_layer.on_event(event, ctx);
            }
        }
    }
}

/// Custom formatter that only prints the target node or client span
/// Node spans + client span are nested by order of creation when nodes are spawned in the same thread. Only the last one will be printed.
/// This is only a trick not to print the whole tree of nested spans, it doesn't fix it.
pub struct SpawnedNodesLogFormatter;

impl<S, N> FormatEvent<S, N> for SpawnedNodesLogFormatter
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
        let level = *event.metadata().level();
        let module = event.metadata().module_path().unwrap_or("<unknown module>");
        let lno = event.metadata().line().unwrap_or(0);

        write!(writer, "[")?;
        SystemTime.format_time(&mut writer)?;
        write!(writer, " {level} {module} {lno}")?;

        // No loop - just check current span directly
        if let Some(span_ref) = ctx.lookup_current() {
            if let Some(metadata) = span_ref.extensions().get::<SpanMetadata>() {
                write!(writer, "/{}", metadata.display_name)?;
            } else {
                write!(writer, "/{}", span_ref.name())?;
            }
        }

        write!(writer, "] ")?;
        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}
