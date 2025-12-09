// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Helper module for serializing/deserializing `Vec<PeerId>`

use libp2p::PeerId;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub fn serialize<S>(peers: &[PeerId], serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let bytes_vec: Vec<Vec<u8>> = peers.iter().map(|p| p.to_bytes()).collect();
    bytes_vec.serialize(serializer)
}

pub fn deserialize<'de, D>(deserializer: D) -> std::result::Result<Vec<PeerId>, D::Error>
where
    D: Deserializer<'de>,
{
    let bytes_vec: Vec<Vec<u8>> = Vec::deserialize(deserializer)?;
    bytes_vec
        .into_iter()
        .map(|bytes| PeerId::from_bytes(&bytes).map_err(serde::de::Error::custom))
        .collect()
}
