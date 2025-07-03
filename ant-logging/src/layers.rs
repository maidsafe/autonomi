// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    appender,
    error::{Error, Result},
    LogFormat, LogOutputDest,
};
use std::collections::BTreeMap;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_core::Metadata;
use tracing_core::{Event, Level, Subscriber};
use tracing_subscriber::{
    filter::Targets,
    fmt::{
        self as tracing_fmt,
        format::Writer,
        time::{FormatTime, SystemTime},
        FmtContext, FormatEvent, FormatFields,
    },
    layer::{Context, Filter},
    registry::LookupSpan,
    reload::{self, Handle},
    Layer, Registry,
};

pub(crate) const MAX_LOG_SIZE: usize = 20 * 1024 * 1024;
pub(crate) const MAX_UNCOMPRESSED_LOG_FILES: usize = 10;
pub(crate) const MAX_LOG_FILES: usize = 1000;
// Everything is logged by default
const ALL_ANT_LOGS: &str = "all";
// Trace at nodes, clients, debug at networking layer
const VERBOSE_ANT_LOGS: &str = "v";

/// Handle that implements functions to change the log level on the fly.
pub struct ReloadHandle(pub(crate) Handle<Box<dyn Filter<Registry> + Send + Sync>, Registry>);

impl ReloadHandle {
    /// Modify the log level to the provided CSV value
    /// Example input: `libp2p=DEBUG,tokio=INFO,all,sn_client=ERROR`
    ///
    /// Custom keywords will take less precedence if the same target has been manually specified in the CSV.
    /// `sn_client=ERROR` in the above example will be used instead of the TRACE level set by "all" keyword.
    pub fn modify_log_level(&self, logging_value: &str) -> Result<()> {
        let targets: Vec<(String, Level)> = get_logging_targets(logging_value)?;
        self.0.modify(|old_filter| {
            let new_filter: Box<dyn Filter<Registry> + Send + Sync> =
                Box::new(Targets::new().with_targets(targets));
            *old_filter = new_filter;
        })?;

        Ok(())
    }
}

#[derive(Default)]
/// Tracing log formatter setup for easier span viewing
pub(crate) struct LogFormatter;

impl<S, N> FormatEvent<S, N> for LogFormatter
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
        ctx.visit_spans(|span| write!(writer, "/{}", span.name()))?;
        write!(writer, "] ")?;

        // Add the log message and any fields associated with the event
        ctx.field_format().format_fields(writer.by_ref(), event)?;

        writeln!(writer)
    }
}

/// The different Subscribers composed into a list of layers
#[derive(Default)]
pub(crate) struct TracingLayers {
    pub(crate) layers: Vec<Box<dyn Layer<Registry> + Send + Sync>>,
    pub(crate) log_appender_guard: Option<WorkerGuard>,
}

