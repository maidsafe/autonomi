// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::ChunkAddress;
use crate::client::data_types::chunk::DataMapChunk;
use crate::client::{Client, GetError};
use crate::self_encryption::DataMapLevel;
use bytes::Bytes;
use eyre::Result;
use self_encryption::{ChunkInfo, DataMap, get_root_data_map};
use xor_name::XorName;

impl Client {
    /// Restore a complete datamap from a DataMapChunk, handling both old and new formats
    /// This function properly handles recursive DataMapLevel schemes by traversing all levels
    /// until it reaches the root datamap containing the actual file chunks.
    ///
    /// self_encryption now changed to always return a data_map pointing to the 3 datamap_chunks.
    /// Hence, the input data_map_bytes is actually the root_data_map pointing to that 3 datamap_chunks.
    /// The downloading work flow then shall be:
    ///   * fetch that 3 datamap_chunks first
    ///   * compose the real datamap of the file, from that 3 datamap_chunks
    ///   * repeat the above two steps if the recovered data_map contains chiled
    ///   * fetch the leftover content chunks to compose the file
    pub(crate) async fn restore_data_map_from_chunk(
        &self,
        data_map_chunk: &DataMapChunk,
    ) -> Result<DataMap, GetError> {
        let mut data_map_bytes = data_map_chunk.0.value().clone();

        // In case the input is the new data_map root, restore it further,
        // before going into the further recursive.
        if let Ok(data_map) = rmp_serde::from_slice::<DataMap>(&data_map_bytes) {
            info!("Restoring from new root data_map:\n{data_map:?}");
            let file_data_map = self.fetch_new_data_map(&data_map)?;

            info!(
                "Fetched file data_map from {:?} of new version: \n{file_data_map:?}",
                data_map_chunk.0.address()
            );
            return Ok(file_data_map);
        }

        loop {
            // The data_map_bytes could be an Archive, we shall return earlier for that case
            match Self::deserialize_data_map(&data_map_bytes) {
                Ok(mut data_map) => {
                    info!("Restoring from data_map:\n{data_map:?}");
                    if !data_map.is_child() {
                        return Ok(data_map);
                    }
                    data_map.child = None;
                    data_map_bytes = self.fetch_from_data_map(&data_map).await?;
                }
                Err(e) => {
                    info!("Failed to deserialize data_map_bytes: {e:?}");
                    return Err(GetError::Configuration(format!("{e:?}")));
                }
            }
        }
    }

    /// Fetch the file data_map from the root one using lazy evaluation.
    /// Chunks are only fetched from the network when actually needed by get_root_data_map.
    fn fetch_new_data_map(&self, data_map: &DataMap) -> Result<DataMap, GetError> {
        let total_chunks = data_map.infos().len();
        crate::loud_debug!("Using lazy chunk fetching for {total_chunks} of datamap {data_map:?}");

        // Create a closure that fetches chunks on-demand
        let client = self.clone();
        let mut chunk_fetcher = move |xor_name: XorName| -> Result<Bytes, self_encryption::Error> {
            let chunk_addr = ChunkAddress::new(xor_name);

            // Use tokio::task::spawn_blocking to handle the async operation in a sync context
            let fetch_result = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current()
                    .block_on(async { client.chunk_get(&chunk_addr).await })
            });

            match fetch_result {
                Ok(chunk) => {
                    crate::loud_debug!("Successfully fetched chunk at: {chunk_addr:?}");

                    // Such datamap chunks shall be cleanup from chunk_cache immediately
                    client.cleanup_cached_chunks(&[chunk_addr]);

                    Ok(chunk.value)
                }
                Err(err) => {
                    crate::loud_error!("Error fetching chunk at {chunk_addr:?}: {err:?}");
                    Err(self_encryption::Error::Generic(format!(
                        "Failed to fetch chunk at {chunk_addr:?}: {err:?}"
                    )))
                }
            }
        };

        let result_data_map =
            get_root_data_map(data_map.clone(), &mut chunk_fetcher).map_err(|e| {
                error!("Error processing data_map: {e:?}");
                GetError::Decryption(crate::self_encryption::Error::SelfEncryption(e))
            })?;

        crate::loud_debug!("Successfully processed datamap with lazy chunk fetching");

        Ok(result_data_map)
    }

    /// Deserialize datamap from bytes, handling both old and new formats
    pub fn deserialize_data_map(data_map_bytes: &Bytes) -> Result<DataMap, GetError> {
        // Try new format first
        if let Ok(data_map) = rmp_serde::from_slice::<DataMap>(data_map_bytes) {
            return Ok(data_map);
        }

        // Fall back to old format and convert
        let data_map_level = rmp_serde::from_slice::<DataMapLevel>(data_map_bytes)
            .map_err(GetError::InvalidDataMap)?;

        let (old_data_map, child) = match &data_map_level {
            DataMapLevel::First(map) => (map, None),
            DataMapLevel::Additional(map) => (map, Some(0)),
        };

        // Convert to new format
        let chunk_identifiers: Vec<ChunkInfo> = old_data_map
            .infos()
            .iter()
            .map(|ck_info| ChunkInfo {
                index: ck_info.index,
                dst_hash: ck_info.dst_hash,
                src_hash: ck_info.src_hash,
                src_size: ck_info.src_size,
            })
            .collect();

        Ok(DataMap {
            chunk_identifiers,
            child,
        })
    }
}
