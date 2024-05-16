// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    control::ServiceControl, error::Result, ServiceStateActions, ServiceStatus, UpgradeOptions,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use service_manager::ServiceInstallCtx;
use std::{ffi::OsString, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditorServiceData {
    pub auditor_path: PathBuf,
    pub log_dir_path: PathBuf,
    pub pid: Option<u32>,
    pub service_name: String,
    pub status: ServiceStatus,
    pub user: String,
    pub version: String,
}

pub struct AuditorService<'a> {
    pub service_data: &'a mut AuditorServiceData,
    pub service_control: Box<dyn ServiceControl + Send>,
}

impl<'a> AuditorService<'a> {
    pub fn new(
        service_data: &'a mut AuditorServiceData,
        service_control: Box<dyn ServiceControl + Send>,
    ) -> AuditorService<'a> {
        AuditorService {
            service_data,
            service_control,
        }
    }
}

#[async_trait]
impl<'a> ServiceStateActions for AuditorService<'a> {
    fn bin_path(&self) -> PathBuf {
        self.service_data.auditor_path.clone()
    }

    fn build_upgrade_install_context(&self, options: UpgradeOptions) -> Result<ServiceInstallCtx> {
        let mut args = vec![
            OsString::from("--log-output-dest"),
            OsString::from(self.service_data.log_dir_path.to_string_lossy().to_string()),
        ];

        if !options.bootstrap_peers.is_empty() {
            let peers_str = options
                .bootstrap_peers
                .iter()
                .map(|peer| peer.to_string())
                .collect::<Vec<_>>()
                .join(",");
            args.push(OsString::from("--peer"));
            args.push(OsString::from(peers_str));
        }

        args.push(OsString::from("server"));

        Ok(ServiceInstallCtx {
            label: self.service_data.service_name.parse()?,
            program: self.service_data.auditor_path.to_path_buf(),
            args,
            contents: None,
            username: Some(self.service_data.user.to_string()),
            working_directory: None,
            environment: options.env_variables,
        })
    }

    fn data_dir_path(&self) -> PathBuf {
        PathBuf::new()
    }

    fn is_user_mode(&self) -> bool {
        // The auditor service should never run in user mode.
        false
    }

    fn log_dir_path(&self) -> PathBuf {
        PathBuf::new()
    }

    fn name(&self) -> String {
        self.service_data.service_name.clone()
    }

    fn pid(&self) -> Option<u32> {
        self.service_data.pid
    }

    fn on_remove(&mut self) {
        self.service_data.status = ServiceStatus::Removed;
    }

    async fn on_start(&mut self) -> Result<()> {
        let pid = self
            .service_control
            .get_process_pid(&self.service_data.auditor_path)?;
        self.service_data.pid = Some(pid);
        self.service_data.status = ServiceStatus::Running;
        Ok(())
    }

    async fn on_stop(&mut self) -> Result<()> {
        self.service_data.pid = None;
        self.service_data.status = ServiceStatus::Stopped;
        Ok(())
    }

    fn set_version(&mut self, version: &str) {
        self.service_data.version = version.to_string();
    }

    fn status(&self) -> ServiceStatus {
        self.service_data.status.clone()
    }

    fn version(&self) -> String {
        self.service_data.version.clone()
    }
}
