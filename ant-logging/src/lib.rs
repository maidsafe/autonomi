// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// Allow expect usage in logging initialization
#![allow(clippy::expect_used)]
// Allow enum variant names that end with Error as they come from external derives
#![allow(clippy::enum_variant_names)]

mod appender;
mod error;
mod layers;
#[cfg(feature = "process-metrics")]
pub mod metrics;

use crate::error::Result;
use layers::TracingLayers;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::info;
use tracing_core::dispatcher::DefaultGuard;
use tracing_subscriber::{prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt};

pub use error::Error;
pub use layers::ReloadHandle;
pub use tracing_appender::non_blocking::WorkerGuard;

// re-exporting the tracing crate's Level as it is used in our public API
pub use tracing_core::Level;

#[derive(Debug, Clone)]
pub enum LogOutputDest {
    Stderr,
    Stdout,
    Path(PathBuf),
}

impl LogOutputDest {
    pub fn parse_from_str(val: &str) -> Result<Self> {
        match val {
            "stdout" => Ok(LogOutputDest::Stdout),
            "data-dir" => {
                // Get the current timestamp and format it to be human readable
                let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();

                // Get the data directory path and append the timestamp to the log file name
                let dir = match dirs_next::data_dir() {
                    Some(dir) => dir
                        .join("autonomi")
                        .join("client")
                        .join("logs")
                        .join(format!("log_{timestamp}")),
                    None => {
                        return Err(Error::LoggingConfiguration(
                            "could not obtain data directory path".to_string(),
                        ));
                    }
                };
                Ok(LogOutputDest::Path(dir))
            }
            // The path should be a directory, but we can't use something like `is_dir` to check
            // because the path doesn't need to exist. We can create it for the user.
            value => Ok(LogOutputDest::Path(PathBuf::from(value))),
        }
    }
}

impl std::fmt::Display for LogOutputDest {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            LogOutputDest::Stderr => write!(f, "stderr"),
            LogOutputDest::Stdout => write!(f, "stdout"),
            LogOutputDest::Path(p) => write!(f, "{}", p.to_string_lossy()),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum LogFormat {
    Default,
    Json,
}

impl LogFormat {
    pub fn parse_from_str(val: &str) -> Result<Self> {
        match val {
            "default" => Ok(LogFormat::Default),
            "json" => Ok(LogFormat::Json),
            _ => Err(Error::LoggingConfiguration(
                "The only valid values for this argument are \"default\" or \"json\"".to_string(),
            )),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            LogFormat::Default => "default",
            LogFormat::Json => "json",
        }
    }
}

pub struct LogBuilder {
    default_logging_targets: Vec<(String, Level)>,
    output_dest: LogOutputDest,
    format: LogFormat,
    max_log_files: Option<usize>,
    max_archived_log_files: Option<usize>,
    /// Setting this would print the ant_logging related updates to stdout.
    print_updates_to_stdout: bool,
}

impl LogBuilder {
    /// Create a new builder
    /// Provide the default_logging_targets that are used if the `ANT_LOG` env variable is not set.
    ///
    /// By default, we use log to the StdOut with the default format.
    pub fn new(default_logging_targets: Vec<(String, Level)>) -> Self {
        Self {
            default_logging_targets,
            output_dest: LogOutputDest::Stderr,
            format: LogFormat::Default,
            max_log_files: None,
            max_archived_log_files: None,
            print_updates_to_stdout: true,
        }
    }

    /// Set the logging output destination
    pub fn output_dest(&mut self, output_dest: LogOutputDest) {
        self.output_dest = output_dest;
    }

    /// Set the logging format
    pub fn format(&mut self, format: LogFormat) {
        self.format = format
    }

    /// The max number of uncompressed log files to store
    pub fn max_log_files(&mut self, files: usize) {
        self.max_log_files = Some(files);
    }

    /// The max number of compressed files to store
    pub fn max_archived_log_files(&mut self, files: usize) {
        self.max_archived_log_files = Some(files);
    }

    /// Setting this to false would prevent ant_logging from printing things to stdout.
    pub fn print_updates_to_stdout(&mut self, print: bool) {
        self.print_updates_to_stdout = print;
    }

