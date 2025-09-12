// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Log management system for asynchronous log loading and processing.
//!
//! This module provides a separate thread-based system for handling log file operations
//! without blocking the main UI thread. It follows the same pattern as NodeManagement
//! with a task queue and dedicated thread for I/O operations.

pub mod handlers;

use crate::action::Action;
use color_eyre::Result;
use color_eyre::eyre::eyre;
use std::path::PathBuf;
use tokio::runtime::Builder;
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::task::LocalSet;
use tracing::error;

/// Memory and performance limits for log operations
const MAX_LOG_FILE_SIZE: u64 = 100 * 1024 * 1024; // 100MB
const MAX_LOG_LINES: usize = 10_000;
const MAX_LOG_LINE_LENGTH: usize = 2_000;

#[derive(Debug)]
pub enum LogManagementTask {
    /// Load logs for a specific node
    LoadLogs {
        node_name: String,
        log_dir: Option<PathBuf>,
        action_sender: UnboundedSender<Action>,
    },
}

/// Log management system that handles all log file operations on a separate thread
#[derive(Clone)]
pub struct LogManagement {
    task_sender: UnboundedSender<LogManagementTask>,
}

impl LogManagement {
    /// Create a new LogManagement instance with its own thread and runtime
    pub fn new() -> Result<Self> {
        let (send, mut recv) = mpsc::unbounded_channel();

        let rt = Builder::new_current_thread().enable_all().build()?;

        std::thread::spawn(move || {
            let local = LocalSet::new();

            local.spawn_local(async move {
                let mut last_loaded_instant = std::time::Instant::now();
                while let Some(task) = recv.recv().await {
                    match task {
                        LogManagementTask::LoadLogs {
                            node_name,
                            log_dir,
                            action_sender,
                        } => {
                            if last_loaded_instant.elapsed().as_millis() < 1000 {
                                warn!(
                                    "Log load requests are too frequent, throttling to 1 per second"
                                );
                                tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
                            } else {
                                last_loaded_instant = std::time::Instant::now();
                                handlers::load_logs(node_name, log_dir, action_sender).await;
                            }
                        }
                    }
                }
                // If the while loop returns, then all the LocalSpawner objects have been dropped.
            });

            // This will return once all senders are dropped and all spawned tasks have returned.
            rt.block_on(local);
        });

        Ok(Self { task_sender: send })
    }

    /// Send a log management task to the processing thread
    ///
    /// Tasks are executed asynchronously on a separate thread to avoid blocking the UI.
    /// Results are returned via Actions sent to the provided action_sender.
    ///
    /// If this function returns an error, it means the task could not be sent to the processing thread.
    pub fn send_task(&self, task: LogManagementTask) -> Result<()> {
        self.task_sender
            .send(task)
            .inspect_err(|err| error!("The log management thread is down: {err:?}"))
            .map_err(|_| eyre!("Failed to send task to the log management thread"))?;
        Ok(())
    }

    /// Convenience method to load logs for a node
    pub fn load_logs(
        &self,
        node_name: String,
        log_dir: Option<PathBuf>,
        action_sender: UnboundedSender<Action>,
    ) -> Result<()> {
        self.send_task(LogManagementTask::LoadLogs {
            node_name,
            log_dir,
            action_sender,
        })
    }
}

/// Structured error types for log operations
#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error("Log file too large: {size} bytes (max: {max})")]
    FileTooLarge { size: u64, max: u64 },

    #[error("No log files found in {dir}")]
    NoLogFiles { dir: PathBuf },

    #[error("Log file is empty: {file}")]
    EmptyLogFile { file: PathBuf },

    #[error("Node name invalid: '{name}'")]
    InvalidNodeName { name: String },

    #[error("IO error: {source}")]
    IoError {
        #[from]
        source: std::io::Error,
    },

    #[error("Color eyre error: {source}")]
    ColorEyreError {
        #[from]
        source: color_eyre::Report,
    },

    #[error("Log processing error: {message}")]
    ProcessingError { message: String },
}

impl From<LogError> for String {
    fn from(error: LogError) -> String {
        error.to_string()
    }
}
