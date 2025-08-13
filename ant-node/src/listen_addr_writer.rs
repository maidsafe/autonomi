// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

const LISTENER_FILE_NAME: &str = "listen_addrs.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A struct for writing listener addresses to a file.
pub(crate) struct ListenAddrWriter;

impl ListenAddrWriter {
    pub(crate) fn write(root_dir: PathBuf, address: Multiaddr) {
        #[allow(clippy::let_underscore_future)]
        let _ = tokio::spawn(async move {
            let mut listeners = match Self::load_from_file(&root_dir) {
                Ok(listeners) => listeners,
                Err(err) => {
                    error!("Error loading listeners during add: {err:?}");
                    return;
                }
            };

            if !listeners.contains(&address.to_string()) {
                listeners.push(address.to_string());
                if let Err(err) = Self::save_to_file(&listeners, &root_dir) {
                    error!("Error saving listeners during add: {err:?}");
                }
            }
        });
    }

    pub(crate) fn remove_listener(root_dir: PathBuf, addresses: Vec<Multiaddr>) {
        #[allow(clippy::let_underscore_future)]
        let _ = tokio::spawn(async move {
            let mut listeners = match Self::load_from_file(&root_dir) {
                Ok(listeners) => listeners,
                Err(err) => {
                    error!("Error loading listeners during remove: {err:?}");
                    return;
                }
            };
            let addresses = addresses
                .into_iter()
                .map(|addr| addr.to_string())
                .collect::<HashSet<_>>();
            listeners.retain(|addr| !addresses.contains(addr));
            if let Err(err) = Self::save_to_file(&listeners, &root_dir) {
                error!("Error saving listeners during remove: {err:?}");
            }
        });
    }

    pub(crate) fn reset(root_dir: PathBuf) {
        #[allow(clippy::let_underscore_future)]
        let _ = tokio::spawn(async move {
            let path = root_dir.join(LISTENER_FILE_NAME);
            if path.exists() {
                if let Err(err) = fs::remove_file(&path) {
                    error!("Error removing listeners file during reset: {err:?}");
                }
            }
        });
    }

    fn load_from_file(root_dir: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let path = root_dir.join(LISTENER_FILE_NAME);
        let data = if path.exists() {
            let content = fs::read_to_string(&path)?;
            let data: Vec<String> = serde_json::from_str(&content)?;
            data
        } else {
            Vec::new()
        };
        debug!("Loaded {} listeners from file: {path:?}", data.len());
        Ok(data)
    }

    fn save_to_file(
        listeners: &Vec<String>,
        root_dir: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !root_dir.exists() {
            fs::create_dir_all(root_dir)?;
        }
        let path = root_dir.join(LISTENER_FILE_NAME);

        let json = serde_json::to_string_pretty(&listeners)?;
        fs::write(&path, json)?;
        debug!("Saved listeners to file: {path:?}",);
        Ok(())
    }
}
