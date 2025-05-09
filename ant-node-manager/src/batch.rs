// Copyright (C) 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::{net::SocketAddr, path::PathBuf, time::Duration};

use ant_service_management::{
    control::ServiceControl,
    rpc::{RpcActions, RpcClient},
    NodeRegistry, NodeServiceData, ServiceStatus,
};

use crate::VerbosityLevel;

pub struct BatchServiceManager {
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

impl BatchServiceManager {
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

        BatchServiceManager {
            services,
            service_control,
            verbosity,
        }
    }

    pub async fn start_all(&self, fixed_interval: u64) -> Vec<(String, Result<(), String>)> {
        let mut results = Vec::new();

        // First pass: just start all services with optional delay between starts
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

        // Poll all services for their status
        for service in &self.services {
            let mut rpc_client = RpcClient::from_socket_addr(service.rpc_address);
            rpc_client.set_max_attempts(1);

            let mut node_data = service.node_data.clone();

            rpc_client.is_node_connected_to_network(timeout).await.ok();

            // Check if the process is running
            match self.service_control.get_process_pid(&service.bin_path) {
                Ok(pid) => {
                    node_data.pid = Some(pid);
                    node_data.status = ServiceStatus::Running;

                    // Try to get additional info via RPC
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

            collected_data.push((service.service_name.clone(), node_data));
        }

        if self.verbosity != VerbosityLevel::Minimal {
            println!(
                "Completed polling after {} seconds",
                start_time.elapsed().as_secs()
            );
        }

        collected_data
    }
}
