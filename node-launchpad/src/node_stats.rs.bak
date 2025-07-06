// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_service_management::{NodeServiceData, ServiceStatus};
use color_eyre::Result;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Instant};
use tokio::sync::mpsc::UnboundedSender;

use super::components::status::NODE_STAT_UPDATE_INTERVAL;

use crate::action::{Action, StatusActions};

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndividualNodeStats {
    pub service_name: String,
    pub forwarded_rewards: usize,
    pub rewards_wallet_balance: usize,
    pub memory_usage_mb: usize,
    pub bandwidth_inbound: usize,
    pub bandwidth_outbound: usize,
    pub bandwidth_inbound_rate: usize,
    pub bandwidth_outbound_rate: usize,
    pub max_records: usize,
    pub peers: usize,
    pub connections: usize,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeStats {
    pub total_forwarded_rewards: usize,
    pub total_rewards_wallet_balance: usize,
    pub total_memory_usage_mb: usize,
    pub individual_stats: Vec<IndividualNodeStats>,
}

impl NodeStats {
    fn merge(&mut self, other: &IndividualNodeStats) {
        self.total_forwarded_rewards += other.forwarded_rewards;
        self.total_rewards_wallet_balance += other.rewards_wallet_balance;
        self.total_memory_usage_mb += other.memory_usage_mb;
        self.individual_stats.push(other.clone()); // Store individual stats
    }

    /// Fetches statistics from all running nodes and sends the aggregated stats via the action sender.
    ///
    /// This method iterates over the provided list of `NodeServiceData` instances, filters out nodes that are not running,
    /// and for each running node, it checks if a metrics port is available. If a metrics port is found, the node's details
    /// (service name, metrics port, and data directory path) are collected. If no metrics port is found, a debug message
    /// is logged indicating that the node's stats will not be fetched.
    ///
    /// If there are any nodes with available metrics ports, this method spawns a local task to asynchronously fetch
    /// statistics from these nodes using `fetch_all_node_stats_inner`. The aggregated statistics are then sent via the
    /// provided `action_sender`.
    ///
    /// If no running nodes with metrics ports are found, a debug message is logged indicating that there are no running nodes
    /// to fetch stats from.
    ///
    /// # Parameters
    ///
    /// * `nodes`: A slice of `NodeServiceData` instances representing the nodes to fetch statistics from.
    /// * `action_sender`: An unbounded sender of `Action` instances used to send the aggregated node statistics.
    pub fn fetch_all_node_stats(nodes: &[NodeServiceData], action_sender: UnboundedSender<Action>) {
        let node_details = nodes
            .iter()
            .filter_map(|node| {
                if node.status == ServiceStatus::Running {
                    if let Some(metrics_port) = node.metrics_port {
                        Some((
                            node.service_name.clone(),
                            metrics_port,
                            node.data_dir_path.clone(),
                        ))
                    } else {
                        error!(
                            "No metrics port found for {:?}. Skipping stat fetch.",
                            node.service_name
                        );
                        None
                    }
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        if !node_details.is_empty() {
            debug!("Fetching stats from {} nodes", node_details.len());
            tokio::spawn(async move {
                Self::fetch_all_node_stats_inner(node_details, action_sender).await;
            });
        } else {
            debug!("No running nodes to fetch stats from.");
        }
    }

    /// This method is an inner function used to fetch statistics from all nodes.
    /// It takes a vector of node details (service name, metrics port, and data directory path) and an unbounded sender of `Action` instances.
    /// The method iterates over the provided list of `NodeServiceData` instances, filters out nodes that are not running,
    /// and for each running node, it checks if a metrics port is available. If a metrics port is found, the node's details
    /// (service name, metrics port, and data directory path) are collected. If no metrics port is found, a debug message
    /// is logged indicating that the node's stats will not be fetched.
    ///
    /// If there are any nodes with available metrics ports, this method spawns a local task to asynchronously fetch
    /// statistics from these nodes using `fetch_all_node_stats_inner`. The aggregated statistics are then sent via the
    /// provided `action_sender`.
    ///
    /// If no running nodes with metrics ports are found, a debug message is logged indicating that there are no running nodes
    /// to fetch stats from.
    ///
    /// # Parameters
    ///
    /// * `node_details`: A vector of tuples, each containing the service name, metrics port, and data directory path of a node.
    /// * `action_sender`: An unbounded sender of `Action` instances used to send the aggregated node statistics.
    async fn fetch_all_node_stats_inner(
        node_details: Vec<(String, u16, PathBuf)>,
        action_sender: UnboundedSender<Action>,
    ) {
        let mut stream = futures::stream::iter(node_details)
            .map(|(service_name, metrics_port, data_dir)| async move {
                (
                    Self::fetch_stat_per_node(metrics_port, data_dir).await,
                    service_name,
                )
            })
            .buffer_unordered(5);

        let mut all_node_stats = NodeStats::default();

        while let Some((result, service_name)) = stream.next().await {
            match result {
                Ok(stats) => {
                    let individual_stats = IndividualNodeStats {
                        service_name: service_name.clone(),
                        forwarded_rewards: stats.forwarded_rewards,
                        rewards_wallet_balance: stats.rewards_wallet_balance,
                        memory_usage_mb: stats.memory_usage_mb,
                        bandwidth_inbound: stats.bandwidth_inbound,
                        bandwidth_outbound: stats.bandwidth_outbound,
                        max_records: stats.max_records,
                        peers: stats.peers,
                        connections: stats.connections,
                        bandwidth_inbound_rate: stats.bandwidth_inbound_rate,
                        bandwidth_outbound_rate: stats.bandwidth_outbound_rate,
                    };
                    all_node_stats.merge(&individual_stats);
                }
                Err(err) => {
                    error!("Error while fetching stats from {service_name:?}: {err:?}");
                }
            }
        }

        if let Err(err) = action_sender.send(Action::StatusActions(
            StatusActions::NodesStatsObtained(all_node_stats),
        )) {
            error!("Error while sending action: {err:?}");
        }
    }

    async fn fetch_stat_per_node(
        metrics_port: u16,
        _data_dir: PathBuf,
    ) -> Result<IndividualNodeStats> {
        let now = Instant::now();

        let body = reqwest::get(&format!("http://localhost:{metrics_port}/metrics"))
            .await?
            .text()
            .await?;
        let lines: Vec<_> = body.lines().map(|s| Ok(s.to_owned())).collect();
        let all_metrics = prometheus_parse::Scrape::parse(lines.into_iter())?;

        let mut stats = IndividualNodeStats::default();

        for sample in all_metrics.samples.iter() {
            if sample.metric == "ant_node_total_forwarded_rewards" {
                // Nanos
                match sample.value {
                    prometheus_parse::Value::Counter(val)
                    | prometheus_parse::Value::Gauge(val)
                    | prometheus_parse::Value::Untyped(val) => {
                        stats.forwarded_rewards = val as usize;
                    }
                    _ => {}
                }
            } else if sample.metric == "ant_node_current_reward_wallet_balance" {
                // Attos
                match sample.value {
                    prometheus_parse::Value::Counter(val)
                    | prometheus_parse::Value::Gauge(val)
                    | prometheus_parse::Value::Untyped(val) => {
                        stats.rewards_wallet_balance = val as usize;
                    }
                    _ => {}
                }
            } else if sample.metric == "ant_networking_process_memory_used_mb" {
                // Memory
                match sample.value {
                    prometheus_parse::Value::Counter(val)
                    | prometheus_parse::Value::Gauge(val)
                    | prometheus_parse::Value::Untyped(val) => {
                        stats.memory_usage_mb = val as usize;
                    }
                    _ => {}
                }
            } else if sample.metric == "libp2p_bandwidth_bytes_total" {
                // Mbps
                match sample.value {
                    prometheus_parse::Value::Counter(val)
                    | prometheus_parse::Value::Gauge(val)
                    | prometheus_parse::Value::Untyped(val) => {
                        if let Some(direction) = sample.labels.get("direction") {
                            if direction == "Inbound" {
                                let current_inbound = val as usize;
                                let rate = (current_inbound as f64
                                    - stats.bandwidth_inbound as f64)
                                    / NODE_STAT_UPDATE_INTERVAL.as_secs_f64();
                                stats.bandwidth_inbound_rate = rate as usize;
                                stats.bandwidth_inbound = current_inbound;
                            } else if direction == "Outbound" {
                                let current_outbound = val as usize;
                                let rate = (current_outbound as f64
                                    - stats.bandwidth_outbound as f64)
                                    / NODE_STAT_UPDATE_INTERVAL.as_secs_f64();
                                stats.bandwidth_outbound_rate = rate as usize;
                                stats.bandwidth_outbound = current_outbound;
                            }
                        }
                    }
                    _ => {}
                }
            } else if sample.metric == "ant_networking_records_stored" {
                // Records
                match sample.value {
                    prometheus_parse::Value::Counter(val)
                    | prometheus_parse::Value::Gauge(val)
                    | prometheus_parse::Value::Untyped(val) => {
                        stats.max_records = val as usize;
                    }
                    _ => {}
                }
            } else if sample.metric == "ant_networking_peers_in_routing_table" {
                // Peers
                match sample.value {
                    prometheus_parse::Value::Counter(val)
                    | prometheus_parse::Value::Gauge(val)
                    | prometheus_parse::Value::Untyped(val) => {
                        stats.peers = val as usize;
                    }
                    _ => {}
                }
            } else if sample.metric == "ant_networking_open_connections" {
                // Connections
                match sample.value {
                    prometheus_parse::Value::Counter(val)
                    | prometheus_parse::Value::Gauge(val)
                    | prometheus_parse::Value::Untyped(val) => {
                        stats.connections = val as usize;
                    }
                    _ => {}
                }
            }
        }
        trace!(
            "Fetched stats from metrics_port {metrics_port:?} in {:?}",
            now.elapsed()
        );
        Ok(stats)
    }
}
