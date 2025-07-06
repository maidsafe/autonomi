// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use serde::Serialize;
use std::time::Duration;
use sysinfo::{self, Networks, Pid, System};
use tracing::{debug, error};

const UPDATE_INTERVAL: Duration = Duration::from_secs(60);
const TO_MB: u64 = 1_000_000;

// The following Metrics are collected and logged
#[derive(Debug, Serialize)]
struct Metrics {
    // Number of threads in the system
    physical_cpu_threads: usize,
    // Percentage of CPU used
    system_cpu_usage_percent: f32,
    // Process metrics is None if the Pid for the process is invalid
    process: Option<ProcessMetrics>,
}

#[derive(Debug, Serialize)]
struct ProcessMetrics {
    // Percentage of CPU used by the process
    cpu_usage_percent: f32,
    // RAM used in MBytes
    memory_used_mb: u64,
    // Bytes read from disk during UPDATE_INTERVAL
    bytes_read: u64,
    // Bytes written to disk during UPDATE_INTERVAL
    bytes_written: u64,
    // The total MBytes read from disk by the process
    total_mb_read: u64,
    // The total MBytes written to disk by the process
    total_mb_written: u64,
}

// Obtains the system metrics every UPDATE_INTERVAL and logs it.
// The function should be spawned as a task and should be re-run if our main process is restarted.
pub async fn init_metrics(pid: u32) {
    let mut sys = System::new();
    let mut networks = Networks::new_with_refreshed_list();
    let pid = Pid::from_u32(pid);

    loop {
        refresh_metrics(&mut sys, &mut networks, pid);

        let process = match sys.process(pid) {
            Some(antnode) => {
                let disk_usage = antnode.disk_usage();
                let process = ProcessMetrics {
                    cpu_usage_percent: antnode.cpu_usage(),
                    memory_used_mb: antnode.memory() / TO_MB,
                    bytes_read: disk_usage.read_bytes,
                    bytes_written: disk_usage.written_bytes,
                    total_mb_read: disk_usage.total_read_bytes / TO_MB,
                    total_mb_written: disk_usage.total_written_bytes / TO_MB,
                };
                Some(process)
            }
            None => {
                // antnode with the provided Pid not found
                None
            }
        };

        let cpu_stat = sys.global_cpu_info();
        let metrics = Metrics {
            physical_cpu_threads: sys.cpus().len(),
            system_cpu_usage_percent: cpu_stat.cpu_usage(),
            process,
        };
        match serde_json::to_string(&metrics) {
            Ok(metrics) => debug!("{metrics}"),
            Err(err) => error!("Metrics error, could not serialize to JSON {err}"),
        }

        tokio::time::sleep(UPDATE_INTERVAL).await;
    }
}

// Refreshes only the metrics that we interested in.
fn refresh_metrics(sys: &mut System, networks: &mut Networks, pid: Pid) {
    sys.refresh_process(pid);
    sys.refresh_memory();
    sys.refresh_cpu();
    networks.refresh();
}
