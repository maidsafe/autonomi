// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Log loading handlers that perform the actual file I/O operations.

use super::{LogError, MAX_LOG_FILE_SIZE, MAX_LOG_LINE_LENGTH, MAX_LOG_LINES};
use crate::action::Action;
use ant_node_manager::config::get_service_log_dir_path;
use ant_releases::ReleaseType;
use color_eyre::eyre::Context;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::fs;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{error, info, warn};
/// Load logs for a specific node
pub async fn load_logs(
    node_name: String,
    log_dir: Option<PathBuf>,
    action_sender: UnboundedSender<Action>,
) {
    info!("Loading logs for node: {node_name}");

    let result = load_logs_internal(node_name.clone(), log_dir).await;

    let action = match result {
        Ok((logs, total_lines, file_path, last_modified)) => {
            info!("Successfully loaded {total_lines} log lines for {node_name}");
            Action::LogsLoaded {
                node_name,
                logs,
                total_lines,
                file_path,
                last_modified,
            }
        }
        Err(error) => {
            error!("Failed to load logs for {node_name}: {error}");
            Action::LogsLoadError {
                node_name,
                error: error.to_string(),
            }
        }
    };

    if let Err(e) = action_sender.send(action) {
        error!("Failed to send log loading result: {e}");
    }
}

/// Internal function that performs the actual log loading with error handling and limits
async fn load_logs_internal(
    node_name: String,
    log_dir: Option<PathBuf>,
) -> Result<(Vec<String>, usize, Option<String>, Option<SystemTime>), LogError> {
    // Validate node name
    if node_name.is_empty() || node_name == "No node available" {
        return Ok((
            vec![
                "No nodes available for log viewing".to_string(),
                "".to_string(),
                "To view logs:".to_string(),
                "1. Add some nodes by pressing [+]".to_string(),
                "2. Start at least one node".to_string(),
                "3. Select a node and press [L] to view its logs".to_string(),
            ],
            6,
            None,
            None,
        ));
    }

    // Determine log directory
    let log_dir = if let Some(custom_dir) = log_dir {
        custom_dir.join("logs")
    } else {
        get_service_log_dir_path(ReleaseType::NodeLaunchpad, None, None)
            .with_context(|| format!("Failed to get log directory for {node_name}"))?
            .join(&node_name)
            .join("logs")
    };

    // Check if log directory exists
    if !log_dir.exists() {
        return Ok((
            vec![
                format!("Log directory not found for node '{node_name}'"),
                format!("Expected path: {}", log_dir.display()),
                "".to_string(),
                "This could mean:".to_string(),
                "- The node hasn't been started yet".to_string(),
                "- The node name is incorrect".to_string(),
                "- Logs are stored in a different location".to_string(),
            ],
            7,
            None,
            None,
        ));
    }

    // Find the most recent log file
    let latest_log_file = find_latest_log_file(&log_dir).await?;

    // Check file size before loading
    let metadata = fs::metadata(&latest_log_file)
        .await
        .with_context(|| format!("Failed to read metadata for {}", latest_log_file.display()))?;

    if metadata.len() > MAX_LOG_FILE_SIZE {
        return Err(LogError::FileTooLarge {
            size: metadata.len(),
            max: MAX_LOG_FILE_SIZE,
        });
    }

    // Extract file information
    let relative_file_path = latest_log_file
        .strip_prefix(&log_dir)
        .ok()
        .and_then(|p| p.to_str())
        .map(|s| s.to_string());

    let modification_time = metadata.modified().ok();

    // Load and process the log file
    let logs = load_log_file_with_limits(&latest_log_file).await?;

    if logs.is_empty() {
        return Ok((
            vec![
                format!("Log file for node '{node_name}' is empty"),
                format!("File: {}", latest_log_file.display()),
            ],
            2,
            relative_file_path,
            modification_time,
        ));
    }

    // Add header information
    let mut result_logs = vec![
        format!("=== Logs for node '{node_name}' ==="),
        format!("File: {}", latest_log_file.display()),
        format!(
            "Lines: {} (showing last {})",
            logs.len(),
            MAX_LOG_LINES.min(logs.len())
        ),
        "".to_string(),
    ];

    result_logs.extend(logs);
    let total_lines = result_logs.len();

    Ok((
        result_logs,
        total_lines,
        relative_file_path,
        modification_time,
    ))
}

