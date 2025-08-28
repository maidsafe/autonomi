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
        let parallel_chunk_fetcher = move |chunk_names: &[(usize, XorName)]| -> Result<
            Vec<(usize, Bytes)>,
            self_encryption::Error,
        > {
            let chunk_addrs: Vec<(usize, ChunkAddress)> = chunk_names
                .iter()
                .map(|(i, name)| (*i, ChunkAddress::new(*name)))
                .collect();

            // Use tokio::task::block_in_place to handle async in sync context
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    self.fetch_chunks_parallel(&chunk_addrs, total_chunks, disable_cache)
                        .await
                        .map_err(|e| self_encryption::Error::Decryption(format!("{e:?}")))
                })
            })
        };

        // Stream decrypt directly to file
        streaming_decrypt_from_storage(data_map, to_dest, parallel_chunk_fetcher).map_err(|e| {
            error!("Streaming decryption failed: {e:?}");
            Error::GetError(GetError::Decryption(e))
        })?;

        #[cfg(feature = "loud")]
        println!(
            "Successfully streamed {total_chunks} to file {}",
            to_dest.display()
        );
        info!(
            "Successfully streamed {total_chunks} to file {}",
            to_dest.display()
        );

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

        let chunk_addrs: Vec<(usize, ChunkAddress)> = data_map
            .infos()
            .iter()
            .map(|info| (info.index, ChunkAddress::new(info.dst_hash)))
            .collect();

        let chunk_datas = self
            .fetch_chunks_parallel(&chunk_addrs, total_chunks, disable_cache)
            .await
            .map_err(GetError::Decryption)?;
        let encrypted_chunks: Vec<EncryptedChunk> = chunk_datas
            .into_iter()
            .map(|(_idx, content)| EncryptedChunk { content })
            .collect();

        debug!("Successfully fetched all {total_chunks} encrypted chunks");

        let data = decrypt(data_map, &encrypted_chunks).map_err(|e| {
            error!("Error decrypting encrypted_chunks: {e:?}");
            Error::GetError(GetError::Decryption(e))
        })?;

        #[cfg(feature = "loud")]
        println!("Successfully decrypted all {total_chunks} chunks");
        info!("Successfully decrypted all {total_chunks} chunks");

        // Clean up cache if not disabled
        if !disable_cache {
            self.cleanup_cached_chunks(&chunk_addrs);
        }

        Ok(data)
    }

    /// Fetch multiple chunks in parallel from the network, with caching support
    ///
    /// chunk_addresses shall holds the chunkinfo.index information.
    async fn fetch_chunks_parallel(
        &self,
        chunk_addresses: &[(usize, ChunkAddress)],
        total_chunks: usize,
        disable_cache: bool,
    ) -> Result<Vec<(usize, Bytes)>, self_encryption::Error> {
        let mut download_tasks = vec![];

        for (idx, chunk_addr) in chunk_addresses {
            let client_clone = self.clone();
            let addr_clone = *chunk_addr;

            download_tasks.push(async move {
                #[cfg(feature = "loud")]
                println!("Fetching chunk {idx}/{total_chunks} ...");
                info!("Fetching chunk {idx}/{total_chunks}({addr_clone:?})");

                // Try loading from cache first if caching is enabled
                let chunk = if !disable_cache
                    && client_clone.config.chunk_cache_enabled
                    && let Ok(Some(chunk)) = client_clone.try_load_chunk_from_cache(&addr_clone)
                {
                    // Try loading from cache first if caching is enabled
                    debug!("Loaded chunk from cache: {addr_clone:?}");
                    chunk
                } else {
                    // Fetch from network
                    let key = NetworkAddress::from(addr_clone);
                    debug!("Fetching chunk from network at: {key:?}");

                    let record = self
                        .network
                        .get_record_with_retries(key, &client_clone.config.chunks)
                        .await
                        .map_err(Error::NetworkError)?
                        .ok_or(Error::GetError(GetError::RecordNotFound))?;

                    // Deserialize chunk from record
                    crate::client::record_get::deserialize_chunk_from_record(&record)?
                };

                // Store in cache if enabled and not disabled
                if !disable_cache
                    && self.config.chunk_cache_enabled
                    && let Err(e) = self.try_cache_chunk(&addr_clone, &chunk)
                {
                    warn!("Failed to cache chunk {}: {e}", addr_clone.to_hex());
                }

                #[cfg(feature = "loud")]
                println!("Fetching chunk {idx}/{total_chunks} [DONE]");
                info!("Fetching chunk {idx}/{total_chunks}({addr_clone:?}) [DONE]");
                Ok((*idx, chunk.value().clone()))
            });
        }

        let results: Vec<Result<(usize, Bytes), _>> =
            process_tasks_with_max_concurrency(download_tasks, *CHUNK_DOWNLOAD_BATCH_SIZE).await;

        let mut chunks = vec![];
        let errors: Vec<Error> = results
            .into_iter()
            .filter_map(|res| match res {
                Ok(chunk) => {
                    chunks.push(chunk);
                    None
                }
                Err(e) => Some(e),
            })
            .collect();

        // When hit any error for one entry, return with error
        if !errors.is_empty() {
            Err(self_encryption::Error::Generic(format!("{errors:?}")))
        } else {
            #[cfg(feature = "loud")]
            println!("Successfully fetched all {total_chunks} encrypted chunks");
            info!("Successfully fetched all {total_chunks} encrypted chunks");
            Ok(chunks)
        }
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
    fn cleanup_cached_chunks(&self, chunk_addrs: &[(usize, ChunkAddress)]) {
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