    /// Inits node logging, returning the NonBlocking guard if present.
    /// This guard should be held for the life of the program.
    ///
    /// Logging should be instantiated only once.
    pub fn initialize(self) -> Result<(ReloadHandle, Option<WorkerGuard>)> {
        let mut layers = TracingLayers::default();

        let reload_handle = layers.fmt_layer(
            self.default_logging_targets.clone(),
            &self.output_dest,
            self.format,
            self.max_log_files,
            self.max_archived_log_files,
            self.print_updates_to_stdout,
        )?;

        #[cfg(feature = "otlp")]
        {
            match std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
                Ok(_) => layers.otlp_layer(self.default_logging_targets)?,
                Err(_) => println!(
                    "The OTLP feature is enabled but the OTEL_EXPORTER_OTLP_ENDPOINT variable is not \
                set, so traces will not be submitted."
                ),
            }
        }

        if tracing_subscriber::registry()
            .with(layers.layers)
            .try_init()
            .is_err()
        {
            eprintln!("Tried to initialize and set global default subscriber more than once");
        }

        Ok((reload_handle, layers.log_appender_guard))
    }

    /// Logs to the data_dir with per-test log files. Should be called from a single threaded tokio/non-tokio context.
    /// Each test gets its own log file based on the test name.
    ///
    /// This function creates separate log files for each test to avoid mixing logs from different tests.
    /// The test file name is automatically detected from the test module path.
    /// subscriber.set_default() should be used if under a single threaded tokio / single threaded non-tokio context.
    /// Refer here for more details: <https://github.com/tokio-rs/tracing/discussions/1626>
    pub fn init_single_threaded_tokio_test() -> (Option<WorkerGuard>, DefaultGuard) {
        let test_name = std::thread::current()
            .name()
            .unwrap_or("unknown_test")
            .to_string();

        // Auto-detect test file name from the test name
        let test_file_name = Self::extract_test_file_name(&test_name);

        let layers = Self::get_test_layers(&test_name, &test_file_name);
        let log_guard = tracing_subscriber::registry()
            .with(layers.layers)
            .set_default();

        info!("Running test: {test_name}");
        (layers.log_appender_guard, log_guard)
    }

    /// Logs to the data_dir. Should be called from a multi threaded tokio context.
    /// The test file name is automatically detected from the test module path.
    ///
    /// subscriber.init() should be used under multi threaded tokio context. If you have 1+ multithreaded tokio tests under
    /// the same integration test, this might result in loss of logs. Hence use .init() (instead of .try_init()) to panic
    /// if called more than once.
    pub fn init_multi_threaded_tokio_test() -> Option<WorkerGuard> {
        let test_name = std::thread::current()
            .name()
            .unwrap_or("unknown_test")
            .to_string();

        // Auto-detect test file name from the test name
        let test_file_name = Self::extract_test_file_name(&test_name);

        let layers = Self::get_test_layers(&test_name, &test_file_name);
        tracing_subscriber::registry()
        .with(layers.layers)
        .try_init()
        .expect("You have tried to init multi_threaded tokio logging twice\nRefer ant_logging::get_test_layers docs for more.");

        layers.log_appender_guard
    }

    /// Extract the test file name from the test name.
    /// Test names typically follow the pattern: module::path::test_name
    /// We extract the module path and use it as the test file name.
    fn extract_test_file_name(test_name: &str) -> String {
        // Try to extract from structured test name first (e.g., "ant_bootstrap::tests::test_something")
        if let Some(module_name) = Self::extract_from_structured_test_name(test_name) {
            return module_name;
        }

        // For integration tests, try to extract from the current executable name
        if let Some(integration_test_name) = Self::extract_from_executable_name() {
            return integration_test_name;
        }

        // Fallback to the full test name if all parsing methods fail
        test_name.to_string()
    }

    /// Extract module name from structured test names like "ant_bootstrap::tests::test_something"
    fn extract_from_structured_test_name(test_name: &str) -> Option<String> {
        let parts: Vec<&str> = test_name.split("::").collect();
        if parts.len() >= 2 {
            Some(parts[0].to_string())
        } else {
            None
        }
    }

    /// Extract test name from the current executable for integration tests
    fn extract_from_executable_name() -> Option<String> {
        let current_exe = std::env::current_exe().ok()?;
        let file_name = current_exe.file_name()?;
        let exe_name = file_name.to_string_lossy();

        // Integration test binaries are typically named like "test_name-<hash>"
        // Extract the test file name part before the first dash
        exe_name.split('-').next().map(|s| s.to_string())
    }