impl TracingLayers {
    pub(crate) fn fmt_layer(
        &mut self,
        default_logging_targets: Vec<(String, Level)>,
        output_dest: &LogOutputDest,
        format: LogFormat,
        max_uncompressed_log_files: Option<usize>,
        max_compressed_log_files: Option<usize>,
        print_updates_to_stdout: bool,
    ) -> Result<ReloadHandle> {
        let layer = match output_dest {
            LogOutputDest::Stdout => {
                if print_updates_to_stdout {
                    println!("Logging to stdout");
                }
                tracing_fmt::layer()
                    .with_ansi(false)
                    .with_target(false)
                    .event_format(LogFormatter)
                    .boxed()
            }
            LogOutputDest::Stderr => tracing_fmt::layer()
                .with_ansi(false)
                .with_target(false)
                .event_format(LogFormatter)
                .with_writer(std::io::stderr)
                .boxed(),
            LogOutputDest::Path(path) => {
                std::fs::create_dir_all(path)?;
                if print_updates_to_stdout {
                    println!("Logging to directory: {path:?}");
                }

                // the number of normal files
                let max_uncompressed_log_files =
                    max_uncompressed_log_files.unwrap_or(MAX_UNCOMPRESSED_LOG_FILES);
                // the total number of files; should be greater than uncompressed
                let max_log_files = if let Some(max_compressed_log_files) = max_compressed_log_files
                {
                    max_compressed_log_files + max_uncompressed_log_files
                } else {
                    std::cmp::max(max_uncompressed_log_files, MAX_LOG_FILES)
                };
                let (file_rotation, worker_guard) = appender::file_rotater(
                    path,
                    MAX_LOG_SIZE,
                    max_uncompressed_log_files,
                    max_log_files,
                );
                self.log_appender_guard = Some(worker_guard);

                match format {
                    LogFormat::Json => tracing_fmt::layer()
                        .json()
                        .flatten_event(true)
                        .with_writer(file_rotation)
                        .boxed(),
                    LogFormat::Default => tracing_fmt::layer()
                        .with_ansi(false)
                        .with_writer(file_rotation)
                        .event_format(LogFormatter)
                        .boxed(),
                }
            }
        };
        let targets = match std::env::var("ANT_LOG") {
            Ok(sn_log_val) => {
                if print_updates_to_stdout {
                    println!("Using ANT_LOG={sn_log_val}");
                }
                get_logging_targets(&sn_log_val)?
            }
            Err(_) => default_logging_targets,
        };

        let target_filters: Box<dyn Filter<Registry> + Send + Sync> =
            Box::new(Targets::new().with_targets(targets));

        let (filter, reload_handle) = reload::Layer::new(target_filters);

        let layer = layer.with_filter(filter);
        self.layers.push(Box::new(layer));

        Ok(ReloadHandle(reload_handle))
    }

    #[cfg(feature = "otlp")]
    pub(crate) fn otlp_layer(
        &mut self,
        default_logging_targets: Vec<(String, Level)>,
    ) -> Result<()> {
        use opentelemetry::{
            sdk::{trace, Resource},
            KeyValue,
        };
        use opentelemetry_otlp::WithExportConfig;
        use opentelemetry_semantic_conventions::resource::{SERVICE_INSTANCE_ID, SERVICE_NAME};
        use rand::{distributions::Alphanumeric, thread_rng, Rng};

        let service_name = std::env::var("OTLP_SERVICE_NAME").unwrap_or_else(|_| {
            let random_node_name: String = thread_rng()
                .sample_iter(&Alphanumeric)
                .take(10)
                .map(char::from)
                .collect();
            random_node_name
        });
        println!("The opentelemetry traces are logged under the name: {service_name}");

        let tracer = opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_exporter(opentelemetry_otlp::new_exporter().tonic().with_env())
            .with_trace_config(trace::config().with_resource(Resource::new(vec![
                KeyValue::new(SERVICE_NAME, service_name),
                KeyValue::new(SERVICE_INSTANCE_ID, std::process::id().to_string()),
            ])))
            .install_batch(opentelemetry::runtime::Tokio)?;

        let targets = match std::env::var("ANT_LOG_OTLP") {
            Ok(sn_log_val) => {
                println!("Using ANT_LOG_OTLP={sn_log_val}");
                get_logging_targets(&sn_log_val)?
            }
            Err(_) => default_logging_targets,
        };

        let target_filters: Box<dyn Filter<Registry> + Send + Sync> =
            Box::new(Targets::new().with_targets(targets));
        let otlp_layer = tracing_opentelemetry::layer()
            .with_tracer(tracer)
            .with_filter(target_filters)
            .boxed();
        self.layers.push(otlp_layer);
        Ok(())
    }
}

