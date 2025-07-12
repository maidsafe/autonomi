// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

pub mod control;
pub mod daemon;
pub mod error;
pub mod metric;
pub mod node;
pub mod registry;
pub mod rpc;

#[macro_use]
extern crate tracing;

pub mod antctl_proto {
    tonic::include_proto!("antctl_proto");
}

use crate::control::ServiceControl;
use async_trait::async_trait;
use semver::Version;
use serde::{Deserialize, Serialize};
use service_manager::ServiceInstallCtx;
use std::path::PathBuf;

pub use daemon::{DaemonService, DaemonServiceData};
pub use error::{Error, Result};
pub use node::{NodeService, NodeServiceData};
pub use registry::{get_local_node_registry_path, NodeRegistryManager, StatusSummary};

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
/// Deprecated. We don't use this anymore, but keeping it for backward compatibility.
pub enum NatDetectionStatus {
    Public,
    UPnP,
    Private,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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
    async fn bin_path(&self) -> PathBuf;
    async fn build_upgrade_install_context(
        &self,
        options: UpgradeOptions,
    ) -> Result<ServiceInstallCtx>;
    async fn data_dir_path(&self) -> PathBuf;
    async fn is_user_mode(&self) -> bool;
    async fn log_dir_path(&self) -> PathBuf;
    async fn name(&self) -> String;
    async fn pid(&self) -> Option<u32>;
    async fn on_remove(&self);
    async fn on_start(&self, pid: Option<u32>, full_refresh: bool) -> Result<()>;
    async fn wait_until_started(&self) -> Result<()>;
    async fn on_stop(&self) -> Result<()>;
    async fn set_version(&self, version: &str);
    async fn status(&self) -> ServiceStatus;
    async fn set_status(&self, status: ServiceStatus);
    async fn set_metrics_port_if_not_set(&self, service_control: &dyn ServiceControl)
        -> Result<()>;
    async fn version(&self) -> String;
}
