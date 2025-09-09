// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_bootstrap::InitialPeersConfig;
use ant_evm::{EvmNetwork, RewardsAddress};
use ant_service_management::{NodeServiceData, ReachabilityProgress, ServiceStatus};
use color_eyre::eyre::Result;
use serde::{Deserialize, Serialize};
use serde_json;
use std::{
    env,
    fs::File,
    io::Write,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
};
use tempfile::TempDir;

static MOCK_COUNTER: AtomicU64 = AtomicU64::new(0);

fn get_antnode_filename() -> String {
    format!("antnode{}", env::consts::EXE_SUFFIX)
}

/// Mock node registry for testing that manages temporary registry files
pub struct MockNodeRegistry {
    temp_dir: TempDir,
    registry_path: PathBuf,
    nodes: Vec<NodeServiceData>,
}

/// The registry JSON structure that matches the actual format
#[derive(Clone, Debug, Serialize, Deserialize)]
struct RegistryJson {
    environment_variables: Option<Vec<(String, String)>>,
    nodes: Vec<NodeServiceData>,
    save_path: PathBuf,
}

impl MockNodeRegistry {
    /// Create an empty registry
    pub fn empty() -> Result<Self> {
        let temp_dir = tempfile::tempdir()?;
        let registry_path = temp_dir.path().join("node_registry.json");

        let registry = Self {
            temp_dir,
            registry_path,
            nodes: Vec::new(),
        };

        registry.save_to_file()?;
        Ok(registry)
    }

    /// Create registry with a specific number of nodes
    pub fn with_nodes(count: u64) -> Result<Self> {
        let mut registry = Self::empty()?;

        for i in 0..count {
            let node = registry.create_test_node_service_data(i, ServiceStatus::Running);
            registry.nodes.push(node);
        }

        registry.save_to_file()?;
        Ok(registry)
    }

    /// Add a node to the registry
    pub fn add_node(&mut self, node: NodeServiceData) -> Result<()> {
        self.nodes.push(node);
        self.save_to_file()
    }

    /// Remove a node by service name
    pub fn remove_node(&mut self, service_name: &str) -> Result<()> {
        self.nodes.retain(|node| node.service_name != service_name);
        self.save_to_file()
    }

    /// Update a node's status
    pub fn update_node_status(&mut self, service_name: &str, status: ServiceStatus) -> Result<()> {
        if let Some(node) = self
            .nodes
            .iter_mut()
            .find(|n| n.service_name == service_name)
        {
            node.status = status;
        }
        self.save_to_file()
    }

    /// Reset all nodes (remove everything)
    pub fn reset_all(&mut self) -> Result<()> {
        self.nodes.clear();
        self.save_to_file()
    }

    /// Get the registry file path
    pub fn path(&self) -> &Path {
        &self.registry_path
    }

    /// Get the number of nodes
    pub fn node_count(&self) -> u64 {
        self.nodes.len() as u64
    }

    /// Check if registry contains a specific node
    pub fn contains_node(&self, service_name: &str) -> bool {
        self.nodes
            .iter()
            .any(|node| node.service_name == service_name)
    }

    /// Verify a node has specific status
    pub fn verify_node_status(&self, service_name: &str, status: ServiceStatus) -> bool {
        self.nodes
            .iter()
            .any(|node| node.service_name == service_name && node.status == status)
    }

    /// Save the registry to file
    fn save_to_file(&self) -> Result<()> {
        let registry_json = RegistryJson {
            environment_variables: None,
            nodes: self.nodes.clone(),
            save_path: self.registry_path.clone(),
        };

        let json_content = serde_json::to_string_pretty(&registry_json)?;
        let mut file = File::create(&self.registry_path)?;
        file.write_all(json_content.as_bytes())?;

        Ok(())
    }

    /// Get the registry path
    pub fn get_registry_path(&self) -> &PathBuf {
        &self.registry_path
    }

    /// Create a test NodeServiceData with deterministic values using direct struct construction
    pub fn create_test_node_service_data(
        &self,
        index: u64,
        status: ServiceStatus,
    ) -> NodeServiceData {
        let service_name = format!("antnode-{}", index + 1);
        let unique_id = MOCK_COUNTER.fetch_add(1, Ordering::SeqCst);
        let temp_dir = self.temp_dir.path().join(format!("antnode_{unique_id}",));

        NodeServiceData {
            schema_version: 3,
            service_name,
            version: "0.1.0".to_string(),
            status: status.clone(),
            antnode_path: temp_dir.join(get_antnode_filename()),
            data_dir_path: temp_dir.join("data"),
            log_dir_path: temp_dir.join("logs"),
            number: (index + 1) as u16,
            metrics_port: (25000 + index) as u16,
            connected_peers: 5,
            alpha: false,
            auto_restart: false,
            evm_network: EvmNetwork::ArbitrumOne,
            initial_peers_config: InitialPeersConfig {
                first: false,
                local: false,
                addrs: vec![],
                network_contacts_url: vec![],
                ignore_cache: false,
                bootstrap_cache_dir: None,
            },
            listen_addr: None,
            log_format: None,
            max_archived_log_files: Some(10),
            max_log_files: Some(5),
            network_id: Some(1),
            node_ip: Some(Ipv4Addr::new(127, 0, 0, 1)),
            node_port: Some((15000 + index) as u16),
            no_upnp: false,
            peer_id: None,
            pid: if status == ServiceStatus::Running {
                Some((1000 + index) as u32)
            } else {
                None
            },
            rewards_address: RewardsAddress::from_str("0x1234567890123456789012345678901234567890")
                .unwrap_or_default(),
            reachability_progress: ReachabilityProgress::NotRun,
            rpc_socket_addr: Some(
                SocketAddr::from_str(&format!("127.0.0.1:{}", 35000 + index))
                    .expect("Invalid socket address"),
            ),
            skip_reachability_check: false,
            user: None,
            user_mode: false,
            write_older_cache_files: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_registry_empty() -> Result<()> {
        let registry = MockNodeRegistry::empty()?;
        assert_eq!(registry.node_count(), 0);
        assert!(registry.path().exists());
        Ok(())
    }

    #[tokio::test]
    async fn test_mock_registry_with_nodes() -> Result<()> {
        let registry = MockNodeRegistry::with_nodes(3)?;
        assert_eq!(registry.node_count(), 3);
        assert!(registry.contains_node("antnode-1"));
        assert!(registry.contains_node("antnode-2"));
        assert!(registry.contains_node("antnode-3"));
        Ok(())
    }

    #[tokio::test]
    async fn test_mock_registry_manipulation() -> Result<()> {
        let mut registry = MockNodeRegistry::empty()?;

        // Add a node
        let node = registry.create_test_node_service_data(0, ServiceStatus::Added);
        registry.add_node(node)?;
        assert_eq!(registry.node_count(), 1);
        assert!(registry.contains_node("antnode-1"));

        // Update status
        registry.update_node_status("antnode-1", ServiceStatus::Running)?;
        assert!(registry.verify_node_status("antnode-1", ServiceStatus::Running));

        // Remove node
        registry.remove_node("antnode-1")?;
        assert_eq!(registry.node_count(), 0);
        assert!(!registry.contains_node("antnode-1"));

        Ok(())
    }
}
