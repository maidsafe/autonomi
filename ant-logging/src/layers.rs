// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    LogFormat, LogOutputDest, appender,
    error::{Error, Result},
};
use std::collections::BTreeMap;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_core::{Event, Level, Subscriber};
use tracing_subscriber::{
    Layer, Registry,
    filter::Targets,
    fmt::{
        self as tracing_fmt, FmtContext, FormatEvent, FormatFields,
        format::Writer,
        time::{FormatTime, SystemTime},
    },
    layer::Filter,
    registry::LookupSpan,
    reload::{self, Handle},
};

const MAX_LOG_SIZE: usize = 20 * 1024 * 1024;
const MAX_UNCOMPRESSED_LOG_FILES: usize = 10;
const MAX_LOG_FILES: usize = 1000;

// Verbosity level keywords for ANT_LOG environment variable
// Verbose mode: TRACE/DEBUG level logging for debugging and dev testnets
const VERBOSITY_VERBOSE: &str = "verbose";
const VERBOSITY_VERBOSE_SHORT: &str = "v";
// Standard mode: sets ALL crates to INFO level
const VERBOSITY_STANDARD: &str = "standard";
const VERBOSITY_STANDARD_SHORT: &str = "std";
// Minimal mode: uses application's default targets (no override)
const VERBOSITY_MINIMAL: &str = "minimal";
const VERBOSITY_MINIMAL_SHORT: &str = "min";

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
                // Check if path ends with .log extension
                if path.extension() == Some(std::ffi::OsStr::new("log")) {
                    // Direct file logging without rotation
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    if print_updates_to_stdout {
                        println!("Logging to file: {path:?}");
                    }

                    let file = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)?;

                    match format {
                        LogFormat::Json => tracing_fmt::layer()
                            .json()
                            .flatten_event(true)
                            .with_writer(file)
                            .boxed(),
                        LogFormat::Default => tracing_fmt::layer()
                            .with_ansi(false)
                            .with_writer(file)
                            .event_format(LogFormatter)
                            .boxed(),
                    }
                } else {
                    // Directory logging with rotation
                    std::fs::create_dir_all(path)?;
                    if print_updates_to_stdout {
                        println!("Logging to directory: {path:?}");
                    }

                    // the number of normal files
                    let max_uncompressed_log_files =
                        max_uncompressed_log_files.unwrap_or(MAX_UNCOMPRESSED_LOG_FILES);
                    // the total number of files; should be greater than uncompressed
                    let max_log_files =
                        if let Some(max_compressed_log_files) = max_compressed_log_files {
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
            KeyValue,
            sdk::{Resource, trace},
        };
        use opentelemetry_otlp::WithExportConfig;
        use opentelemetry_semantic_conventions::resource::{SERVICE_INSTANCE_ID, SERVICE_NAME};
        use rand::{Rng, distributions::Alphanumeric, thread_rng};

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
/// `export ANT_LOG = libp2p=DEBUG, tokio=INFO, verbose, autonomi=ERROR`
///
/// Supported verbosity keywords:
/// - `minimal` or `min`: Uses application's default targets (no override)
/// - `standard` or `std`: Sets ALL crates to INFO level
/// - `verbose` or `v`: Sets ALL crates to TRACE/DEBUG level for deep debugging
///
/// User-specified targets (e.g., `autonomi=ERROR`) take precedence over keyword defaults.
fn get_logging_targets(logging_env_value: &str) -> Result<Vec<(String, Level)>> {
    let mut targets = BTreeMap::new();
    let mut contains_verbose = false;
    let mut contains_standard = false;
    // Note: minimal just means "use application defaults", so we only need to skip the keyword

    for crate_log_level in logging_env_value.split(',') {
        let trimmed = crate_log_level.trim();
        // Check for verbosity keywords
        if trimmed == VERBOSITY_VERBOSE || trimmed == VERBOSITY_VERBOSE_SHORT {
            contains_verbose = true;
            continue;
        } else if trimmed == VERBOSITY_STANDARD || trimmed == VERBOSITY_STANDARD_SHORT {
            contains_standard = true;
            continue;
        } else if trimmed == VERBOSITY_MINIMAL || trimmed == VERBOSITY_MINIMAL_SHORT {
            // Minimal = use application defaults, just skip this keyword
            continue;
        }

        let mut split = trimmed.split('=');
        let crate_name = split.next().ok_or_else(|| {
            Error::LoggingConfiguration("Could not obtain crate name in logging string".to_string())
        })?;
        // Skip empty crate names
        if crate_name.is_empty() {
            continue;
        }
        let log_level = split.next().unwrap_or("trace");
        targets.insert(crate_name.to_string(), get_log_level_from_str(log_level)?);
    }

    let mut keyword_targets = if contains_verbose {
        // Verbose mode: TRACE/DEBUG for all crates
        BTreeMap::from_iter(vec![
            // bins
            ("ant".to_string(), Level::TRACE),
            ("evm_testnet".to_string(), Level::TRACE),
            ("antnode".to_string(), Level::TRACE),
            ("antctl".to_string(), Level::TRACE),
            ("node_launchpad".to_string(), Level::DEBUG),
            // libs
            ("ant_bootstrap".to_string(), Level::TRACE),
            ("ant_build_info".to_string(), Level::TRACE),
            ("ant_evm".to_string(), Level::TRACE),
            ("ant_logging".to_string(), Level::TRACE),
            ("ant_node".to_string(), Level::TRACE),
            ("ant_node_manager".to_string(), Level::TRACE),
            ("ant_node_rpc_client".to_string(), Level::TRACE),
            ("ant_protocol".to_string(), Level::TRACE),
            ("ant_service_management".to_string(), Level::TRACE),
            ("service-manager".to_string(), Level::DEBUG),
            ("autonomi".to_string(), Level::TRACE),
            ("evmlib".to_string(), Level::TRACE),
        ])
    } else if contains_standard {
        // Standard mode: INFO level for all crates
        BTreeMap::from_iter(vec![
            // bins
            ("ant".to_string(), Level::INFO),
            ("evm_testnet".to_string(), Level::INFO),
            ("antnode".to_string(), Level::INFO),
            ("antctl".to_string(), Level::INFO),
            ("node_launchpad".to_string(), Level::INFO),
            // libs
            ("ant_bootstrap".to_string(), Level::INFO),
            ("ant_build_info".to_string(), Level::INFO),
            ("ant_evm".to_string(), Level::INFO),
            ("ant_logging".to_string(), Level::INFO),
            ("ant_node".to_string(), Level::INFO),
            ("ant_node_manager".to_string(), Level::INFO),
            ("ant_node_rpc_client".to_string(), Level::INFO),
            ("ant_protocol".to_string(), Level::INFO),
            ("ant_service_management".to_string(), Level::INFO),
            ("service-manager".to_string(), Level::INFO),
            ("autonomi".to_string(), Level::INFO),
            ("evmlib".to_string(), Level::INFO),
        ])
    } else {
        // Minimal (default): empty map, use application's default targets
        Default::default()
    };
    // User-specified targets take precedence over keyword defaults
    keyword_targets.extend(targets);
    Ok(keyword_targets.into_iter().collect())
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
