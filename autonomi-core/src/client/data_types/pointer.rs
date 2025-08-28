// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::resolve_split_records;

use crate::{
    GetError,
    networking::{PeerId, Record},
};

use ant_protocol::{NetworkAddress, storage::try_deserialize_record};
use std::collections::HashMap;

pub use ant_protocol::storage::{Pointer, PointerAddress, PointerTarget};
pub use bls::{PublicKey, SecretKey};

/// Resolve a Pointer split.
pub(crate) fn resolve_pointer_split(
    result_map: HashMap<PeerId, Record>,
    network_addr: NetworkAddress,
) -> Result<Pointer, GetError> {
    info!("Pointer at {network_addr:?} is split, trying resolution");
    resolve_split_records(
        result_map,
        network_addr,
        |r| Ok(try_deserialize_record::<Pointer>(r)?),
        |p: &Pointer| p.counter(),
        |a: &Pointer, b: &Pointer| a == b,
    )
}
