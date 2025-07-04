// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::Error;
use prometheus_parse::{Sample, Value};
use std::time::Duration;
use tonic::async_trait;

const REACHABILITY_STATUS_METRIC: &str = "ant_networking_reachability_status";

const REACHABILITY_CHECK_TIMEOUT_SEC: u64 = 14 * 60;

#[derive(Debug, Clone, Default)]
pub struct ReachabilityStatusValues {
    pub not_performed: bool,
    pub ongoing: bool,
    pub reachable: bool,
    pub relay: bool,
    pub not_routable: bool,
    pub upnp: bool,
}

#[derive(Debug, Clone, Default)]
pub struct NodeMetrics {
    pub reachability_status: ReachabilityStatusValues,
}

#[async_trait]
pub trait MetricsAction: Sync + Send {
    async fn get_node_metrics(&self) -> Result<NodeMetrics, Error>;
    async fn wait_until_reachability_check_completes(
        &self,
        timeout: Option<Duration>,
    ) -> Result<(), Error>;
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
    ) -> Result<prometheus_parse::Scrape, Error> {
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

    async fn get_raw_metrics(&self, url: &String) -> Result<prometheus_parse::Scrape, Error> {
        let body = reqwest::get(url)
            .await
            .map_err(|err| {
                error!("Failed to fetch metrics from {url}: {err}");
                Error::MetricsConnectionError(format!("Connection error to {url}"))
            })?
            .text()
            .await
            .map_err(|err| {
                error!("Failed to read response body from {url}: {err}");
                Error::MetricsConnectionError(format!("Text read error from {url}"))
            })?;
        let lines: Vec<_> = body.lines().map(|s| Ok(s.to_owned())).collect();
        let all_metrics = prometheus_parse::Scrape::parse(lines.into_iter()).map_err(|err| {
            error!("Failed to parse metrics from {url}: {err}");
            Error::MetricsParseError
        })?;

        Ok(all_metrics)
    }
}

#[async_trait]
impl MetricsAction for MetricsClient {
    /// Fetches node metrics from the "/metrics" endpoint of the node.
    async fn get_node_metrics(&self) -> Result<NodeMetrics, Error> {
        let all_metrics = self
            .get_raw_metrics_with_retry("metrics".to_string())
            .await?;

        let node_metrics = NodeMetrics {
            reachability_status: ReachabilityStatusValues::from(&all_metrics.samples),
        };

        Ok(node_metrics)
    }

    /// Waits until the reachability check completes or times out.
    ///
    /// The default timeout is set to 14 minutes, which is the maximum time the reachability check can take.
    async fn wait_until_reachability_check_completes(
        &self,
        timeout: Option<Duration>,
    ) -> Result<(), Error> {
        let timeout =
            timeout.unwrap_or_else(|| Duration::from_secs(REACHABILITY_CHECK_TIMEOUT_SEC));
        debug!("Waiting for node to complete reachability check with a timeout of {timeout:?}...");

        let max_attempts = std::cmp::max(1, timeout.as_secs() / self.retry_delay.as_secs());
        trace!(
            "Metrics: reachability check max attempts set to: {max_attempts} with retry_delay of {:?}",
            self.retry_delay
        );

        let mut attempts = 0;
        loop {
            debug!("Attempting to check if reachability check is completed");

            let metrics = self
                .get_node_metrics()
                .await
                .inspect_err(|err| error!("Error getting node metrics: {err}"))?;

            if metrics.reachability_status.not_performed {
                debug!("Reachability check has not been enabled/performed. Considering reachability check completed.");
                return Ok(());
            }

            if !metrics.reachability_status.ongoing {
                debug!(
                    "Reachability check is not ongoing. Considering reachability check completed."
                );
                return Ok(());
            }

            attempts += 1;
            debug!("Reachability check is ongoing. Waiting for it to complete... {} / {max_attempts} attempts", attempts);

            tokio::time::sleep(self.retry_delay).await;
            if attempts >= max_attempts {
                error!("Reachability check has not completed after {max_attempts} attempts. Timing out.");
                return Err(Error::ReachabilityStatusCheckTimedOut {
                    metrics_port: self.port,
                    timeout,
                });
            }
        }
    }
}

impl From<&Vec<Sample>> for ReachabilityStatusValues {
    fn from(samples: &Vec<Sample>) -> Self {
        let mut not_performed = false;
        let mut ongoing = false;
        let mut reachable = false;
        let mut relay = false;
        let mut not_routable = false;
        let mut upnp = false;

        for sample in samples {
            if sample.metric == REACHABILITY_STATUS_METRIC {
                if sample.labels.get("status") == Some("NotPerformed") {
                    if let Value::Gauge(value) = sample.value {
                        not_performed = value == 1.0;
                    } else {
                        error!(
                            "Expected Gauge value for 'NotPerformed' status, found: {:?}",
                            sample.value
                        );
                    }
                } else if sample.labels.get("status") == Some("Ongoing") {
                    if let Value::Gauge(value) = sample.value {
                        ongoing = value == 1.0;
                    } else {
                        error!(
                            "Expected Gauge value for 'Ongoing' status, found: {:?}",
                            sample.value
                        );
                    }
                } else if sample.labels.get("status") == Some("Reachable") {
                    if let Value::Gauge(value) = sample.value {
                        reachable = value == 1.0;
                    } else {
                        error!(
                            "Expected Gauge value for 'Reachable' status, found: {:?}",
                            sample.value
                        );
                    }
                } else if sample.labels.get("status") == Some("Relay") {
                    if let Value::Gauge(value) = sample.value {
                        relay = value == 1.0;
                    } else {
                        error!(
                            "Expected Gauge value for 'Relay' status, found: {:?}",
                            sample.value
                        );
                    }
                } else if sample.labels.get("status") == Some("NotRoutable") {
                    if let Value::Gauge(value) = sample.value {
                        not_routable = value == 1.0;
                    } else {
                        error!(
                            "Expected Gauge value for 'NotRoutable' status, found: {:?}",
                            sample.value
                        );
                    }
                } else if sample.labels.get("status") == Some("UpnPSupported") {
                    if let Value::Gauge(value) = sample.value {
                        upnp = value == 1.0;
                    } else {
                        error!(
                            "Expected Gauge value for 'UpnPSupported' status, found: {:?}",
                            sample.value
                        );
                    }
                }
            }
        }
        ReachabilityStatusValues {
            not_performed,
            ongoing,
            reachable,
            relay,
            not_routable,
            upnp,
        }
    }
}
