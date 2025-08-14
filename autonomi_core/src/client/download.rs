// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Upload and data retrieval implementation for the Autonomi core client
//!
//! This module provides data map handling, chunk fetching, and streaming download capabilities

use super::{Client, Error, GetError};
use crate::client::data_types::chunk::CHUNK_DOWNLOAD_BATCH_SIZE;
use crate::client::utils::{chunk_cache, process_tasks_with_max_concurrency};

use ant_protocol::NetworkAddress;
use ant_protocol::storage::{Chunk, ChunkAddress};
use bytes::Bytes;
use self_encryption::{DataMap, EncryptedChunk, decrypt, streaming_decrypt_from_storage};
use std::path::{Path, PathBuf};
use tracing::{debug, error, info, warn};
use xor_name::XorName;

impl Client {
    /// Fetch data from a DataMapChunk, handling decryption and chunk assembly.
    /// * when to_dest is provided, using the streaming_decryption and flush the content to dest_file directly, return None bytes in that case.
    /// * when disable_cache is false, flush the fetched chunks to cache, to allow later on continuous from the broken point.
    pub async fn fetch_from_data_map(
        &self,
        data_map: &DataMap,
        to_dest: Option<PathBuf>,
        disable_cache: bool,
    ) -> Result<Option<Bytes>, Error> {
        info!("Fetching from data_map: {data_map:?}");

        // Handle streaming download to file
        if let Some(dest_path) = to_dest {
            self.stream_download_chunks_to_file(data_map, &dest_path, disable_cache)?;
            return Ok(None);
        }

        // Handle regular download to memory
        self.fetch_from_data_map_to_memory(data_map, disable_cache)
            .await
            .map(Some)
    }

