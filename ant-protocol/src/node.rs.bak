// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::error::{Error, Result};
use libp2p::PeerId;
use std::path::PathBuf;

/// Get the default antnode root dir for the provided PeerId
pub fn get_antnode_root_dir(peer_id: PeerId) -> Result<PathBuf> {
    let dir = dirs_next::data_dir()
        .ok_or_else(|| Error::CouldNotObtainDataDir)?
        .join("autonomi")
        .join("node")
        .join(peer_id.to_string());
    Ok(dir)
}
