// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod appender;
mod error;
pub mod layers;
#[cfg(feature = "process-metrics")]
pub mod metrics;
pub mod spawned_nodes_layers;

use crate::error::Result;
use layers::TracingLayers;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tracing::info;
use tracing_core::dispatcher::DefaultGuard;
use tracing_subscriber::{prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt};

pub use error::Error;
pub use layers::ReloadHandle;
pub use tracing_appender::non_blocking::WorkerGuard;

// re-exporting the tracing crate's Level as it is used in our public API
pub use tracing_core::Level;

// ====== CONSTANTS ======

const TIMESTAMP_FORMAT: &str = "%Y-%m-%d_%H-%M-%S";
const UNKNOWN_TEST_NAME: &str = "unknown_test";
// const UNKNOWN_FILE_NAME: &str = "unknown_file";
pub const NODE_SPAN_NAME: &str = "node";

// ====== PUBLIC TYPES ======

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
                let timestamp = chrono::Local::now().format(TIMESTAMP_FORMAT).to_string();

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
                        ))
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
    /// By default, we use log to the StdErr with the default format.
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
        self.format = format;
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

    /// Logs to the data_dir. Should be called from a single threaded tokio/non-tokio context.
    /// Provide the test file name to capture tracings from the test.
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

    /// Initialize multi-node logging with automatic test-specific directory naming
    pub fn initialize_with_multi_node_logging(
        self,
        node_count: usize,
    ) -> Result<MultiNodeLogHandle> {
        let base_log_dir = self.get_base_log_path()?;
        let targets = self.get_logging_targets()?;

        let test_name = get_thread_name().unwrap_or(UNKNOWN_TEST_NAME.to_string());

        let (routing_layer, _appender_guards) =
            self.setup_node_routing_and_appenders(&base_log_dir, node_count, targets, &test_name)?; // NEW
        let layers = self.configure_tracing_layers(routing_layer)?;

        let _subscriber_guard = tracing_subscriber::registry()
            .with(layers.layers)
            .set_default();

        let reload_handle = self.create_reload_handle();

        let multi_node_log_handle = MultiNodeLogHandle {
            base_log_dir,
            node_count,
            _appender_guards,
            _subscriber_guard,
            reload_handle,
            test_name: Some(test_name), // NEW
        };

        Ok(multi_node_log_handle)
    }

    //TODO: To be renamed once it's 100% stress tested
    /// Initialize multi-node logging with support for unique test spans
    /// Automatically extracts test name and sets up routing for client_testname and node_XX_testname patterns
    pub fn initialize_with_multi_nodes_logging_for_unique_spans(
        self,
        node_count: usize,
    ) -> Result<MultiNodeLogHandle> {
        let base_log_dir = self.get_base_log_path()?;
        let targets = self.get_logging_targets()?;

        let test_name = get_thread_name().unwrap_or(UNKNOWN_TEST_NAME.to_string());

        let (routing_layer, _appender_guards) = self.setup_unique_span_routing_and_appenders(
            &base_log_dir,
            node_count,
            targets,
            &test_name,
        )?;
        let layers = self.configure_tracing_layers_for_unique_spans(routing_layer)?;

        let _subscriber_guard = tracing_subscriber::registry()
            .with(layers.layers)
            .set_default();

        let reload_handle = self.create_reload_handle();

        let multi_node_log_handle = MultiNodeLogHandle {
            base_log_dir,
            node_count,
            _appender_guards,
            _subscriber_guard,
            reload_handle,
            test_name: Some(test_name),
        };

        Ok(multi_node_log_handle)
    }

    // ====== PRIVATE METHODS OF LogBuilder ======

    /// Set up routing layer and appenders for unique span patterns
    fn setup_unique_span_routing_and_appenders(
        &self,
        base_log_dir: &PathBuf,
        node_count: usize,
        targets: Vec<(String, Level)>,
        test_name: &str,
    ) -> Result<(
        crate::spawned_nodes_layers::UniqueSpansNodeRoutingLayer,
        Vec<WorkerGuard>,
    )> {
        let mut routing_layer =
            crate::spawned_nodes_layers::UniqueSpansNodeRoutingLayer::new(targets);
        let mut guards = Vec::new();

        // Set up client appender for client_testname spans
        let client_key = format!("client_{test_name}");
        let client_guard =
            self.setup_client_appender_with_key(base_log_dir, &mut routing_layer, &client_key)?;
        guards.push(client_guard);

        // Set up node appenders for node_XX_testname spans
        let data_root = self.calculate_data_root(base_log_dir)?;
        for i in 1..=node_count {
            let node_key = format!("node_{i:02}_{test_name}");
            let node_guard = self.setup_single_node_appender_for_unique_spans(
                &data_root,
                i,
                &mut routing_layer,
                test_name,
                &node_key,
            )?;
            guards.push(node_guard);
        }

        Ok((routing_layer, guards))
    }

    /// Modified client appender setup that accepts custom routing key
    fn setup_client_appender_with_key(
        &self,
        base_log_dir: &PathBuf,
        routing_layer: &mut crate::spawned_nodes_layers::UniqueSpansNodeRoutingLayer,
        routing_key: &str,
    ) -> Result<WorkerGuard> {
        self.create_directory(base_log_dir)?;

        let (client_appender, client_guard) = self.create_file_appender(base_log_dir);
        routing_layer.add_node_writer(routing_key.to_string(), client_appender);

        Ok(client_guard)
    }

    /// Set up node appender with unique span routing key
    fn setup_single_node_appender_for_unique_spans(
        &self,
        data_root: &Path,
        node_index: usize,
        routing_layer: &mut crate::spawned_nodes_layers::UniqueSpansNodeRoutingLayer,
        test_name: &str,
        routing_key: &str,
    ) -> Result<WorkerGuard> {
        let node_dir_name = format!("node_{node_index:02}_{test_name}");
        let node_log_dir = data_root.join(&node_dir_name).join("logs");

        self.create_directory(&node_log_dir)?;

        let (appender, guard) = self.create_file_appender(&node_log_dir);
        routing_layer.add_node_writer(routing_key.to_string(), appender);

        Ok(guard)
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

        std::env::set_var("ANT_LOG", log_pattern);

        let output_dest = match dirs_next::data_dir() {
            Some(dir) => {
                // Get the current timestamp and format it to be human readable
                let timestamp = chrono::Local::now().format(TIMESTAMP_FORMAT).to_string();

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

    /// Get the base log path, ensuring it's a file path (not stdout/stderr)
    fn get_base_log_path(&self) -> Result<PathBuf> {
        match &self.output_dest {
            LogOutputDest::Path(path) => Ok(path.clone()),
            _ => Err(Error::LoggingConfiguration(
                "Multi-node logging requires file output".to_string(),
            )),
        }
    }

    /// Get logging targets from environment or defaults
    fn get_logging_targets(&self) -> Result<Vec<(String, Level)>> {
        match std::env::var("ANT_LOG") {
            Ok(ant_log_val) => crate::layers::get_logging_targets(&ant_log_val),
            Err(_) => Ok(self.default_logging_targets.clone()),
        }
    }

    /// Set up routing layer and create all appenders with test-specific naming
    fn setup_node_routing_and_appenders(
        &self,
        base_log_dir: &PathBuf,
        node_count: usize,
        targets: Vec<(String, Level)>,
        test_name: &str, // NEW
    ) -> Result<(
        crate::spawned_nodes_layers::NodeRoutingLayer,
        Vec<WorkerGuard>,
    )> {
        let mut routing_layer = crate::spawned_nodes_layers::NodeRoutingLayer::new(targets);
        let mut guards = Vec::new();

        // Set up client appender
        let client_guard = self.setup_client_appender(base_log_dir, &mut routing_layer)?;
        guards.push(client_guard);

        // Set up node appenders with test name suffix
        let node_guards =
            self.setup_node_appenders(base_log_dir, node_count, &mut routing_layer, test_name)?; // NEW
        guards.extend(node_guards);

        Ok((routing_layer, guards))
    }

    /// Create and configure client logging appender
    fn setup_client_appender(
        &self,
        base_log_dir: &PathBuf,
        routing_layer: &mut crate::spawned_nodes_layers::NodeRoutingLayer,
    ) -> Result<WorkerGuard> {
        self.create_directory(base_log_dir)?;

        let (client_appender, client_guard) = self.create_file_appender(base_log_dir);
        routing_layer.add_node_writer("client".to_string(), client_appender);

        Ok(client_guard)
    }

    /// Create and configure all node logging appenders with test-specific naming
    fn setup_node_appenders(
        &self,
        base_log_dir: &Path,
        node_count: usize,
        routing_layer: &mut crate::spawned_nodes_layers::NodeRoutingLayer,
        test_name: &str, // NEW
    ) -> Result<Vec<WorkerGuard>> {
        let data_root = self.calculate_data_root(base_log_dir)?;
        let mut guards = Vec::new();

        for i in 1..=node_count {
            let guard = self.setup_single_node_appender(&data_root, i, routing_layer, test_name)?; // NEW
            guards.push(guard);
        }

        Ok(guards)
    }

    /// Set up logging appender for a single node with test-specific naming
    fn setup_single_node_appender(
        &self,
        data_root: &Path,
        node_index: usize,
        routing_layer: &mut crate::spawned_nodes_layers::NodeRoutingLayer,
        test_name: &str, // NEW
    ) -> Result<WorkerGuard> {
        let node_name = format!("node_{node_index:02}_{test_name}");
        let node_log_dir = data_root.join(&node_name).join("logs");

        self.create_directory(&node_log_dir)?;

        let (appender, guard) = self.create_file_appender(&node_log_dir);
        routing_layer.add_node_writer(format!("node_{node_index:02}"), appender); // NEW

        Ok(guard)
    }

    /// Calculate the data root directory from base log directory
    fn calculate_data_root(&self, base_log_dir: &Path) -> Result<PathBuf> {
        base_log_dir
            .parent() // Remove log_timestamp
            .and_then(|p| p.parent()) // Remove logs
            .and_then(|p| p.parent()) // Remove client
            .map(|p| p.to_path_buf())
            .ok_or_else(|| {
                Error::LoggingConfiguration("Could not determine data root directory".to_string())
            })
    }

    /// Create file appender with configured rotation settings
    fn create_file_appender(
        &self,
        log_dir: &PathBuf,
    ) -> (tracing_appender::non_blocking::NonBlocking, WorkerGuard) {
        appender::file_rotater_with_thread_name(
            log_dir,
            crate::layers::MAX_LOG_SIZE,
            self.max_log_files
                .unwrap_or(crate::layers::MAX_UNCOMPRESSED_LOG_FILES),
            self.max_archived_log_files
                .map(|max_archived| {
                    max_archived
                        + self
                            .max_log_files
                            .unwrap_or(crate::layers::MAX_UNCOMPRESSED_LOG_FILES)
                })
                .unwrap_or(crate::layers::MAX_LOG_FILES),
        )
    }

    /// Create a directory and handle errors
    fn create_directory(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir).map_err(|e| {
            Error::LoggingConfiguration(format!(
                "Failed to create directory {}: {}",
                dir.display(),
                e
            ))
        })
    }

    /// Configure all tracing layers including OTLP if enabled (for original NodeRoutingLayer)
    fn configure_tracing_layers(
        &self,
        routing_layer: crate::spawned_nodes_layers::NodeRoutingLayer,
    ) -> Result<TracingLayers> {
        let mut layers = TracingLayers::default();
        layers.layers.push(Box::new(routing_layer));

        self.add_otlp_layer_if_enabled(&mut layers)?;

        Ok(layers)
    }

    /// Configure all tracing layers including OTLP if enabled (for UniqueSpansNodeRoutingLayer)
    fn configure_tracing_layers_for_unique_spans(
        &self,
        routing_layer: crate::spawned_nodes_layers::UniqueSpansNodeRoutingLayer,
    ) -> Result<TracingLayers> {
        let mut layers = TracingLayers::default();
        layers.layers.push(Box::new(routing_layer));

        self.add_otlp_layer_if_enabled(&mut layers)?;

        Ok(layers)
    }

    /// Add OTLP layer if the feature is enabled and configured
    #[cfg(feature = "otlp")]
    fn add_otlp_layer_if_enabled(&self, layers: &mut TracingLayers) -> Result<()> {
        match std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
            Ok(_) => layers.otlp_layer(self.default_logging_targets.clone()),
            Err(_) => {
                println!(
                    "The OTLP feature is enabled but the OTEL_EXPORTER_OTLP_ENDPOINT variable is not \
                        set, so traces will not be submitted."
                );
                Ok(())
            }
        }
    }

    /// Add OTLP layer if the feature is enabled and configured (no-op when disabled)
    #[cfg(not(feature = "otlp"))]
    fn add_otlp_layer_if_enabled(&self, _layers: &mut TracingLayers) -> Result<()> {
        Ok(())
    }

    /// Create a reload handle for dynamic log level changes
    fn create_reload_handle(&self) -> ReloadHandle {
        let targets_filter: Box<
            dyn tracing_subscriber::layer::Filter<tracing_subscriber::Registry> + Send + Sync,
        > = Box::new(tracing_subscriber::filter::Targets::new());
        let (_, reload_handle) = tracing_subscriber::reload::Layer::new(targets_filter);
        ReloadHandle(reload_handle)
    }
}

