// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use libp2p::PeerId;
use prometheus_parse::{Sample, Value};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, str::FromStr, time::Duration};
use thiserror::Error;
use tonic::async_trait;

#[derive(Debug, Error)]
pub enum MetricsActionError {
    #[error("Could not find PeerId while parsing the metrics")]
    PeerIdNotFound,
    #[error("Failed to parse PeerId from string")]
    PeerIdParseError,
    #[error("Connection error while attempting to fetch the url at {0}")]
    ConnectionError(String),
    #[error("Text read error while attempting to fetch the url at {0}")]
    TextReadError(String),
    #[error("Failed to parse Prometheus metrics at {0}")]
    PrometheusParseError(String),
    #[error("Reachability status check has timed out for port {metrics_port} after {timeout:?}")]
    ReachabilityStatusCheckTimedOut {
        metrics_port: u16,
        timeout: Duration,
    },
    #[error("Failed to parse PID from string")]
    PidParseError,
    #[error("PID not found")]
    PidNotFound,
    #[error("Root directory not found")]
    RootDirNotFound,
    #[error("Log directory not found")]
    LogDirNotFound,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReachabilityStatusValues {
    /// Progress percentage indicator for reachability check. 0 = not started, between 0-99 = in progress, 100 = completed.
    pub progress_percent: u8,
    /// Whether UPnP is enabled.
    pub upnp: bool,
    /// Whether the external address is same as the internal address.
    pub public: bool,
    /// Whether the external address is different from the internal address.
    pub private: bool,
}

#[derive(Debug, Clone, Default)]
pub struct NodeMetrics {
    pub reachability_status: ReachabilityStatusValues,
    pub connected_peers: u32,
}

#[derive(Debug, Clone)]
pub struct NodeMetadataExtended {
    pub peer_id: PeerId,
    pub pid: u32,
    pub root_dir: PathBuf,
    pub log_dir: PathBuf,
}

#[async_trait]
pub trait MetricsAction: Sync + Send {
    async fn get_node_metrics(&self) -> Result<NodeMetrics, MetricsActionError>;
    async fn get_node_metadata_extended(&self) -> Result<NodeMetadataExtended, MetricsActionError>;
}

#[derive(Debug, Clone)]
pub struct MetricsClient {
    port: u16,
    max_attempts: u8,
    retry_delay: Duration,
}

impl MetricsClient {
    const MAX_CONNECTION_RETRY_ATTEMPTS: u8 = 5;
    const CONNECTION_RETRY_DELAY_SEC: Duration = Duration::from_secs(1);

    pub fn new(port: u16) -> Self {
        Self {
            port,
            max_attempts: Self::MAX_CONNECTION_RETRY_ATTEMPTS,
            retry_delay: Self::CONNECTION_RETRY_DELAY_SEC,
        }
    }

    /// Set the maximum number of retry attempts when connecting to the RPC endpoint. Default is 5.
    pub fn set_max_attempts(&mut self, max_retry_attempts: u8) {
        self.max_attempts = max_retry_attempts;
    }

    /// Set the delay between retry attempts when connecting to the RPC endpoint. Default is 1 second.
    pub fn set_retry_delay(&mut self, retry_delay: Duration) {
        self.retry_delay = retry_delay;
    }

    async fn get_raw_metrics_with_retry(
        &self,
        endpoint: String,
    ) -> Result<prometheus_parse::Scrape, MetricsActionError> {
        let mut attempts = 0;
        let url = format!("http://localhost:{}/{endpoint}", self.port);

        loop {
            debug!("Attempting to read metrics from {url}...",);

            match self.get_raw_metrics(&url).await {
                Ok(all_metrics) => {
                    debug!("Metrics read successfully from {url}");
                    break Ok(all_metrics);
                }
                Err(err) => {
                    attempts += 1;
                    if attempts >= self.max_attempts {
                        error!(
                            "Failed to read metrics from {url} after {attempts} attempts: {err}"
                        );
                        return Err(err);
                    }
                    error!(
                        "Error reading metrics from {url}: {err}. Retrying {attempts}/{}",
                        self.max_attempts
                    );
                    tokio::time::sleep(self.retry_delay).await;
                }
            }
        }
    }

    async fn get_raw_metrics(
        &self,
        url: &String,
    ) -> Result<prometheus_parse::Scrape, MetricsActionError> {
        let body = reqwest::get(url)
            .await
            .map_err(|err| {
                error!("Failed to fetch metrics from {url}: {err}");
                MetricsActionError::ConnectionError(url.to_string())
            })?
            .text()
            .await
            .map_err(|err| {
                error!("Failed to read response body from {url}: {err}");
                MetricsActionError::TextReadError(url.to_string())
            })?;
        let lines: Vec<_> = body.lines().map(|s| Ok(s.to_owned())).collect();
        let all_metrics = prometheus_parse::Scrape::parse(lines.into_iter()).map_err(|err| {
            error!("Failed to parse metrics from {url}: {err}");
            MetricsActionError::PrometheusParseError(url.to_string())
        })?;

        Ok(all_metrics)
    }
}

#[async_trait]
impl MetricsAction for MetricsClient {
    /// Fetches node metrics from the "/metrics" endpoint of the node.
    async fn get_node_metrics(&self) -> Result<NodeMetrics, MetricsActionError> {
        let all_metrics = self
            .get_raw_metrics_with_retry("metrics".to_string())
            .await?;

        let connected_peers = all_metrics
            .samples
            .iter()
            .find(|s| s.metric == "ant_networking_connected_peers")
            .map(|s| s.value.clone())
            .map(|v| {
                if let Value::Gauge(value) = v {
                    value as u32
                } else {
                    error!(
                        "Expected Gauge value for 'ant_networking_connected_peers', found: {:?}",
                        v
                    );
                    0
                }
            })
            .unwrap_or(0);

        let node_metrics = NodeMetrics {
            reachability_status: ReachabilityStatusValues::from(&all_metrics.samples),
            connected_peers,
        };

        Ok(node_metrics)
    }

