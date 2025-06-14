// Copyright (C) 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::VerbosityLevel;
use ant_service_management::{
    control::ServiceControl,
    rpc::{RpcActions, RpcClient},
    NodeServiceData, ServiceStatus,
};
use std::{collections::VecDeque, sync::Arc, time::Duration};
use tokio::sync::RwLock;

pub struct NodeBatchServiceManager {
    services: Vec<Arc<RwLock<NodeServiceData>>>,
    service_control: Box<dyn ServiceControl>,
    verbosity: VerbosityLevel,
}

impl NodeBatchServiceManager {
    pub fn new(
        services: Vec<Arc<RwLock<NodeServiceData>>>,
        service_control: Box<dyn ServiceControl>,
        verbosity: VerbosityLevel,
    ) -> Self {
        NodeBatchServiceManager {
            services,
            service_control,
            verbosity,
        }
    }

    pub async fn start_all(&self, fixed_interval: u64) -> Vec<(String, Result<(), String>)> {
        let mut results = Vec::new();

        for node in &self.services {
            let node = node.read().await;

            if self.verbosity != VerbosityLevel::Minimal {
                println!("Starting {}...", node.service_name);
            }

            let result = match self
                .service_control
                .start(&node.service_name, node.user_mode)
            {
                Ok(_) => {
                    info!("Started service {}", node.service_name);
                    Ok(())
                }
                Err(err) => {
                    error!("Failed to start service {}: {}", node.service_name, err);
                    Err(err.to_string())
                }
            };

            results.push((node.service_name.clone(), result));

            tokio::time::sleep(Duration::from_millis(fixed_interval)).await;
        }

        results
    }

    pub async fn poll_services(&self, timeout: Duration) {
        let start_time = std::time::Instant::now();

        if self.verbosity != VerbosityLevel::Minimal {
            println!(
                "Checking if the service is connected to the network with a timeout of {} seconds.",
                timeout.as_secs()
            );
        }

        let mut re_check_queue = VecDeque::new();
        re_check_queue.extend(self.services.iter().cloned());

        // Poll all services for their status.
        // if the service is not connected + reachability_check a, re-check it until the reachability_check is successful.
        while !re_check_queue.is_empty() {
            if let Some(node) = re_check_queue.pop_back() {
                let mut rpc_client = RpcClient::from_socket_addr(node.read().await.rpc_socket_addr);
                rpc_client.set_max_attempts(1);

                let is_reachability_check_ongoing =
                    Self::is_reachability_check_ongoing(node.clone())
                        .await
                        .unwrap_or(false);

                if is_reachability_check_ongoing {
                    re_check_queue.push_front(node);
                    std::thread::sleep(Duration::from_millis(1000));
                    continue;
                }

                info!("Reachability check is not ongoing for {}, checking if the node is connected to the network", node.read().await.service_name);

                rpc_client
                    .wait_until_node_connects_to_network(Some(timeout))
                    .await
                    .ok();

                match self
                    .service_control
                    .get_process_pid(&node.read().await.antnode_path)
                {
                    Ok(pid) => {
                        node.write().await.pid = Some(pid);
                        node.write().await.status = ServiceStatus::Running;

                        if let Ok(info) = rpc_client.node_info().await {
                            node.write().await.peer_id = Some(info.peer_id);

                            if let Ok(network_info) = rpc_client.network_info().await {
                                node.write().await.connected_peers =
                                    Some(network_info.connected_peers);
                                node.write().await.listen_addr = Some(network_info.listeners);
                            }
                        }
                    }
                    Err(_) => {
                        node.write().await.status = ServiceStatus::Stopped;
                        node.write().await.pid = None;
                    }
                }

                trace!("Completed polling for {}", node.read().await.service_name);
            }

            if self.verbosity != VerbosityLevel::Minimal {
                println!(
                    "Completed polling after {} seconds",
                    start_time.elapsed().as_secs()
                );
            }
        }
    }

    async fn is_reachability_check_ongoing(
        node_data: Arc<RwLock<NodeServiceData>>,
    ) -> Option<bool> {
        let service_name = node_data.read().await.service_name.clone();

        let port = node_data.read().await.metrics_port.or_else(|| {
            error!("Metrics port not set for node {service_name}");
            None
        })?;

        let body = reqwest::get(&format!("http://localhost:{port}/metrics",))
            .await
            .inspect_err(|err| {
                error!("Failed to fetch metrics from port {port}: {err}");
            })
            .ok()?
            .text()
            .await
            .inspect_err(|err| {
                error!("Failed to read response body from port {port}: {err}");
            })
            .ok()?;
        let lines: Vec<_> = body.lines().map(|s| Ok(s.to_owned())).collect();
        let all_metrics = prometheus_parse::Scrape::parse(lines.into_iter())
            .inspect_err(|err| {
                error!("Failed to parse metrics from port {port}: {err}");
            })
            .ok()?;

        for sample in all_metrics.samples.iter() {
            // status metric: Sample { metric: "ant_networking_reachability_status_info", value: Untyped(1.0),
            // labels: Labels({"not_routable": "0", "relay": "0", "upnp_supported": "0", "not_performed": "0", "ongoing": "1", "reachable": "0"})
            if sample.metric == "ant_networking_reachability_status_info" {
                info!("metric: {:?}", sample);
                if sample.labels.get("not_performed").is_some_and(|v| v == "1") {
                    info!("Reachability check is not enabled for node {service_name}");
                    return Some(false);
                } else if sample.labels.get("ongoing").is_some_and(|v| v == "1") {
                    info!("Reachability check is ongoing for node {service_name}");
                    return Some(true);
                } else {
                    info!("Reachability check is not ongoing for node {service_name}");
                    return Some(false);
                }
            }
        }

        Some(false)
    }
}