/// Handle returned from multi-node logging initialization
/// Provides access to multi-node specific operations
// pub struct MultiNodeLogHandle {
//     base_log_dir: PathBuf,
//     node_count: usize,
//     reload_handle: ReloadHandle,
//     guards: Vec<WorkerGuard>,
// }
pub struct MultiNodeLogHandle {
    base_log_dir: PathBuf,
    node_count: usize,
    _appender_guards: Vec<WorkerGuard>, // Keep the background writer threads alive for each log appender (client and nodes)
    _subscriber_guard: DefaultGuard,    // Keep the tracing subscriber alive
    reload_handle: ReloadHandle,        // Handle for dynamic log level changes
    test_name: Option<String>,          // Add this field // NEW
}

impl MultiNodeLogHandle {
    /// Get the base log directory
    pub fn base_log_dir(&self) -> &PathBuf {
        &self.base_log_dir
    }

    /// Get the node count
    pub fn node_count(&self) -> usize {
        self.node_count
    }

    /// Get the test name (if any)
    pub fn test_name(&self) -> Option<&String> {
        self.test_name.as_ref()
    }

    /// Get the appender guards (should be held for the lifetime of the log handle)
    pub fn appender_guards(&self) -> &Vec<WorkerGuard> {
        &self._appender_guards
    }

