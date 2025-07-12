// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use libp2p::{core::transport::ListenerId, Multiaddr};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

const LISTENER_FILE_NAME: &str = "listen_addrs.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListenAddrWriter {
    #[serde(skip)]
    file_path: PathBuf,
    listeners: HashMap<String, String>,
}

impl ListenAddrWriter {
    pub fn new(base_dir: &Path) -> Self {
        let file_path = base_dir.join(LISTENER_FILE_NAME);
        let listeners = Self::load_from_file(&file_path).unwrap_or_default();
        
        Self {
            file_path,
            listeners,
        }
    }

    pub fn add_listener(&mut self, listener_id: ListenerId, address: Multiaddr) {
        self.listeners.insert(format!("{:?}", listener_id), address.to_string());
        let _ = self.save_to_file();
    }

    pub fn remove_listener(&mut self, listener_id: &ListenerId) {
        self.listeners.remove(&format!("{:?}", listener_id));
        let _ = self.save_to_file();
    }

    pub fn remove_address(&mut self, address: &Multiaddr) {
        let addr_string = address.to_string();
        self.listeners.retain(|_, v| v != &addr_string);
        let _ = self.save_to_file();
    }

    pub fn get_listeners(&self) -> &HashMap<String, String> {
        &self.listeners
    }

    fn load_from_file(path: &Path) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        let data: ListenAddrData = serde_json::from_str(&content)?;
        Ok(data.listeners)
    }

    fn save_to_file(&self) -> Result<(), Box<dyn std::error::Error>> {
        let data = ListenAddrData {
            listeners: self.listeners.clone(),
        };
        
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        let json = serde_json::to_string_pretty(&data)?;
        fs::write(&self.file_path, json)?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ListenAddrData {
    listeners: HashMap<String, String>,
}
