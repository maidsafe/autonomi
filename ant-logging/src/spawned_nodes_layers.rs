// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::NODE_SPAN_NAME;

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
    unique_span_name: String,
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

        // Extract node_id and test name from spans named "node"
        if span_name == NODE_SPAN_NAME {
            let mut visitor = NodeIdVisitor { node_id: None };
            attrs.record(&mut visitor);

            if let Some(node_id) = visitor.node_id {
                let test_name = std::thread::current()
                    .name()
                    .map(|name| name.replace("::", "_"))
                    .unwrap_or_else(|| "unknown_test".to_string());

                let unique_node_name = format!("node_{node_id:02}_{test_name}");

                span.extensions_mut().insert(SpanMetadata {
                    unique_span_name: unique_node_name,
                });
            }
        }

        // Extract test name from spans named "client"
        if span_name == "client" {
            let test_name = std::thread::current()
                .name()
                .map(|name| name.replace("::", "_"))
                .unwrap_or_else(|| "unknown_test".to_string());

            let unique_client_name = format!("client_{test_name}");

            span.extensions_mut().insert(SpanMetadata {
                unique_span_name: unique_client_name,
            });
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

    /* TODO: Original function, to be removed once the newer function is stress tested
        fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
            let mut target_writer_key = None;

            if let Some(span_ref) = ctx.lookup_current() {
                let mut current = Some(span_ref);
                while let Some(span) = current {
                    let span_name = span.name();

                    // Check for node spans FIRST (most specific)
                    if span_name == "node" {
                        if let Some(metadata) = span.extensions().get::<NodeMetadata>() {
                            target_writer_key = Some(metadata.node_name.clone());
                            break;
                        }
                    }

                    // Check for client spans SECOND (more general)
                    if span_name == "client" {
                        let test_name = std::thread::current().name()
                            .map(|name| name.replace("::", "_"))
                            .unwrap_or_else(|| "unknown_test".to_string());
                        target_writer_key = Some(format!("client_{}", test_name));
                        break;
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
                        .event_format(LogFormatter);

                    // Forward the event to the temporary layer for proper formatting
                    temp_layer.on_event(event, ctx);
                }
            }
        }
    */
}

/// Custom formatter that only shows the target node span, avoiding nested node spans
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
        // Write level and target
        let level = *event.metadata().level();
        let module = event.metadata().module_path().unwrap_or("<unknown module>");
        let lno = event.metadata().line().unwrap_or(0);
        let time = SystemTime;

        //=======================================================================================
        
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
            if *span_name == "node" || span_name.starts_with("node_") || *span_name == "node_other"
            {
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
        
        //=======================================================================================
        
        // write!(writer, "[")?;
        // time.format_time(&mut writer)?;
        // write!(writer, " {level} {module} {lno}")?;
        // ctx.visit_spans(|span| write!(writer, "/{}", span.name()))?;
        // write!(writer, "] ")?;

        // // Add the log message and any fields associated with the event
        // ctx.field_format().format_fields(writer.by_ref(), event)?;

        // writeln!(writer)
    }
}