    /// Get the reload handle for dynamic log level changes
    pub fn reload_handle(&self) -> &ReloadHandle {
        &self.reload_handle
    }

    /// Copy logs from temporary node_XX_testname directories to actual node data directories
    pub fn copy_logs_to_node_data_dirs(&self, peer_ids: &[String]) -> Result<()> {
        if peer_ids.len() != self.node_count {
            return Err(Error::LoggingConfiguration(format!(
                "Expected {} peer IDs but got {}",
                self.node_count,
                peer_ids.len()
            )));
        }

        let data_root = self
            .base_log_dir
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .ok_or_else(|| {
                Error::LoggingConfiguration("Could not determine data root directory".to_string())
            })?;

        for (i, peer_id) in peer_ids.iter().enumerate() {
            let node_index = i + 1;

            // For debugging purposes, put in comment to not pollute the CI because tests are run with --nocapture flag.
            println!("\n--- Node {node_index:02} ---");
            println!("Peer ID: {peer_id}");

            // Source: node_01_testname/logs/ or node_01/logs/ (if no test name)
            let source_dir = if let Some(ref test_name) = self.test_name {
                data_root
                    .join(format!("node_{node_index:02}_{test_name}"))
                    .join("logs") // NEW
            } else {
                data_root.join(format!("node_{node_index:02}")).join("logs") // OLD
            };

            // Destination: data_dir/node/{peer_id}/logs/
            let dest_dir = data_root.join("node").join(peer_id).join("logs");

            // For debugging purposes, put in comment to not pollute the CI because tests are run with --nocapture flag.
            println!("Source: {}", source_dir.display());
            println!("Destination: {}", dest_dir.display());

            if source_dir.exists() {
                // Create the full destination directory path (including logs subfolder)
                std::fs::create_dir_all(&dest_dir).map_err(|e| {
                    Error::LoggingConfiguration(format!(
                        "Failed to create destination directory {}: {}",
                        dest_dir.display(),
                        e
                    ))
                })?;

                // Copy all files from source to destination
                copy_dir_contents(&source_dir, &dest_dir)?;
            } else {
                println!(
                    "⚠ Source directory does not exist: {}",
                    source_dir.display()
                );
            }
        }

        Ok(())
    }