/// Find the most recent log file in the directory
async fn find_latest_log_file(log_dir: &Path) -> Result<PathBuf, LogError> {
    let mut entries = fs::read_dir(log_dir)
        .await
        .with_context(|| format!("Failed to read log directory: {}", log_dir.display()))?;

    let mut log_files = Vec::new();

    while let Some(entry) = entries
        .next_entry()
        .await
        .with_context(|| "Failed to read directory entry")?
    {
        let path = entry.path();
        if path.is_file()
            && let Some(extension) = path.extension()
            && extension == "log"
        {
            let metadata = entry
                .metadata()
                .await
                .with_context(|| format!("Failed to read metadata for {}", path.display()))?;

            let modified = metadata
                .modified()
                .with_context(|| "Failed to get file modification time")?;

            log_files.push((path, modified));
        }
    }

    if log_files.is_empty() {
        return Err(LogError::NoLogFiles {
            dir: log_dir.to_path_buf(),
        });
    }

    // Sort by modification time, most recent first
    log_files.sort_by(|a, b| b.1.cmp(&a.1));

    Ok(log_files[0].0.clone())
}

/// Load a log file with memory and line limits
async fn load_log_file_with_limits(file_path: &Path) -> Result<Vec<String>, LogError> {
    let file = fs::File::open(file_path)
        .await
        .with_context(|| format!("Failed to open log file: {}", file_path.display()))?;

    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    let mut result = Vec::new();
    let mut line_count = 0;

    while let Some(line) = lines
        .next_line()
        .await
        .with_context(|| "Failed to read line from log file")?
    {
        // Enforce line count limit
        if line_count >= MAX_LOG_LINES {
            warn!("Reached maximum log line limit ({MAX_LOG_LINES}), truncating");
            break;
        }

        // Enforce line length limit
        let truncated_line = if line.len() > MAX_LOG_LINE_LENGTH {
            warn!("Truncating long log line (length: {})", line.len());
            format!("{}... [TRUNCATED]", &line[..MAX_LOG_LINE_LENGTH])
        } else {
            line
        };

        result.push(truncated_line);
        line_count += 1;
    }

    // Keep only the last MAX_LOG_LINES for performance (tail behavior)
    if result.len() > MAX_LOG_LINES {
        let skip_count = result.len() - MAX_LOG_LINES;
        result = result.into_iter().skip(skip_count).collect();
        info!("Showing last {MAX_LOG_LINES} lines from log file");
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Action;
    use tokio::sync::mpsc::unbounded_channel;

    #[tokio::test]
    async fn load_logs_reports_missing_directory_with_guidance() {
        let (tx, mut rx) = unbounded_channel();
        let missing_node = "non-existent".to_string();

        load_logs(missing_node.clone(), None, tx).await;

        let action = rx.recv().await.expect("action");
        match action {
            Action::LogsLoaded {
                node_name,
                logs,
                total_lines,
                file_path,
                last_modified,
            } => {
                assert_eq!(node_name, missing_node);
                assert_eq!(total_lines, logs.len());
                assert!(file_path.is_none(), "unexpected file path for missing logs");
                assert!(
                    logs.first()
                        .is_some_and(|line| line.contains("Log directory not found")),
                    "missing helpful guidance: {logs:?}"
                );
                assert!(last_modified.is_none());
            }
            other => panic!("expected LogsLoaded guidance, got {other:?}"),
        }
    }
}