/// Parses the logging targets from the env variable (ANT_LOG). The crates should be given as a CSV, for e.g.,
/// `export ANT_LOG = libp2p=DEBUG, tokio=INFO, all, sn_client=ERROR`
/// Custom keywords will take less precedence if the same target has been manually specified in the CSV.
/// `sn_client=ERROR` in the above example will be used instead of the TRACE level set by "all" keyword.
pub(crate) fn get_logging_targets(logging_env_value: &str) -> Result<Vec<(String, Level)>> {
    let mut targets = BTreeMap::new();
    let mut contains_keyword_all_sn_logs = false;
    let mut contains_keyword_verbose_sn_logs = false;

    for crate_log_level in logging_env_value.split(',') {
        // TODO: are there other default short-circuits wanted?
        // Could we have a default set if NOT on a release commit?
        if crate_log_level == ALL_ANT_LOGS {
            contains_keyword_all_sn_logs = true;
            continue;
        } else if crate_log_level == VERBOSE_ANT_LOGS {
            contains_keyword_verbose_sn_logs = true;
            continue;
        }

        let mut split = crate_log_level.split('=');
        let crate_name = split.next().ok_or_else(|| {
            Error::LoggingConfiguration("Could not obtain crate name in logging string".to_string())
        })?;
        let log_level = split.next().unwrap_or("trace");
        targets.insert(crate_name.to_string(), get_log_level_from_str(log_level)?);
    }

    let mut to_be_overriden_targets =
        if contains_keyword_all_sn_logs || contains_keyword_verbose_sn_logs {
            let mut t = BTreeMap::from_iter(vec![
                // bins
                ("autonomi_cli".to_string(), Level::TRACE),
                ("evm_testnet".to_string(), Level::TRACE),
                ("antnode".to_string(), Level::TRACE),
                ("antnode_rpc_client".to_string(), Level::TRACE),
                ("antctl".to_string(), Level::TRACE),
                ("antctld".to_string(), Level::TRACE),
                // libs
                ("ant_bootstrap".to_string(), Level::TRACE),
                ("ant_build_info".to_string(), Level::TRACE),
                ("ant_evm".to_string(), Level::TRACE),
                ("ant_logging".to_string(), Level::TRACE),
                ("ant_node_manager".to_string(), Level::TRACE),
                ("ant_node_rpc_client".to_string(), Level::TRACE),
                ("ant_protocol".to_string(), Level::TRACE),
                ("ant_service_management".to_string(), Level::TRACE),
                ("autonomi".to_string(), Level::TRACE),
                ("evmlib".to_string(), Level::TRACE),
            ]);

            if !t.contains_key("ant_node") {
                if contains_keyword_all_sn_logs {
                    t.insert("ant_node".to_string(), Level::TRACE)
                } else if contains_keyword_verbose_sn_logs {
                    t.insert("ant_node".to_string(), Level::DEBUG)
                } else {
                    t.insert("ant_node".to_string(), Level::INFO)
                };
            }
            t
        } else {
            Default::default()
        };
    to_be_overriden_targets.extend(targets);
    Ok(to_be_overriden_targets.into_iter().collect())
}

fn get_log_level_from_str(log_level: &str) -> Result<Level> {
    match log_level.to_lowercase().as_str() {
        "info" => Ok(Level::INFO),
        "debug" => Ok(Level::DEBUG),
        "trace" => Ok(Level::TRACE),
        "warn" => Ok(Level::WARN),
        "error" => Ok(Level::WARN),
        _ => Err(Error::LoggingConfiguration(format!(
            "Log level {log_level} is not supported"
        ))),
    }
}

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id};
use tracing_appender::non_blocking::NonBlocking;

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
struct NodeSpecificFormatter;

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
        let mut spans_to_include = Vec::new();

        if let Some(span_ref) = ctx.lookup_current() {
            let mut current = Some(span_ref);

            while let Some(span) = current {
                let span_name = span.name();
                spans_to_include.push(span_name);

                // Stop collecting spans after we find the first "node" span
                if span_name == "node" {
                    break;
                }

                // Also stop for legacy node spans
                if span_name.starts_with("node_") || span_name == "node_other" {
                    break;
                }

                current = span.parent();
            }
        }

        // Write spans in reverse order (from outermost to innermost, but only up to the first node)
        for span_name in spans_to_include.iter().rev() {
            write!(writer, "/{span_name}")?;
        }

        write!(writer, "] ")?;

        // Add the log message and any fields associated with the event
        ctx.field_format().format_fields(writer.by_ref(), event)?;

        writeln!(writer)
    }
}
