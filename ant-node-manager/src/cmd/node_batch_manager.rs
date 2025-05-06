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
use std::{sync::Arc, time::Duration};
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

        // Poll all services for their status
        for node in &self.services {
            let mut rpc_client = RpcClient::from_socket_addr(node.read().await.rpc_socket_addr);
            rpc_client.set_max_attempts(1);
            rpc_client.is_node_connected_to_network(timeout).await.ok();

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
                            node.write().await.connected_peers = Some(network_info.connected_peers);
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
