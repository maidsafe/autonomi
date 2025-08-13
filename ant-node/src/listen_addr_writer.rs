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
    fs::{self, OpenOptions},
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
};

const LISTENER_FILE_NAME: &str = "listen_addrs.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A struct for writing listener addresses to a file.
pub(crate) struct ListenAddrWriter;

impl ListenAddrWriter {
    /// Add a listener address to the file
    pub(crate) fn add_listeners(root_dir: PathBuf, address: Multiaddr) {
        #[allow(clippy::let_underscore_future)]
        let _ = tokio::spawn(async move {
            info!("Writing listener address {address:?} to {LISTENER_FILE_NAME} file");
            let address_str = address.to_string();
            if let Err(err) = Self::modify_listeners_file(&root_dir, |listeners| {
                if !listeners.contains(&address_str) {
                    listeners.push(address_str);
                }
            }) {
                error!("Error adding listener: {err:?}");
            }
        });
    }

    /// Remove listener addresses from the file
    pub(crate) fn remove_listener(root_dir: PathBuf, addresses: Vec<Multiaddr>) {
        #[allow(clippy::let_underscore_future)]
        let _ = tokio::spawn(async move {
            info!("Removing listener addresses {addresses:?} from {LISTENER_FILE_NAME} file");
            let addresses_to_remove = addresses
                .into_iter()
                .map(|addr| addr.to_string())
                .collect::<HashSet<_>>();

            if let Err(err) = Self::modify_listeners_file(&root_dir, |listeners| {
                listeners.retain(|addr| !addresses_to_remove.contains(addr));
            }) {
                error!("Error removing listeners: {err:?}");
            }
        });
    }

    /// Reset/delete the listeners file
    pub(crate) fn reset(root_dir: PathBuf) {
        #[allow(clippy::let_underscore_future)]
        let _ = tokio::spawn(async move {
            let path = root_dir.join(LISTENER_FILE_NAME);
            if path.exists() {
                info!("Removing listeners file {path:?}");
                if let Err(err) = fs::remove_file(&path) {
                    error!("Error removing listeners file during reset: {err:?}");
                }
            }
        });
    }

    /// Performs an atomic read-modify-write operation on the listeners file with exclusive locking
    fn modify_listeners_file<F>(
        root_dir: &Path,
        modifier: F,
    ) -> Result<(), Box<dyn std::error::Error>>
    where
        F: FnOnce(&mut Vec<String>),
    {
        // Ensure directory exists
        if !root_dir.exists() {
            fs::create_dir_all(root_dir)?;
        }
        let path = root_dir.join(LISTENER_FILE_NAME);

        // Open file with read/write permissions, create if it doesn't exist
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)?;

        // Acquire exclusive lock for the entire read-modify-write operation
        file.lock()?;

        // Read existing data
        let mut content = String::new();
        let _ = file.read_to_string(&mut content)?;

        let mut listeners = if content.trim().is_empty() {
            Vec::new()
        } else {
            serde_json::from_str(&content).unwrap_or_else(|_| Vec::new())
        };

        // Apply the modification
        modifier(&mut listeners);

        // Write back the modified data
        let _ = file.seek(std::io::SeekFrom::Start(0))?;
        file.set_len(0)?; // Truncate the file
        let json = serde_json::to_string_pretty(&listeners)?;
        file.write_all(json.as_bytes())?;

        debug!("Modified listeners file: {path:?}");
        Ok(())
    }
}
