// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    control::ServiceControl,
    error::{Error, Result},
    ServiceStateActions, ServiceStatus, UpgradeOptions,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use service_manager::ServiceInstallCtx;
use std::{ffi::OsString, net::SocketAddr, path::PathBuf, sync::Arc};
use tokio::sync::RwLock;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DaemonServiceData {
    pub daemon_path: PathBuf,
    pub endpoint: Option<SocketAddr>,
    pub pid: Option<u32>,
    pub service_name: String,
    pub status: ServiceStatus,
    pub version: String,
}

pub struct DaemonService {
    pub service_data: Arc<RwLock<DaemonServiceData>>,
    pub service_control: Box<dyn ServiceControl + Send>,
}

impl DaemonService {
    pub fn new(
        service_data: Arc<RwLock<DaemonServiceData>>,
        service_control: Box<dyn ServiceControl + Send>,
    ) -> DaemonService {
        DaemonService {
            service_data,
            service_control,
        }
    }
}

#[async_trait]
impl ServiceStateActions for DaemonService {
    async fn bin_path(&self) -> PathBuf {
        self.service_data.read().await.daemon_path.clone()
    }

    async fn build_upgrade_install_context(
        &self,
        _options: UpgradeOptions,
    ) -> Result<ServiceInstallCtx> {
        let (address, port) = self
            .service_data
            .read()
            .await
            .endpoint
            .ok_or_else(|| {
                error!("Daemon endpoint not set in the service_data");
                Error::DaemonEndpointNotSet
            })
            .map(|e| (e.ip().to_string(), e.port().to_string()))?;

        let install_ctx = ServiceInstallCtx {
            args: vec![
                OsString::from("--port"),
                OsString::from(port),
                OsString::from("--address"),
                OsString::from(address),
            ],
            autostart: true,
            contents: None,
            environment: None,
            label: self.service_data.read().await.service_name.parse()?,
            program: self.service_data.read().await.daemon_path.clone(),
            username: None,
            working_directory: None,
            disable_restart_on_failure: false,
        };
        Ok(install_ctx)
    }

    async fn data_dir_path(&self) -> PathBuf {
        PathBuf::new()
    }

    async fn is_user_mode(&self) -> bool {
        // The daemon service should never run in user mode.
        false
    }

    async fn log_dir_path(&self) -> PathBuf {
        PathBuf::new()
    }

    async fn name(&self) -> String {
        self.service_data.read().await.service_name.clone()
    }

    async fn pid(&self) -> Option<u32> {
        self.service_data.read().await.pid
    }

    async fn on_remove(&self) {
        self.service_data.write().await.status = ServiceStatus::Removed;
    }

    async fn on_start(&self, pid: Option<u32>, _full_refresh: bool) -> Result<()> {
        self.service_data.write().await.pid = pid;
        self.service_data.write().await.status = ServiceStatus::Running;
        Ok(())
    }

    async fn wait_until_started(&self) -> Result<()> {
        // Daemon service does not require waiting for start.
        Ok(())
    }

    async fn on_stop(&self) -> Result<()> {
        self.service_data.write().await.pid = None;
        self.service_data.write().await.status = ServiceStatus::Stopped;
        Ok(())
    }

    async fn set_version(&self, version: &str) {
        self.service_data.write().await.version = version.to_string();
    }

    async fn status(&self) -> ServiceStatus {
        self.service_data.read().await.status.clone()
    }

    async fn set_status(&self, status: ServiceStatus) {
        self.service_data.write().await.status = status;
    }

    async fn version(&self) -> String {
        self.service_data.read().await.version.clone()
    }

    async fn set_metrics_port_if_not_set(
        &self,
        _service_control: &dyn ServiceControl,
    ) -> Result<()> {
        Ok(())
    }
}
