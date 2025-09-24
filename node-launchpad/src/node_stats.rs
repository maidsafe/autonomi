// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::components::status::NODE_STAT_UPDATE_INTERVAL;
use crate::{
    action::{Action, NodeManagementCommand, NodeTableActions},
    components::node_table::lifecycle::RegistryNode,
};
use ant_service_management::metric::ReachabilityStatusValues;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, time::Instant};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndividualNodeStats {
    pub service_name: String,
    pub rewards_wallet_balance: usize,
    pub memory_usage_mb: usize,
    pub bandwidth_inbound: usize,
    pub bandwidth_outbound: usize,
    pub bandwidth_inbound_rate: usize,
    pub bandwidth_outbound_rate: usize,
    pub max_records: usize,
    pub peers: usize,
    pub reachability_status: ReachabilityStatusValues,
    pub connections: usize,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AggregatedNodeStats {
    pub total_rewards_wallet_balance: usize,
    pub total_memory_usage_mb: usize,
    pub individual_stats: Vec<IndividualNodeStats>,
    /// Nodes whose metrics endpoint could not be reached during the last poll.
    pub failed_to_connect: Vec<String>,
}

pub trait MetricsFetcher: Send + Sync {
    fn fetch(&self, running_nodes: Vec<RegistryNode>, action_sender: UnboundedSender<Action>);
}

#[derive(Default)]
pub struct AsyncMetricsFetcher;

impl MetricsFetcher for AsyncMetricsFetcher {
    fn fetch(&self, running_nodes: Vec<RegistryNode>, action_sender: UnboundedSender<Action>) {
        if running_nodes.is_empty() {
            return;
        }

        tokio::spawn(async move {
            AggregatedNodeStats::collect(running_nodes, action_sender).await;
        });
    }
}

/// Result of fetching stats from an individual node.
enum IndividualNodeStatsResult {
    Success(IndividualNodeStats),
    FailedToConnectToNode { service_name: String },
    OtherFailure { service_name: String },
}

impl AggregatedNodeStats {
    fn merge(&mut self, other: &IndividualNodeStats) {
        self.total_rewards_wallet_balance += other.rewards_wallet_balance;
        self.total_memory_usage_mb += other.memory_usage_mb;
        self.individual_stats.push(other.clone()); // Store individual stats
    }

    async fn collect(running_nodes: Vec<RegistryNode>, action_sender: UnboundedSender<Action>) {
        let mut stream = futures::stream::iter(running_nodes)
            .map(|registry_node| async move {
                Self::fetch_stat_per_node(registry_node.service_name, registry_node.metrics_port)
                    .await
            })
            .buffer_unordered(5);

        let mut aggregated_node_stats = AggregatedNodeStats::default();

        let mut probably_failed = HashSet::new();
        let mut failed_to_connect = HashSet::new();
        while let Some(result) = stream.next().await {
            match result {
                IndividualNodeStatsResult::Success(individual_stats) => {
                    if individual_stats.reachability_status.indicates_unreachable() {
                        warn!(
                            "Node {} is unreachable according to its metrics",
                            individual_stats.service_name
                        );
                        probably_failed.insert(individual_stats.service_name.clone());
                    }
                    aggregated_node_stats.merge(&individual_stats);
                }
                IndividualNodeStatsResult::FailedToConnectToNode { service_name } => {
                    failed_to_connect.insert(service_name.clone());
                    probably_failed.insert(service_name.clone());
                }
                IndividualNodeStatsResult::OtherFailure { service_name } => {
                    error!("Other failure while fetching stats from {service_name:?}");
                }
            }
        }

        aggregated_node_stats.failed_to_connect = failed_to_connect.into_iter().collect();

        if let Err(err) =
            action_sender.send(Action::StoreAggregatedNodeStats(aggregated_node_stats))
        {
            error!("Failed to send aggregated node stats action: {err:?}");
        }

        if !probably_failed.is_empty() {
            warn!(
                "These nodes have probably failed: {probably_failed:?}, trying to refresh registry to update the service status."
            );
            if let Err(err) = action_sender.send(Action::NodeTableActions(
                NodeTableActions::NodeManagementCommand(NodeManagementCommand::RefreshRegistry),
            )) {
                error!("Failed to send refresh registry action: {err:?}");
            }
        }
    }

    async fn fetch_stat_per_node(
        service_name: String,
        metrics_port: u16,
    ) -> IndividualNodeStatsResult {
        let now = Instant::now();

        let Ok(response) = reqwest::get(&format!("http://localhost:{metrics_port}/metrics")).await
        else {
            return IndividualNodeStatsResult::FailedToConnectToNode { service_name };
        };

        let Ok(body) = response.text().await else {
            return IndividualNodeStatsResult::OtherFailure { service_name };
        };

        let lines: Vec<_> = body.lines().map(|s| Ok(s.to_owned())).collect();
        let Ok(all_metrics) = prometheus_parse::Scrape::parse(lines.into_iter()) else {
            return IndividualNodeStatsResult::OtherFailure { service_name };
        };

        let mut stats = IndividualNodeStats {
            service_name,
            reachability_status: ReachabilityStatusValues::from(&all_metrics.samples),
            ..Default::default()
        };

        for sample in all_metrics.samples.iter() {
            if sample.metric == "ant_node_current_reward_wallet_balance" {
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
        IndividualNodeStatsResult::Success(stats)
    }
}
