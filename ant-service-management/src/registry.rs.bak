// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::error::{Error, Result};
use crate::{DaemonServiceData, NatDetectionStatus, NodeServiceData};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tokio::sync::RwLock;

/// Used to manage the NodeRegistry data and allows us to share the data across multiple threads.
///
/// Can be cloned freely.
#[derive(Clone, Debug)]
#[allow(clippy::type_complexity)]
pub struct NodeRegistryManager {
    pub daemon: Arc<RwLock<Option<Arc<RwLock<DaemonServiceData>>>>>,
    pub environment_variables: Arc<RwLock<Option<Vec<(String, String)>>>>,
    pub nat_status: Arc<RwLock<Option<NatDetectionStatus>>>,
    pub nodes: Arc<RwLock<Vec<Arc<RwLock<NodeServiceData>>>>>,
    pub save_path: PathBuf,
}

impl From<NodeRegistry> for NodeRegistryManager {
    fn from(registry: NodeRegistry) -> Self {
        NodeRegistryManager {
            daemon: Arc::new(RwLock::new(
                registry.daemon.map(|daemon| Arc::new(RwLock::new(daemon))),
            )),
            environment_variables: Arc::new(RwLock::new(registry.environment_variables)),
            nat_status: Arc::new(RwLock::new(registry.nat_status)),
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
            daemon: Arc::new(RwLock::new(None)),
            environment_variables: Arc::new(RwLock::new(None)),
            nat_status: Arc::new(RwLock::new(None)),
            nodes: Arc::new(RwLock::new(Vec::new())),
            save_path,
        }
    }

    /// Loads the node registry from the specified path.
    /// If the file does not exist, it returns a default `NodeRegistryManager` with an empty state.
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
        let mut daemon = None;
        {
            if let Some(d) = self.daemon.read().await.as_ref() {
                daemon = Some(d.read().await.clone());
            }
        }
        let registry = NodeRegistry {
            daemon,
            environment_variables: self.environment_variables.read().await.clone(),
            nat_status: self.nat_status.read().await.clone(),
            nodes,
            save_path: self.save_path.clone(),
        };

        registry
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

    /// Inserts the DaemonServiceData into the registry.
    pub async fn insert_daemon(&self, daemon: DaemonServiceData) {
        let mut daemon_lock = self.daemon.write().await;
        *daemon_lock = Some(Arc::new(RwLock::new(daemon)));
    }

    pub async fn get_node_service_data(&self) -> Vec<NodeServiceData> {
        let mut node_services = Vec::new();
        for node in self.nodes.read().await.iter() {
            let node = node.read().await;
            node_services.push(node.clone());
        }
        node_services
    }
}

/// The struct that is written to the fs.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct NodeRegistry {
    daemon: Option<DaemonServiceData>,
    environment_variables: Option<Vec<(String, String)>>,
    nat_status: Option<NatDetectionStatus>,
    nodes: Vec<NodeServiceData>,
    save_path: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatusSummary {
    pub nodes: Vec<NodeServiceData>,
    pub daemon: Option<DaemonServiceData>,
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
                daemon: None,
                environment_variables: None,
                nat_status: None,
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
            return Ok(NodeRegistry {
                daemon: None,
                environment_variables: None,
                nat_status: None,
                nodes: vec![],
                save_path: path.to_path_buf(),
            });
        }

        Self::from_json(&contents)
    }

    fn from_json(json: &str) -> Result<Self> {
        let registry = serde_json::from_str(json)
            .inspect_err(|err| error!("Error deserializing node registry: {err:?}"))?;
        Ok(registry)
    }

    fn to_status_summary(&self) -> StatusSummary {
        StatusSummary {
            nodes: self.nodes.clone(),
            daemon: self.daemon.clone(),
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
