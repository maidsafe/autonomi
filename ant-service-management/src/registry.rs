// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::NodeServiceData;
use crate::error::{Error, Result};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tokio::sync::{RwLock, mpsc};

/// Used to manage the NodeRegistry data and allows us to share the data across multiple threads.
///
/// Can be cloned freely.
#[derive(Clone, Debug)]
#[allow(clippy::type_complexity)]
pub struct NodeRegistryManager {
    pub environment_variables: Arc<RwLock<Option<Vec<(String, String)>>>>,
    pub nodes: Arc<RwLock<Vec<Arc<RwLock<NodeServiceData>>>>>,
    pub save_path: PathBuf,
}

impl From<NodeRegistry> for NodeRegistryManager {
    fn from(registry: NodeRegistry) -> Self {
        NodeRegistryManager {
            environment_variables: Arc::new(RwLock::new(registry.environment_variables)),
            nodes: Arc::new(RwLock::new(
                registry
                    .nodes
                    .into_iter()
                    .map(|node| Arc::new(RwLock::new(node)))
                    .collect(),
            )),
            save_path: registry.save_path,
        }
    }
}

impl NodeRegistryManager {
    /// Creates a new `NodeRegistryManager` with the specified save path.
    ///
    /// This is primarily used for testing purposes.
    pub fn empty(save_path: PathBuf) -> Self {
        NodeRegistryManager {
            environment_variables: Arc::new(RwLock::new(None)),
            nodes: Arc::new(RwLock::new(Vec::new())),
            save_path,
        }
    }

    /// Loads the node registry from the specified path.
    /// If the file does not exist, it returns a default `NodeRegistryManager` with an empty state.
    #[allow(clippy::unused_async)]
    pub async fn load(path: &Path) -> Result<Self> {
        let registry = NodeRegistry::load(path)?;
        let manager = NodeRegistryManager::from(registry);

        Ok(manager)
    }

    /// Saves the current state of the node registry to the specified path.
    pub async fn save(&self) -> Result<()> {
        let registry = self.to_registry().await;
        registry.save()?;
        Ok(())
    }

    /// Converts the current state of the `NodeRegistryManager` to a `NodeRegistry`.
    async fn to_registry(&self) -> NodeRegistry {
        let nodes = self.get_node_service_data().await;
        NodeRegistry {
            environment_variables: self.environment_variables.read().await.clone(),
            nodes,
            save_path: self.save_path.clone(),
        }
    }

    /// Converts the current state of the `NodeRegistryManager` to a `StatusSummary`.
    pub async fn to_status_summary(&self) -> StatusSummary {
        let registry = self.to_registry().await;
        registry.to_status_summary()
    }

    /// Inserts a new NodeServiceData into the registry.
    pub async fn push_node(&self, node: NodeServiceData) {
        let mut nodes = self.nodes.write().await;
        nodes.push(Arc::new(RwLock::new(node)));
    }

    pub async fn get_node_service_data(&self) -> Vec<NodeServiceData> {
        let mut node_services = Vec::new();
        for node in self.nodes.read().await.iter() {
            let node = node.read().await;
            node_services.push(node.clone());
        }
        node_services
    }

    /// Starts watching the registry file for changes and automatically reloads when modified
    /// Returns a channel receiver that notifies when the registry has been reloaded
    ///
    /// The returned receiver can be ignored if you are not interested in the notifications.
    pub fn watch_registry_file(&self) -> Result<mpsc::UnboundedReceiver<()>> {
        let (tx, rx) = mpsc::unbounded_channel();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let manager = self.clone();
        let save_path = self.save_path.clone();

        // Create watcher that sends events through a channel
        let mut watcher = RecommendedWatcher::new(
            move |res: notify::Result<Event>| {
                if let Ok(event) = res {
                    trace!("File watcher event: {event:?}");
                    // Only handle modify and create events
                    match event.kind {
                        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) => {
                            // Check if the event is for our registry file
                            for path in &event.paths {
                                // Use canonicalized paths for comparison to handle symlinks/different representations
                                let path_canonical =
                                    path.canonicalize().unwrap_or_else(|_| path.clone());
                                let save_path_canonical = save_path
                                    .canonicalize()
                                    .unwrap_or_else(|_| save_path.clone());

                                if path_canonical == save_path_canonical {
                                    trace!("Registry file change detected for: {path:?}");
                                    if let Err(err) = event_tx.send(event.clone()) {
                                        error!(
                                            "Failed to send registry file change event to internal rx: {err}"
                                        );
                                    }
                                    break;
                                }
                            }
                        }
                        _ => {}
                    }
                } else {
                    debug!("File watcher error: {res:?}",);
                }
            },
            Config::default(),
        )?;

