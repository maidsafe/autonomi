// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

pub use ant_protocol::storage::{Scratchpad, ScratchpadAddress};
pub use bls::{PublicKey, SecretKey, Signature};

use super::resolve_split_records;

use crate::{
    GetError,
    networking::{PeerId, Record},
};

use ant_protocol::{NetworkAddress, storage::try_deserialize_record};
use std::collections::HashMap;

/// Resolve a Scratchpad split.
pub(crate) fn resolve_scratchpad_split(
    result_map: HashMap<PeerId, Record>,
    network_addr: NetworkAddress,
) -> Result<Scratchpad, GetError> {
    info!("Scratchpad at {network_addr:?} is split, trying resolution");
    resolve_split_records(
        result_map,
        network_addr.clone(),
        |r| Ok(try_deserialize_record::<Scratchpad>(r)?),
        |s: &Scratchpad| s.counter(),
        |a: &Scratchpad, b: &Scratchpad| {
            a.data_encoding() == b.data_encoding() && a.encrypted_data() == b.encrypted_data()
        },
    )
}