    async fn get_node_metadata_extended(&self) -> Result<NodeMetadataExtended, MetricsActionError> {
        let all_metrics = self
            .get_raw_metrics_with_retry("metadata_extended".to_string())
            .await?;

        let node_metadata = NodeMetadataExtended::try_from(&all_metrics.samples)?;

        Ok(node_metadata)
    }
}

impl TryFrom<&Vec<Sample>> for NodeMetadataExtended {
    type Error = MetricsActionError;

    fn try_from(samples: &Vec<Sample>) -> Result<Self, Self::Error> {
        let mut peer_id = None;
        let mut pid = None;
        let mut root_dir = None;
        let mut log_dir = None;

        for sample in samples {
            if sample.metric == "ant_networking_peer_id_info"
                && let Some(peer_id_str) = sample.labels.get("peer_id")
            {
                peer_id = Some(
                    PeerId::from_str(peer_id_str).or(Err(MetricsActionError::PeerIdParseError))?,
                );
            }

            if sample.metric == "ant_networking_pid_info"
                && let Some(pid_str) = sample.labels.get("pid")
            {
                pid = Some(
                    pid_str
                        .parse::<u32>()
                        .or(Err(MetricsActionError::PidParseError))?,
                );
            }

            if sample.metric == "ant_networking_root_dir_info"
                && let Some(root_dir_str) = sample.labels.get("root_dir")
            {
                root_dir = Some(PathBuf::from(root_dir_str));
            }

            if sample.metric == "ant_networking_log_dir_info"
                && let Some(log_dir_str) = sample.labels.get("log_dir")
            {
                log_dir = Some(PathBuf::from(log_dir_str));
            }
        }

        Ok(NodeMetadataExtended {
            peer_id: peer_id.ok_or(MetricsActionError::PeerIdNotFound)?,
            pid: pid.ok_or(MetricsActionError::PidNotFound)?,
            root_dir: root_dir.ok_or(MetricsActionError::RootDirNotFound)?,
            log_dir: log_dir.ok_or(MetricsActionError::LogDirNotFound)?,
        })
    }
}

impl From<&Vec<Sample>> for ReachabilityStatusValues {
    fn from(samples: &Vec<Sample>) -> Self {
        let mut progress_percent: u8 = 0;
        let mut upnp = false;
        let mut public = false;
        let mut private = false;

        for sample in samples {
            if sample.metric == "ant_networking_reachability_check_progress" {
                if let Value::Gauge(value) = sample.value {
                    let percent = (value * 100.0) as u8;
                    progress_percent = percent;
                } else {
                    error!(
                        "Expected Gauge value for 'ant_networking_reachability_check_progress', found: {:?}",
                        sample.value
                    );
                }
            }

            if sample.metric == "ant_networking_reachability_adapter" {
                if sample.labels.get("mode") == Some("UPnP") {
                    if let Value::Gauge(value) = sample.value {
                        upnp = value == 1.0;
                    } else {
                        error!(
                            "Expected Gauge value for 'UPnP' mode, found: {:?}",
                            sample.value
                        );
                    }
                }

                if sample.labels.get("mode") == Some("Private") {
                    if let Value::Gauge(value) = sample.value {
                        private = value == 1.0;
                    } else {
                        error!(
                            "Expected Gauge value for 'Private' mode, found: {:?}",
                            sample.value
                        );
                    }
                }

                if sample.labels.get("mode") == Some("Public") {
                    if let Value::Gauge(value) = sample.value {
                        public = value == 1.0;
                    } else {
                        error!(
                            "Expected Gauge value for 'Public' mode, found: {:?}",
                            sample.value
                        );
                    }
                }
            }
        }
        ReachabilityStatusValues {
            progress_percent,
            upnp,
            public,
            private,
        }
    }
}

#[cfg(test)]
mod tests {
    use ant_logging::LogBuilder;

    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_get_raw_metrics() {
        let _log_guard = LogBuilder::init_single_threaded_tokio_test();
        let client = MetricsClient::new(49854);
        let result = client
            .get_raw_metrics_with_retry("metrics".to_string())
            .await;
        info!("{result:?}");

        let result = client.get_node_metrics().await;
        info!("{result:?}");
    }
}