    /// Delete the temporary node_XX_testname directories after copying logs
    ///
    /// This removes temporary directories:
    /// - `node_01_testname/`
    /// - `node_02_testname/`
    /// - etc.
    ///
    /// Should typically be called after `copy_logs_to_node_data_dirs()` to clean up.
    pub fn delete_temp_node_dirs(&self) -> Result<()> {
        let data_root = self
            .base_log_dir
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .ok_or_else(|| {
                Error::LoggingConfiguration("Could not determine data root directory".to_string())
            })?;

        for i in 1..=self.node_count {
            let node_dir = if let Some(ref test_name) = self.test_name {
                // NEW
                data_root.join(format!("node_{i:02}_{test_name}")) // NEW
            } else {
                data_root.join(format!("node_{i:02}")) // OLD
            };

            if node_dir.exists() {
                std::fs::remove_dir_all(&node_dir).map_err(|e| {
                    Error::LoggingConfiguration(format!(
                        "Failed to remove temporary node directory {}: {}",
                        node_dir.display(),
                        e
                    ))
                })?;
            }
        }

        Ok(())
    }
}

// ====== PRIVATE FUNCTIONS ======

/// Recursively copy all contents from source directory to destination directory
fn copy_dir_contents(source: &Path, dest: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if source_path.is_dir() {
            fs::create_dir_all(&dest_path)?;
            copy_dir_contents(&source_path, &dest_path)?;
        } else {
            fs::copy(&source_path, &dest_path)?;
        }
    }
    Ok(())
}

// ====== PRIVATE UTILITY FUNCTIONS ======

/// Extract and clean the current thread name
/// Returns None if no thread name is available
fn get_thread_name() -> Option<String> {
    std::thread::current()
        .name()
        .map(|name| name.replace("::", "_"))
}

// ====== TESTS ======

#[cfg(test)]
mod tests {
    use crate::{layers::LogFormatter, ReloadHandle};
    use color_eyre::Result;
    use tracing::{trace, warn, Level};
    use tracing_subscriber::{
        filter::Targets,
        fmt as tracing_fmt,
        layer::{Filter, SubscriberExt},
        reload,
        util::SubscriberInitExt,
        Layer, Registry,
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
