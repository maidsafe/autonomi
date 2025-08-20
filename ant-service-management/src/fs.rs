// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::error::Result;
use libp2p::Multiaddr;
use std::fs::OpenOptions;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub listeners: Vec<Multiaddr>,
}

pub trait FileSystemActions: Sync {
    fn node_info(&self, root_dir: &Path) -> Result<NodeInfo>;
}

#[derive(Debug, Clone, Default)]
pub struct FileSystemClient;

impl FileSystemActions for FileSystemClient {
    fn node_info(&self, root_dir: &Path) -> Result<NodeInfo> {
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
        let node_info = NodeInfo { listeners };
        Ok(node_info)
    }
}
