// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_protocol::storage::Chunk;
use bytes::{BufMut, Bytes, BytesMut};
use rayon::prelude::*;
use self_encryption::DataMap;
use self_encryption_old::DataMap as OldDataMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Encoding(#[from] rmp_serde::encode::Error),
    #[error(transparent)]
    SelfEncryption(#[from] self_encryption::Error),
}

#[derive(Serialize, Deserialize)]
pub enum DataMapLevel {
    // Holds the datamap to the source data.
    First(OldDataMap),
    // Holds the datamap of an _additional_ level of chunks
    // resulting from chunking up a previous level datamap.
    // This happens when that previous level datamap was too big to fit in a chunk itself.
    Additional(OldDataMap),
}

pub fn encrypt(data: Bytes) -> Result<(Chunk, Vec<Chunk>), Error> {
    let (data_map, chunks) = self_encryption::encrypt(data)?;
    let data_map_chunk = pack_data_map(data_map)?;

    // Transform `EncryptedChunk` into `Chunk`
    let chunks: Vec<Chunk> = chunks
        .into_par_iter()
        .map(|c| Chunk::new(c.content.clone()))
        .collect();

    Ok((data_map_chunk, chunks))
}

// Produces a chunk out of the first `DataMap`, which is validated for its size.
// self-encryption now returns the root_data_map only, which points to the three datamap_chunks.
// Hence guaranteed can be packed into one chunk.
fn pack_data_map(data_map: DataMap) -> Result<Chunk, Error> {
    let chunk_content = wrap_data_map(&data_map)?;
    Ok(Chunk::new(chunk_content))
}

fn wrap_data_map(data_map: &DataMap) -> Result<Bytes, rmp_serde::encode::Error> {
    // we use an initial/starting size of 300 bytes as that's roughly the current size of a DataMap instance.
    let mut bytes = BytesMut::with_capacity(300).writer();
    let mut serialiser = rmp_serde::Serializer::new(&mut bytes);
    data_map
        .serialize(&mut serialiser)
        .inspect_err(|err| error!("Failed to serialize datamap: {err:?}"))?;
    Ok(bytes.into_inner().freeze())
}