        // Watch the parent directory of the registry file
        if let Some(parent) = self.save_path.parent() {
            watcher.watch(parent, RecursiveMode::NonRecursive)?;
        }

        // Spawn task to handle events and perform reloads
        tokio::spawn(async move {
            let _watcher = watcher; // Keep watcher alive

            while let Some(_event) = event_rx.recv().await {
                match manager.reload().await {
                    Ok(()) => {
                        info!("Registry reloaded successfully from file change");
                        if let Err(er) = tx.send(()) {
                            error!("Failed to send registry reload notification: {er}");
                        }
                    }
                    Err(e) => {
                        error!("Failed to reload registry after file change: {e:?}");
                    }
                }
            }
        });

        Ok(rx)
    }

    async fn reload(&self) -> Result<()> {
        let registry = NodeRegistry::load(&self.save_path)?;
        let new_manager = NodeRegistryManager::from(registry);
        *self.environment_variables.write().await =
            new_manager.environment_variables.read().await.clone();
        *self.nodes.write().await = new_manager.nodes.read().await.clone();

        Ok(())
    }
}

/// The struct that is written to the fs.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct NodeRegistry {
    environment_variables: Option<Vec<(String, String)>>,
    nodes: Vec<NodeServiceData>,
    save_path: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatusSummary {
    pub nodes: Vec<NodeServiceData>,
}

impl NodeRegistry {
    fn save(&self) -> Result<()> {
        debug!(
            "Saving node registry to {}",
            self.save_path.to_string_lossy()
        );
        let path = Path::new(&self.save_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).inspect_err(|err| {
                error!("Error creating node registry parent {parent:?}: {err:?}")
            })?;
        }
        trace!("Node registry content before save: {self:?}");

        let json = serde_json::to_string(self)?;
        let mut file = std::fs::File::create(self.save_path.clone())
            .inspect_err(|err| error!("Error creating node registry file: {err:?}"))?;
        file.write_all(json.as_bytes())
            .inspect_err(|err| error!("Error writing to node registry: {err:?}"))?;

        Ok(())
    }

    fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            debug!("Loading default node registry as {path:?} does not exist");
            return Ok(NodeRegistry {
                environment_variables: None,
                nodes: vec![],
                save_path: path.to_path_buf(),
            });
        }
        debug!("Loading node registry from {}", path.to_string_lossy());

        let mut file = std::fs::File::open(path)
            .inspect_err(|err| error!("Error opening node registry: {err:?}"))?;

        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .inspect_err(|err| error!("Error reading node registry: {err:?}"))?;

        // It's possible for the file to be empty if the user runs a `status` command before any
        // services were added.
        if contents.is_empty() {
            info!("Node registry file is empty, returning default registry");
            return Ok(NodeRegistry {
                environment_variables: None,
                nodes: vec![],
                save_path: path.to_path_buf(),
            });
        }

        let registry = serde_json::from_str(&contents)
            .inspect_err(|err| error!("Error deserializing node registry: {err:?}"))?;

        trace!("Loaded node registry: {registry:?}");
        Ok(registry)
    }

    fn to_status_summary(&self) -> StatusSummary {
        StatusSummary {
            nodes: self.nodes.clone(),
        }
    }
}