    /// Stream decrypt chunks directly to file
    fn stream_download_chunks_to_file(
        &self,
        data_map: &DataMap,
        to_dest: &Path,
        disable_cache: bool,
    ) -> Result<(), Error> {
        let total_chunks = data_map.infos().len();
        info!(
            "Streaming {total_chunks} chunks to file: {}",
            to_dest.display()
        );

        // Create parallel chunk fetcher for streaming decryption
        let client_clone = self.clone();

        let parallel_chunk_fetcher = move |chunk_names: &[(usize, XorName)]| -> Result<
            Vec<(usize, Bytes)>,
            self_encryption::Error,
        > {
            let chunk_addresses: Vec<(usize, ChunkAddress)> = chunk_names
                .iter()
                .map(|(i, name)| (*i, ChunkAddress::new(*name)))
                .collect();

            // Use tokio::task::block_in_place to handle async in sync context
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    let mut chunk_futures = Vec::new();

                    for (index, chunk_addr) in chunk_addresses {
                        let client = client_clone.clone();
                        chunk_futures.push(async move {
                            match client
                                .fetch_chunks_parallel(chunk_addr, disable_cache)
                                .await
                            {
                                Ok(chunk_data) => Ok((index, chunk_data)),
                                Err(err) => {
                                    error!("Error fetching chunk {chunk_addr:?}: {err:?}");
                                    Err(self_encryption::Error::Generic(format!(
                                        "Network error: {err:?}"
                                    )))
                                }
                            }
                        });
                    }

                    let results = process_tasks_with_max_concurrency(
                        chunk_futures,
                        *CHUNK_DOWNLOAD_BATCH_SIZE,
                    )
                    .await;
                    results.into_iter().collect::<Result<Vec<_>, _>>()
                })
            })
        };

        // Stream decrypt directly to file
        streaming_decrypt_from_storage(data_map, to_dest, parallel_chunk_fetcher).map_err(|e| {
            error!("Streaming decryption failed: {e:?}");
            Error::GetError(GetError::Decryption(e))
        })?;

        info!("Successfully streamed {total_chunks} chunks to file");
        Ok(())
    }

    /// Fetch and decrypt all chunks in the datamap to memory
    async fn fetch_from_data_map_to_memory(
        &self,
        data_map: &DataMap,
        disable_cache: bool,
    ) -> Result<Bytes, Error> {
        let total_chunks = data_map.infos().len();
        debug!("Fetching {total_chunks} encrypted data chunks from datamap");

        let mut download_tasks = vec![];
        let chunk_addrs: Vec<ChunkAddress> = data_map
            .infos()
            .iter()
            .map(|info| ChunkAddress::new(info.dst_hash))
            .collect();

        for (i, info) in data_map.infos().into_iter().enumerate() {
            let client = self.clone();
            download_tasks.push(async move {
                let idx = i + 1;
                let chunk_addr = ChunkAddress::new(info.dst_hash);

                info!("Fetching chunk {idx}/{total_chunks}({chunk_addr:?})");

                match client
                    .fetch_chunks_parallel(chunk_addr, disable_cache)
                    .await
                {
                    Ok(chunk_data) => {
                        info!("Successfully fetched chunk {idx}/{total_chunks}({chunk_addr:?})");
                        Ok(EncryptedChunk {
                            content: chunk_data,
                        })
                    }
                    Err(err) => {
                        error!(
                            "Error fetching chunk {idx}/{total_chunks}({chunk_addr:?}): {err:?}"
                        );
                        Err(err)
                    }
                }
            });
        }

        let encrypted_chunks =
            process_tasks_with_max_concurrency(download_tasks, *CHUNK_DOWNLOAD_BATCH_SIZE)
                .await
                .into_iter()
                .collect::<Result<Vec<EncryptedChunk>, Error>>()?;

        debug!("Successfully fetched all {total_chunks} encrypted chunks");

        let data = decrypt(data_map, &encrypted_chunks).map_err(|e| {
            error!("Error decrypting encrypted_chunks: {e:?}");
            Error::GetError(GetError::Decryption(e))
        })?;

        debug!("Successfully decrypted all {total_chunks} chunks");

        // Clean up cache if not disabled
        if !disable_cache {
            self.cleanup_cached_chunks(&chunk_addrs);
        }

        Ok(data)
    }

    /// Fetch a single chunk with caching support
    async fn fetch_chunks_parallel(
        &self,
        chunk_addr: ChunkAddress,
        disable_cache: bool,
    ) -> Result<Bytes, Error> {
        // Try loading from cache first if caching is enabled
        if !disable_cache
            && self.config.chunk_cache_enabled
            && let Ok(Some(chunk)) = self.try_load_chunk_from_cache(&chunk_addr)
        {
            debug!("Loaded chunk from cache: {chunk_addr:?}");
            return Ok(chunk.value().clone());
        }

        // Fetch from network
        let key = NetworkAddress::from(chunk_addr);
        debug!("Fetching chunk from network at: {key:?}");

        let record = self
            .network
            .get_record_with_retries(key, &self.config.chunks)
            .await
            .map_err(Error::NetworkError)?
            .ok_or(Error::GetError(GetError::RecordNotFound))?;

        // Deserialize chunk from record
        let chunk = crate::client::record_get::deserialize_chunk_from_record(&record)?;
        let chunk_data = chunk.value().clone();

        // Store in cache if enabled and not disabled
        if !disable_cache
            && self.config.chunk_cache_enabled
            && let Err(e) = self.try_cache_chunk(&chunk_addr, &chunk)
        {
            warn!("Failed to cache chunk {}: {}", chunk_addr.to_hex(), e);
        }

        Ok(chunk_data)
    }

    /// Get chunk cache directory
    fn get_chunk_cache_dir(&self) -> Result<PathBuf, Error> {
        match &self.config.chunk_cache_dir {
            Some(dir) => Ok(dir.clone()),
            None => chunk_cache::default_cache_dir().map_err(|e| {
                Error::GetError(GetError::Configuration(format!(
                    "Chunk caching is enabled but no cache directory is specified: {e}"
                )))
            }),
        }
    }

    /// Try to load a chunk from cache
    fn try_load_chunk_from_cache(&self, addr: &ChunkAddress) -> Result<Option<Chunk>, Error> {
        if !self.config.chunk_cache_enabled {
            return Ok(None);
        }

        let cache_dir = self.get_chunk_cache_dir()?;
        match chunk_cache::load_chunk(cache_dir, addr) {
            Ok(result) => Ok(result),
            Err(err) => {
                warn!("Loading chunk {addr:?} from cache got error: {err}");
                Ok(None)
            }
        }
    }

    /// Try to cache a chunk
    fn try_cache_chunk(&self, addr: &ChunkAddress, chunk: &Chunk) -> Result<(), Error> {
        if self.config.chunk_cache_enabled {
            let cache_dir = self.get_chunk_cache_dir()?;
            chunk_cache::store_chunk(cache_dir, addr, chunk).map_err(|e| {
                Error::GetError(GetError::Configuration(format!("Cache store error: {e}")))
            })?;
        }
        Ok(())
    }

    /// Clean up cached chunks after successful download
    fn cleanup_cached_chunks(&self, chunk_addrs: &[ChunkAddress]) {
        if self.config.chunk_cache_enabled
            && let Ok(cache_dir) = self.get_chunk_cache_dir()
        {
            if let Err(e) = chunk_cache::delete_chunks(cache_dir, chunk_addrs) {
                warn!("Failed to delete cached chunks after download: {e:?}");
            } else {
                debug!(
                    "Deleted {} cached chunks after successful download",
                    chunk_addrs.len()
                );
            }
        }
    }
}
