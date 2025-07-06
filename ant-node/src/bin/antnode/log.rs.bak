use std::path::PathBuf;

const CRITICAL_FAILURE_LOG_FILE: &str = "critical_failure.log";

pub fn set_critical_failure(log_output_dest: &str, reason: &str) {
    let log_path = PathBuf::from(log_output_dest).join(CRITICAL_FAILURE_LOG_FILE);
    let datetime_prefix = chrono::Utc::now();
    let message = format!("[{datetime_prefix}] {reason}");
    std::fs::write(log_path, message)
        .unwrap_or_else(|err| error!("Failed to write to {CRITICAL_FAILURE_LOG_FILE}: {}", err));
}

pub fn reset_critical_failure(log_output_dest: &str) {
    let log_path = PathBuf::from(log_output_dest).join(CRITICAL_FAILURE_LOG_FILE);
    if log_path.exists() {
        let _ = std::fs::remove_file(log_path);
    }
}