pub fn get_local_node_registry_path() -> Result<PathBuf> {
    let path = dirs_next::data_dir()
        .ok_or_else(|| {
            error!("Failed to get data_dir");
            Error::UserDataDirectoryNotObtainable
        })?
        .join("autonomi")
        .join("local_node_registry.json");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .inspect_err(|err| error!("Error creating node registry parent {parent:?}: {err:?}"))?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReachabilityProgress;
    use ant_logging::LogBuilder;
    use tempfile::TempDir;
    use tokio::time::{Duration, sleep};

    #[tokio::test]
    async fn test_two_registry_managers_sync_via_file_watching() {
        let _guard = LogBuilder::init_single_threaded_tokio_test();

        // Create a temporary directory for the test
        let temp_dir = TempDir::new().unwrap();
        let registry_path = temp_dir.path().join("test_registry.json");

        // Create an initial empty registry file
        let initial_registry = NodeRegistry {
            environment_variables: None,
            nodes: vec![],
            save_path: registry_path.clone(),
        };
        initial_registry.save().unwrap();

        // Create first registry manager instance
        let manager1 = NodeRegistryManager::load(&registry_path).await.unwrap();

        // Create second registry manager instance (watching the same file)
        let manager2 = NodeRegistryManager::load(&registry_path).await.unwrap();

        // Start watching on manager2 - this should detect changes made by manager1
        let mut change_receiver = manager2.watch_registry_file().unwrap();

        // Give the watcher time to start
        sleep(Duration::from_millis(500)).await;

        // Verify both managers start empty
        assert_eq!(manager1.get_node_service_data().await.len(), 0);
        assert_eq!(manager2.get_node_service_data().await.len(), 0);

        // Add a node to manager1 and save
        manager1
            .push_node(NodeServiceData {
                alpha: false,
                antnode_path: PathBuf::from("/tmp/antnode"),
                auto_restart: false,
                connected_peers: 10,
                data_dir_path: PathBuf::from("/tmp/data"),
                evm_network: ant_evm::EvmNetwork::default(),
                initial_peers_config: ant_bootstrap::InitialPeersConfig::default(),
                listen_addr: None,
                log_dir_path: PathBuf::from("/tmp/logs"),
                log_format: None,
                max_archived_log_files: None,
                max_log_files: None,
                metrics_port: 6001,
                network_id: None,
                node_ip: None,
                node_port: Some(8080),
                no_upnp: false,
                number: 1,
                peer_id: None,
                pid: Some(12345),
                skip_reachability_check: false,
                reachability_progress: ReachabilityProgress::NotRun,
                last_critical_failure: None,
                rewards_address: ant_evm::RewardsAddress::default(),
                rpc_socket_addr: None,
                schema_version: 3,
                service_name: "test-node-1".to_string(),
                status: crate::ServiceStatus::Running,
                user: Some("test-user".to_string()),
                user_mode: false,
                version: "1.0.0".to_string(),
                write_older_cache_files: false,
            })
            .await;

        // Save changes from manager1 to file - this should trigger manager2's watcher
        manager1.save().await.unwrap();

        // Wait for manager2 to receive the change notification
        tokio::time::timeout(Duration::from_secs(3), change_receiver.recv())
            .await
            .expect("Timeout waiting for file change notification from manager1's save")
            .expect("Channel closed unexpectedly");

        // Verify manager2 was automatically updated
        let manager2_nodes = manager2.get_node_service_data().await;
        assert_eq!(manager2_nodes.len(), 1);
        assert_eq!(manager2_nodes[0].service_name, "test-node-1");

        // Now add another node via manager1 and test again
        manager1
            .push_node(NodeServiceData {
                alpha: false,
                antnode_path: PathBuf::from("/tmp/antnode"),
                auto_restart: false,
                connected_peers: 11,
                data_dir_path: PathBuf::from("/tmp/data"),
                evm_network: ant_evm::EvmNetwork::default(),
                initial_peers_config: ant_bootstrap::InitialPeersConfig::default(),
                listen_addr: None,
                log_dir_path: PathBuf::from("/tmp/logs"),
                log_format: None,
                max_archived_log_files: None,
                max_log_files: None,
                metrics_port: 6002,
                network_id: None,
                node_ip: None,
                node_port: Some(8082),
                no_upnp: false,
                number: 2,
                peer_id: None,
                pid: Some(12346),
                skip_reachability_check: false,
                reachability_progress: ReachabilityProgress::NotRun,
                last_critical_failure: None,
                rewards_address: ant_evm::RewardsAddress::default(),
                rpc_socket_addr: None,
                schema_version: 3,
                service_name: "test-node-2".to_string(),
                status: crate::ServiceStatus::Running,
                user: Some("test-user".to_string()),
                user_mode: false,
                version: "1.0.0".to_string(),
                write_older_cache_files: false,
            })
            .await;

        // Verify manager1 now has both nodes before saving
        let manager1_nodes = manager1.get_node_service_data().await;
        println!(
            "Before second save, Manager1 has {} nodes",
            manager1_nodes.len()
        );
        for node in &manager1_nodes {
            println!("  - {}", node.service_name);
        }

        // Save again
        manager1.save().await.unwrap();

        // Add a small delay to ensure file system has time to process
        sleep(Duration::from_millis(100)).await;

        // Wait for the second change notification
        tokio::time::timeout(Duration::from_secs(3), change_receiver.recv())
            .await
            .expect("Timeout waiting for second file change notification")
            .expect("Channel closed unexpectedly");

        // Verify manager2 now has both nodes
        let manager2_nodes = manager2.get_node_service_data().await;
        println!(
            "After second save, Manager2 has {} nodes",
            manager2_nodes.len()
        );
        for node in &manager2_nodes {
            println!("  - {}", node.service_name);
        }

        assert_eq!(
            manager2_nodes.len(),
            2,
            "Manager2 should have 2 nodes after both additions"
        );
        assert!(
            manager2_nodes
                .iter()
                .any(|n| n.service_name == "test-node-1")
        );
        assert!(
            manager2_nodes
                .iter()
                .any(|n| n.service_name == "test-node-2")
        );
    }

    #[tokio::test]
    async fn test_registry_without_file_watcher_stays_out_of_sync() {
        let _guard = LogBuilder::init_single_threaded_tokio_test();

        // Create a temporary directory for the test
        let temp_dir = TempDir::new().unwrap();
        let registry_path = temp_dir.path().join("test_registry_no_sync.json");

        // Create an initial empty registry file
        let initial_registry = NodeRegistry {
            environment_variables: None,
            nodes: Default::default(),
            save_path: registry_path.clone(),
        };
        initial_registry.save().unwrap();

        // Create two registry manager instances
        let manager1 = NodeRegistryManager::load(&registry_path).await.unwrap();
        let manager2 = NodeRegistryManager::load(&registry_path).await.unwrap();

        // NOTE: We intentionally do NOT start file watching on manager2

        // Verify both managers start empty
        assert_eq!(manager1.get_node_service_data().await.len(), 0);
        assert_eq!(manager2.get_node_service_data().await.len(), 0);

        // Add a node to manager1 and save
        manager1
            .push_node(NodeServiceData {
                alpha: false,
                antnode_path: PathBuf::from("/tmp/antnode"),
                auto_restart: false,
                connected_peers: 10,
                data_dir_path: PathBuf::from("/tmp/data"),
                evm_network: ant_evm::EvmNetwork::default(),
                initial_peers_config: ant_bootstrap::InitialPeersConfig::default(),
                listen_addr: None,
                log_dir_path: PathBuf::from("/tmp/logs"),
                log_format: None,
                max_archived_log_files: None,
                max_log_files: None,
                metrics_port: 6001,
                network_id: None,
                node_ip: None,
                node_port: Some(8080),
                no_upnp: false,
                number: 1,
                peer_id: None,
                pid: Some(12345),
                skip_reachability_check: false,
                reachability_progress: ReachabilityProgress::NotRun,
                last_critical_failure: None,
                rewards_address: ant_evm::RewardsAddress::default(),
                rpc_socket_addr: None,
                schema_version: 3,
                service_name: "test-node-1".to_string(),
                status: crate::ServiceStatus::Running,
                user: Some("test-user".to_string()),
                user_mode: false,
                version: "1.0.0".to_string(),
                write_older_cache_files: false,
            })
            .await;

        // Save changes from manager1 to file
        manager1.save().await.unwrap();

        // Wait a moment to ensure any potential file events have time to process
        sleep(Duration::from_millis(500)).await;

        // Verify manager1 has the node
        let manager1_nodes = manager1.get_node_service_data().await;
        assert_eq!(manager1_nodes.len(), 1);
        assert_eq!(manager1_nodes[0].service_name, "test-node-1");

        // Verify manager2 still has NO nodes (because no file watcher was started)
        let manager2_nodes = manager2.get_node_service_data().await;
        assert_eq!(
            manager2_nodes.len(),
            0,
            "Manager2 should remain empty without file watching"
        );

        // Verify that manager2 can manually reload to get the updates
        manager2.reload().await.unwrap();
        let manager2_nodes_after_reload = manager2.get_node_service_data().await;
        assert_eq!(manager2_nodes_after_reload.len(), 1);
        assert_eq!(manager2_nodes_after_reload[0].service_name, "test-node-1");
    }

    #[tokio::test]
    async fn test_registry_reload_functionality() {
        let _guard = LogBuilder::init_single_threaded_tokio_test();
        // Create a temporary directory for the test
        let temp_dir = TempDir::new().unwrap();
        let registry_path = temp_dir.path().join("test_registry_reload.json");

        // Create an initial empty registry
        let initial_registry = NodeRegistry {
            environment_variables: None,
            nodes: vec![],
            save_path: registry_path.clone(),
        };
        initial_registry.save().unwrap();

        // Load the registry manager
        let manager = NodeRegistryManager::load(&registry_path).await.unwrap();

        // Start watching (even if notifications might not work in tests, the setup should work)
        let _change_receiver = manager.watch_registry_file().unwrap();

        // Test that reload works by manually modifying the file and calling reload
        for i in 1..=3 {
            let mut nodes = vec![];
            for j in 1..=i {
                nodes.push(NodeServiceData {
                    alpha: false,
                    antnode_path: PathBuf::from("/tmp/antnode"),
                    auto_restart: false,
                    connected_peers: 11,
                    data_dir_path: PathBuf::from("/tmp/data"),
                    evm_network: ant_evm::EvmNetwork::default(),
                    initial_peers_config: ant_bootstrap::InitialPeersConfig::default(),
                    listen_addr: None,
                    log_dir_path: PathBuf::from("/tmp/logs"),
                    log_format: None,
                    max_archived_log_files: None,
                    max_log_files: None,
                    metrics_port: 6002,
                    network_id: None,
                    node_ip: None,
                    node_port: Some(8080 + j as u16),
                    no_upnp: false,
                    number: j as u16,
                    peer_id: None,
                    pid: Some(12345 + j as u32),
                    skip_reachability_check: false,
                    reachability_progress: ReachabilityProgress::NotRun,
                    last_critical_failure: None,
                    rewards_address: ant_evm::RewardsAddress::default(),
                    rpc_socket_addr: None,
                    schema_version: 3,
                    service_name: format!("test-node-{j}"),
                    status: crate::ServiceStatus::Running,
                    user: Some("test-user".to_string()),
                    user_mode: false,
                    version: "1.0.0".to_string(),
                    write_older_cache_files: false,
                });
            }

            let registry = NodeRegistry {
                environment_variables: None,
                nodes,
                save_path: registry_path.clone(),
            };
            registry.save().unwrap();

            // Test manual reload functionality
            manager.reload().await.unwrap();

            // Verify the manager was updated
            let current_nodes = manager.get_node_service_data().await;
            assert_eq!(current_nodes.len(), i);
        }
    }
}
