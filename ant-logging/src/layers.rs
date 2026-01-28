// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{LogFormat, LogOutputDest, VerbosityLevel, appender, error::Result};
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
    /// Example input: `libp2p=DEBUG,tokio=INFO,std,sn_client=ERROR`
    ///
    /// Custom keywords will take less precedence if the same target has been manually specified in the CSV.
    /// `sn_client=ERROR` in the above example will be used instead of the INFO level set by "std" keyword.
    ///
    /// Note: Dynamic modifications don't have access to application defaults, so custom targets
    /// without keywords will only log what's explicitly specified.
    pub fn modify_log_level(&self, logging_value: &str) -> Result<()> {
        // Pass empty vec for application targets since we don't have them in dynamic context
        // Pass None for verbosity since this is a runtime modification
        let targets: Vec<(String, Level)> =
            get_logging_targets(Some(logging_value), vec![], None, false)?;
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
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn fmt_layer(
        &mut self,
        application_log_targets: Vec<(String, Level)>,
        output_dest: &LogOutputDest,
        format: LogFormat,
        max_uncompressed_log_files: Option<usize>,
        max_compressed_log_files: Option<usize>,
        print_updates_to_stdout: bool,
        verbosity: Option<VerbosityLevel>,
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
        // Single function handles all verbosity/ANT_LOG logic
        let ant_log = std::env::var("ANT_LOG").ok();
        let targets = get_logging_targets(
            ant_log.as_deref(),
            application_log_targets,
            verbosity,
            print_updates_to_stdout,
        )?;

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
        application_log_targets: Vec<(String, Level)>,
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

        let ant_log_otlp = std::env::var("ANT_LOG_OTLP").ok();
        let targets = get_logging_targets(
            ant_log_otlp.as_deref(),
            application_log_targets,
            None, // No CLI verbosity for OTLP
            true, // Print updates
        )?;

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

/// Computes the final logging targets based on CLI verbosity, ANT_LOG env var, and application defaults.
///
/// Precedence for determining base targets:
/// 1. CLI `--verbosity standard/verbose` sets hardcoded base targets
/// 2. ANT_LOG keywords (`std`/`verbose`) set hardcoded base targets (when CLI is minimal/none)
/// 3. Application defaults are used otherwise
///
/// Custom overrides from ANT_LOG (e.g., `libp2p=debug`) are always applied on top of the base.
///
/// # Examples
/// - `--verbosity standard` → hardcoded INFO targets
/// - `--verbosity standard` + `ANT_LOG=libp2p=debug` → hardcoded INFO + libp2p override
/// - `ANT_LOG=std,libp2p=debug` → hardcoded INFO + libp2p override
/// - `ANT_LOG=libp2p=debug` → application defaults + libp2p override
fn get_logging_targets(
    ant_log_value: Option<&str>,
    application_log_targets: Vec<(String, Level)>,
    verbosity: Option<VerbosityLevel>,
    print_updates: bool,
) -> Result<Vec<(String, Level)>> {
    // Step 1: Parse ANT_LOG for keywords and custom overrides
    let (ant_log_keyword, custom_overrides) = parse_ant_log(ant_log_value);

    // Step 2: Determine base targets (CLI verbosity takes precedence)
    let base_targets = match verbosity {
        Some(VerbosityLevel::Standard) => {
            if print_updates {
                if custom_overrides.is_empty() {
                    println!("Using verbosity: standard");
                } else {
                    println!("Using verbosity: standard with ANT_LOG overrides");
                }
            }
            standard_targets()
        }
        Some(VerbosityLevel::Verbose) => {
            if print_updates {
                if custom_overrides.is_empty() {
                    println!("Using verbosity: verbose");
                } else {
                    println!("Using verbosity: verbose with ANT_LOG overrides");
                }
            }
            verbose_targets()
        }
        Some(VerbosityLevel::Minimal) | None => {
            // Check ANT_LOG keywords
            match ant_log_keyword {
                Some(VerbosityLevel::Standard) => {
                    if print_updates {
                        if custom_overrides.is_empty() {
                            println!("Using ANT_LOG: standard");
                        } else {
                            println!("Using ANT_LOG: standard with overrides");
                        }
                    }
                    standard_targets()
                }
                Some(VerbosityLevel::Verbose) => {
                    if print_updates {
                        if custom_overrides.is_empty() {
                            println!("Using ANT_LOG: verbose");
                        } else {
                            println!("Using ANT_LOG: verbose with overrides");
                        }
                    }
                    verbose_targets()
                }
                Some(VerbosityLevel::Minimal) | None => {
                    if print_updates {
                        if custom_overrides.is_empty() {
                            println!("Using application default log targets");
                        } else {
                            println!("Using application defaults with ANT_LOG overrides");
                        }
                    }
                    BTreeMap::from_iter(application_log_targets)
                }
            }
        }
    };

    // Step 3: Apply custom overrides from ANT_LOG on top of base
    let mut final_targets = base_targets;
    final_targets.extend(custom_overrides);

    Ok(final_targets.into_iter().collect())
}

/// Parses ANT_LOG value for keywords (std/verbose/minimal) and custom target overrides.
/// Returns (keyword_verbosity, custom_overrides)
/// Invalid entries are silently skipped.
fn parse_ant_log(ant_log_value: Option<&str>) -> (Option<VerbosityLevel>, BTreeMap<String, Level>) {
    let Some(value) = ant_log_value else {
        return (None, BTreeMap::new());
    };

    let mut keyword_verbosity = None;
    let mut custom_overrides = BTreeMap::new();

    for part in value.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Check for verbosity keywords
        if trimmed == VERBOSITY_VERBOSE || trimmed == VERBOSITY_VERBOSE_SHORT {
            keyword_verbosity = Some(VerbosityLevel::Verbose);
        } else if trimmed == VERBOSITY_STANDARD || trimmed == VERBOSITY_STANDARD_SHORT {
            keyword_verbosity = Some(VerbosityLevel::Standard);
        } else if trimmed == VERBOSITY_MINIMAL || trimmed == VERBOSITY_MINIMAL_SHORT {
            keyword_verbosity = Some(VerbosityLevel::Minimal);
        } else {
            // Parse as custom target: "crate_name=level"
            let mut split = trimmed.split('=');
            if let Some(crate_name) = split.next()
                && !crate_name.is_empty()
            {
                let log_level = split.next().unwrap_or("trace");
                if let Some(level) = parse_log_level(log_level) {
                    custom_overrides.insert(crate_name.to_string(), level);
                }
                // Invalid log level is silently skipped
            }
        }
    }

    (keyword_verbosity, custom_overrides)
}

/// Returns hardcoded INFO-level targets for standard verbosity
fn standard_targets() -> BTreeMap<String, Level> {
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
}

/// Returns hardcoded TRACE/DEBUG-level targets for verbose mode
fn verbose_targets() -> BTreeMap<String, Level> {
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
}

/// Parses a log level string, returning None for invalid values (graceful error handling)
fn parse_log_level(log_level: &str) -> Option<Level> {
    match log_level.to_lowercase().as_str() {
        "info" | "i" => Some(Level::INFO),
        "debug" | "d" => Some(Level::DEBUG),
        "trace" | "t" => Some(Level::TRACE),
        "warn" | "w" => Some(Level::WARN),
        "error" | "e" => Some(Level::ERROR),
        _ => None,
    }
}
