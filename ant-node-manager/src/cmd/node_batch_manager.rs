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
    NodeRegistry, NodeServiceData, ServiceStatus,
};
use std::{
    collections::VecDeque,
    net::SocketAddr,
    path::PathBuf,
    time::{Duration, Instant},
};

pub struct NodeBatchServiceManager {
    services: Vec<ServiceInfo>,
    service_control: Box<dyn ServiceControl + Send>,
    verbosity: VerbosityLevel,
}

struct ServiceInfo {
    service_name: String,
    node_data: NodeServiceData,
    rpc_address: SocketAddr,
    user_mode: bool,
    bin_path: PathBuf,
}

impl NodeBatchServiceManager {
    pub fn new(
        node_registry: &NodeRegistry,
        service_indices: &[usize],
        service_control: Box<dyn ServiceControl + Send>,
        verbosity: VerbosityLevel,
    ) -> Self {
        let mut services = Vec::new();

        for &index in service_indices {
            let node = &node_registry.nodes[index];
            services.push(ServiceInfo {
                service_name: node.service_name.clone(),
                node_data: node.clone(),
                rpc_address: node.rpc_socket_addr,
                user_mode: node.user_mode,
                bin_path: node.antnode_path.clone(),
            });
        }

        NodeBatchServiceManager {
            services,
            service_control,
            verbosity,
        }
    }

    pub async fn start_all(&self, fixed_interval: u64) -> Vec<(String, Result<(), String>)> {
        let mut results = Vec::new();

        for service in &self.services {
            if self.verbosity != VerbosityLevel::Minimal {
                println!("Starting {}...", service.service_name);
            }

            let result = match self
                .service_control
                .start(&service.service_name, service.user_mode)
            {
                Ok(_) => {
                    info!("Started service {}", service.service_name);
                    Ok(())
                }
                Err(err) => {
                    error!("Failed to start service {}: {}", service.service_name, err);
                    Err(err.to_string())
                }
            };

            results.push((service.service_name.clone(), result));

            tokio::time::sleep(Duration::from_millis(fixed_interval)).await;
        }

        results
    }

    pub async fn poll_services(&self, timeout: Duration) -> Vec<(String, NodeServiceData)> {
        let start_time = std::time::Instant::now();
        let mut collected_data = Vec::new();

        if self.verbosity != VerbosityLevel::Minimal {
            println!(
                "Checking if the service is connected to the network with a timeout of {} seconds.",
                timeout.as_secs()
            );
        }

        let mut re_check_queue = VecDeque::new();
        re_check_queue.extend(self.services.iter());

        // Poll all services for their status.
        // if the service is not connected + reachability_check a, re-check it until the reachability_check is successful.
        while !re_check_queue.is_empty() {
            if let Some(service) = re_check_queue.pop_back() {
                let mut rpc_client = RpcClient::from_socket_addr(service.rpc_address);
                rpc_client.set_max_attempts(1);

                let mut node_data = service.node_data.clone();
                let is_reachability_check_ongoing = Self::is_reachability_check_ongoing(&node_data)
                    .await
                    .unwrap_or(false);

                if is_reachability_check_ongoing {
                    re_check_queue.push_front(service);
                    std::thread::sleep(Duration::from_millis(1000));
                    continue;
                }

                info!("Reachability check is not ongoing for {}, checking if the node is connected to the network", service.service_name);

                rpc_client.is_node_connected_to_network(timeout).await.ok();

                match self.service_control.get_process_pid(&service.bin_path) {
                    Ok(pid) => {
                        node_data.pid = Some(pid);
                        node_data.status = ServiceStatus::Running;

                        if let Ok(info) = rpc_client.node_info().await {
                            node_data.peer_id = Some(info.peer_id);

                            if let Ok(network_info) = rpc_client.network_info().await {
                                node_data.connected_peers = Some(network_info.connected_peers);
                                node_data.listen_addr = Some(network_info.listeners);
                            }
                        }
                    }
                    Err(_) => {
                        node_data.status = ServiceStatus::Stopped;
                        node_data.pid = None;
                    }
                }

                trace!("Completed polling for {}", service.service_name);

                collected_data.push((service.service_name.clone(), node_data));
            }
        }

        if self.verbosity != VerbosityLevel::Minimal {
            println!(
                "Completed polling after {} seconds",
                start_time.elapsed().as_secs()
            );
        }

        collected_data
    }

    async fn is_reachability_check_ongoing(node_data: &NodeServiceData) -> Option<bool> {
        let port = node_data.metrics_port.or_else(|| {
            error!("Metrics port not set for node {}", node_data.service_name);
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
                    info!(
                        "Reachability check is not enabled for node {}",
                        node_data.service_name
                    );
                    return Some(false);
                } else if sample.labels.get("ongoing").is_some_and(|v| v == "1") {
                    info!(
                        "Reachability check is ongoing for node {}",
                        node_data.service_name
                    );
                    return Some(true);
                } else {
                    info!(
                        "Reachability check is not ongoing for node {}",
                        node_data.service_name
                    );
                    return Some(false);
                }
            }
        }

        Some(false)
    }
}
