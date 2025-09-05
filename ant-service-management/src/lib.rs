// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// Allow unwrap/expect usage temporarily
#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

pub mod control;
pub mod error;
pub mod fs;
pub mod metric;
pub mod node;
pub mod registry;

#[macro_use]
extern crate tracing;

pub mod antctl_proto {
    #![allow(clippy::clone_on_ref_ptr)]
    tonic::include_proto!("antctl_proto");
}

use crate::control::ServiceControl;
use async_trait::async_trait;
use semver::Version;
use serde::{Deserialize, Serialize};
use service_manager::ServiceInstallCtx;
use std::path::PathBuf;

pub use error::{Error, Result};
pub use node::{NodeService, NodeServiceData};
pub use registry::{NodeRegistryManager, StatusSummary, get_local_node_registry_path};

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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ReachabilityProgress {
    /// Reachability check has not been run (0%)
    #[default]
    NotRun,
    /// Reachability check is in progress (1-99%)
    InProgress(u8),
    /// Reachability check is completed (100%)
    Complete,
}

impl From<f64> for ReachabilityProgress {
    fn from(value: f64) -> Self {
        if value == 0.0 {
            ReachabilityProgress::NotRun
        } else if value > 0.0 && value < 100.0 {
            ReachabilityProgress::InProgress(value as u8)
        } else {
            ReachabilityProgress::Complete
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceStartupStatus {
    /// Service startup is in progress with percentage value (0-99)
    InProgress(u8),
    /// Service has completed startup (reachability check complete)
    Started,
    /// Service startup has failed
    Failed { reason: String },
}

impl From<ReachabilityProgress> for ServiceStartupStatus {
    fn from(progress: ReachabilityProgress) -> Self {
        match progress {
            ReachabilityProgress::Complete => ServiceStartupStatus::Started,
            ReachabilityProgress::NotRun => ServiceStartupStatus::Started,
            ReachabilityProgress::InProgress(value) => ServiceStartupStatus::InProgress(value),
        }
    }
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
    /// Returns the startup status of the service
    async fn startup_status(&self) -> Result<ServiceStartupStatus>;
    async fn on_stop(&self) -> Result<()>;
    async fn set_version(&self, version: &str);
    async fn status(&self) -> ServiceStatus;
    async fn set_status(&self, status: ServiceStatus);
    async fn set_metrics_port_if_not_set(&self, service_control: &dyn ServiceControl)
    -> Result<()>;
    async fn version(&self) -> String;
}
