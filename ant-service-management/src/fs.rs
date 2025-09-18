// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::error::Result;
use chrono::{DateTime, Utc};
use libp2p::Multiaddr;
use std::fs::OpenOptions;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
// This struct is defined in ant-node. Keep the schema in sync.
pub struct CriticalFailure {
    pub date_time: DateTime<Utc>,
    pub reason: String,
}

pub trait FileSystemActions: Sync {
    fn listen_addrs(&self, root_dir: &Path) -> Result<Vec<Multiaddr>>;
    fn critical_failure(&self, root_dir: &Path) -> Result<Option<CriticalFailure>>;
}

#[derive(Debug, Clone, Default)]
pub struct FileSystemClient;

impl FileSystemActions for FileSystemClient {
    fn listen_addrs(&self, root_dir: &Path) -> Result<Vec<Multiaddr>> {
        let listen_addrs_path = root_dir.join("listen_addrs.json");
        let mut listeners = Vec::new();

        if listen_addrs_path.exists() {
            let mut file = OpenOptions::new().read(true).open(&listen_addrs_path)?;

            file.lock_shared()?;
            let mut contents = String::new();
            file.read_to_string(&mut contents)?;
            file.unlock()?;

            let listeners_str: Vec<String> = if contents.trim().is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(&contents).unwrap_or_else(|_| Vec::new())
            };

            for addr in listeners_str {
                match addr.parse::<Multiaddr>() {
                    Ok(multiaddr) => {
                        listeners.push(multiaddr);
                    }
                    Err(err) => {
                        error!("Failed to parse Multiaddr from string '{addr}': {err}");
                    }
                }
            }
        }
        Ok(listeners)
    }

    fn critical_failure(&self, root_dir: &Path) -> Result<Option<CriticalFailure>> {
        let critical_failure_path = root_dir.join("critical_failure.json");

        let critical_failure = if critical_failure_path.exists() {
            let mut file = OpenOptions::new().read(true).open(&critical_failure_path)?;

            file.lock_shared()?;
            let mut contents = String::new();
            file.read_to_string(&mut contents)?;
            file.unlock()?;

            if contents.trim().is_empty() {
                None
            } else {
                let failure: CriticalFailure = serde_json::from_str(&contents)?;
                info!("Critical failure found: {failure:?}");
                Some(failure)
            }
        } else {
            None
        };

        Ok(critical_failure)
    }
}