    /// Initialize just the fmt_layer for testing purposes with per-test log files.
    ///
    /// Each test gets its own log file based on the test name to avoid mixing logs.
    /// Also overwrites the ANT_LOG variable to log everything including the test_file_name
    fn get_test_layers(test_name: &str, test_file_name: &str) -> TracingLayers {
        // overwrite ANT_LOG
        // Use a more inclusive pattern to capture all logs from the test module
        // For integration tests, we need to capture logs from the test file itself
        let log_pattern = if test_file_name.contains("_tests") || test_file_name.contains("test_") {
            // For integration tests, include the test file name directly
            format!("{test_file_name}=TRACE,all,autonomi=DEBUG,all")
        } else {
            // For unit tests, use the original pattern
            format!("{test_file_name}=TRACE,{test_file_name}::tests=TRACE,all,autonomi=DEBUG,all")
        };

        println!("Setting ANT_LOG to: {log_pattern}");

        // SAFETY: This is called during test initialization before any other threads
        // are spawned, so there's no risk of data races. Setting ANT_LOG is necessary
        // to configure logging levels for test execution.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("ANT_LOG", log_pattern);
        }

        let output_dest = match dirs_next::data_dir() {
            Some(dir) => {
                // Get the current timestamp and format it to be human readable
                let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
                // Create unique filename using test name and timestamp
                let test_name = test_name.replace("::", "_").replace(" ", "_");
                let path = dir
                    .join("autonomi")
                    .join("client")
                    .join("logs")
                    .join(format!("log_{timestamp}_{test_name}"));
                LogOutputDest::Path(path)
            }
            None => LogOutputDest::Stdout,
        };

        println!("Logging test {test_name:?} from {test_file_name:?} to {output_dest:?}");

        let mut layers = TracingLayers::default();

        let _reload_handle = layers
            .fmt_layer(vec![], &output_dest, LogFormat::Default, None, None, false)
            .expect("Failed to get TracingLayers");
        layers
    }
}

#[cfg(test)]
mod tests {
    use crate::{ReloadHandle, layers::LogFormatter};
    use color_eyre::Result;
    use tracing::{Level, trace, warn};
    use tracing_subscriber::{
        Layer, Registry,
        filter::Targets,
        fmt as tracing_fmt,
        layer::{Filter, SubscriberExt},
        reload,
        util::SubscriberInitExt,
    };
    use tracing_test::internal::global_buf;

    #[test]
    // todo: break down the TracingLayers so that we can plug in the writer without having to rewrite the whole function
    // here.
    fn reload_handle_should_change_log_levels() -> Result<()> {
        // A mock write that writes to stdout + collects events to a global buffer. We can later read from this buffer.
        let mock_writer = tracing_test::internal::MockWriter::new(global_buf());

        // Constructing the fmt layer manually.
        let layer = tracing_fmt::layer()
            .with_ansi(false)
            .with_target(false)
            .event_format(LogFormatter)
            .with_writer(mock_writer)
            .boxed();

        let test_target = "ant_logging::tests".to_string();
        // to enable logs just for the test.
        let target_filters: Box<dyn Filter<Registry> + Send + Sync> =
            Box::new(Targets::new().with_targets(vec![(test_target.clone(), Level::TRACE)]));

        // add the reload layer
        let (filter, handle) = reload::Layer::new(target_filters);
        let reload_handle = ReloadHandle(handle);
        let layer = layer.with_filter(filter);
        tracing_subscriber::registry().with(layer).try_init()?;

        // Span is not controlled by the ReloadHandle. So we can set any span here.
        let _span = tracing::info_span!("info span");

        trace!("First trace event");

        {
            let buf = global_buf().lock().unwrap();

            let events: Vec<&str> = std::str::from_utf8(&buf)
                .expect("Logs contain invalid UTF8")
                .lines()
                .collect();
            assert_eq!(events.len(), 1);
            assert!(events[0].contains("First trace event"));
        }

        reload_handle.modify_log_level("ant_logging::tests=WARN")?;

        // trace should not be logged now.
        trace!("Second trace event");
        warn!("First warn event");

        {
            let buf = global_buf().lock().unwrap();

            let events: Vec<&str> = std::str::from_utf8(&buf)
                .expect("Logs contain invalid UTF8")
                .lines()
                .collect();

            assert_eq!(events.len(), 2);
            assert!(events[0].contains("First trace event"));
            assert!(events[1].contains("First warn event"));
        }

        Ok(())
    }
}
