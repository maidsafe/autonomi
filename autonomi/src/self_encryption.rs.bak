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
use self_encryption::{DataMap, MAX_CHUNK_SIZE};
use serde::{Deserialize, Serialize};
use tracing::debug;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Encoding(#[from] rmp_serde::encode::Error),
    #[error(transparent)]
    SelfEncryption(#[from] self_encryption::Error),
}

#[derive(Serialize, Deserialize)]
pub(crate) enum DataMapLevel {
    // Holds the data map to the source data.
    First(DataMap),
    // Holds the data map of an _additional_ level of chunks
    // resulting from chunking up a previous level data map.
    // This happens when that previous level data map was too big to fit in a chunk itself.
    Additional(DataMap),
}

pub fn encrypt(data: Bytes) -> Result<(Chunk, Vec<Chunk>), Error> {
    let (data_map, chunks) = self_encryption::encrypt(data)?;
    let (data_map_chunk, additional_chunks) = pack_data_map(data_map)?;

    // Transform `EncryptedChunk` into `Chunk`
    let chunks: Vec<Chunk> = chunks
        .into_par_iter()
        .map(|c| Chunk::new(c.content.clone()))
        .chain(additional_chunks)
        .collect();

    Ok((data_map_chunk, chunks))
}

// Produces a chunk out of the first `DataMap`, which is validated for its size.
// If the chunk is too big, it is self-encrypted and the resulting (additional level) `DataMap` is put into a chunk.
// The above step is repeated as many times as required until the chunk size is valid.
// In other words: If the chunk content is too big, it will be
// self encrypted into additional chunks, and now we have a new `DataMap`
// which points to all of those additional chunks.. and so on.
fn pack_data_map(data_map: DataMap) -> Result<(Chunk, Vec<Chunk>), Error> {
    let mut chunks = vec![];
    let mut chunk_content = wrap_data_map(&DataMapLevel::First(data_map))?;

    let (data_map_chunk, additional_chunks) = loop {
        debug!("Max chunk size: {}", *MAX_CHUNK_SIZE);
        let chunk = Chunk::new(chunk_content);
        // If datamap chunk is less than `MAX_CHUNK_SIZE` return it so it can be directly sent to the network.
        if *MAX_CHUNK_SIZE >= chunk.size() {
            chunks.reverse();
            // Returns the last datamap, and all the chunks produced.
            break (chunk, chunks);
        } else {
            let mut bytes = BytesMut::with_capacity(*MAX_CHUNK_SIZE).writer();
            let mut serialiser = rmp_serde::Serializer::new(&mut bytes);
            chunk.serialize(&mut serialiser)?;
            let serialized_chunk = bytes.into_inner().freeze();

            let (data_map, next_encrypted_chunks) = self_encryption::encrypt(serialized_chunk)
                .inspect_err(|err| error!("Failed to encrypt chunks: {err:?}"))?;
            chunks = next_encrypted_chunks
                .iter()
                .map(|c| Chunk::new(c.content.clone())) // no need to encrypt what is self-encrypted
                .chain(chunks)
                .collect();
            chunk_content = wrap_data_map(&DataMapLevel::Additional(data_map))?;
        }
    };

    Ok((data_map_chunk, additional_chunks))
}

fn wrap_data_map(data_map: &DataMapLevel) -> Result<Bytes, rmp_serde::encode::Error> {
    // we use an initial/starting size of 300 bytes as that's roughly the current size of a DataMapLevel instance.
    let mut bytes = BytesMut::with_capacity(300).writer();
    let mut serialiser = rmp_serde::Serializer::new(&mut bytes);
    data_map
        .serialize(&mut serialiser)
        .inspect_err(|err| error!("Failed to serialize data map: {err:?}"))?;
    Ok(bytes.into_inner().freeze())
}
