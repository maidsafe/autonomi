// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

pub mod auditor;
pub mod control;
pub mod daemon;
pub mod error;
pub mod faucet;
pub mod metric;
pub mod node;
pub mod rpc;

#[macro_use]
extern crate tracing;

pub mod antctl_proto {
    tonic::include_proto!("antctl_proto");
}

use async_trait::async_trait;
use auditor::AuditorServiceData;
use semver::Version;
use serde::{Deserialize, Serialize};
use service_manager::ServiceInstallCtx;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub use daemon::{DaemonService, DaemonServiceData};
pub use error::{Error, Result};
pub use faucet::{FaucetService, FaucetServiceData};
pub use node::{NodeService, NodeServiceData};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ServiceStatus {
    /// The service has been added but not started for the first time
    Added,
    /// Last time we checked the service was running
    Running,
    /// The service has been stopped
    Stopped,
    /// The service has been removed
    Removed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NatDetectionStatus {
    Public,
    UPnP,
    Private,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UpgradeResult {
    Forced(String, String),
    NotRequired,
    Upgraded(String, String),
    UpgradedButNotStarted(String, String, String),
    Error(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeOptions {
    pub auto_restart: bool,
    pub env_variables: Option<Vec<(String, String)>>,
    pub force: bool,
    pub start_service: bool,
    pub target_bin_path: PathBuf,
    pub target_version: Version,
}

#[async_trait]
pub trait ServiceStateActions {
    fn bin_path(&self) -> PathBuf;
    fn build_upgrade_install_context(&self, options: UpgradeOptions) -> Result<ServiceInstallCtx>;
    fn data_dir_path(&self) -> PathBuf;
    fn is_user_mode(&self) -> bool;
    fn log_dir_path(&self) -> PathBuf;
    fn name(&self) -> String;
    fn pid(&self) -> Option<u32>;
    fn on_remove(&mut self);
    async fn on_start(&mut self, pid: Option<u32>, full_refresh: bool) -> Result<()>;
    async fn on_stop(&mut self) -> Result<()>;
    fn set_version(&mut self, version: &str);
    fn status(&self) -> ServiceStatus;
    fn version(&self) -> String;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatusSummary {
    pub nodes: Vec<NodeServiceData>,
    pub daemon: Option<DaemonServiceData>,
    pub faucet: Option<FaucetServiceData>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeRegistry {
    pub auditor: Option<AuditorServiceData>,
    pub daemon: Option<DaemonServiceData>,
    pub environment_variables: Option<Vec<(String, String)>>,
    pub faucet: Option<FaucetServiceData>,
    pub nat_status: Option<NatDetectionStatus>,
    pub nodes: Vec<NodeServiceData>,
    pub save_path: PathBuf,
}

impl NodeRegistry {
    pub fn save(&self) -> Result<()> {
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

    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            debug!("Loading default node registry as {path:?} does not exist");
            return Ok(NodeRegistry {
                auditor: None,
                daemon: None,
                environment_variables: None,
                faucet: None,
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
                auditor: None,
                daemon: None,
                environment_variables: None,
                faucet: None,
                nat_status: None,
                nodes: vec![],
                save_path: path.to_path_buf(),
            });
        }

        Self::from_json(&contents)
    }

    pub fn from_json(json: &str) -> Result<Self> {
        let registry = serde_json::from_str(json)
            .inspect_err(|err| error!("Error deserializing node registry: {err:?}"))?;
        Ok(registry)
    }

    pub fn to_status_summary(&self) -> StatusSummary {
        StatusSummary {
            nodes: self.nodes.clone(),
            daemon: self.daemon.clone(),
            faucet: self.faucet.clone(),
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
