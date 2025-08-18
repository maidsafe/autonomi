// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::{Error, GetError};

use ant_protocol::NetworkAddress;
use ant_protocol::storage::DataTypes;
use futures::stream::{FuturesUnordered, StreamExt};

pub(crate) fn determine_data_type_from_address(
    address: &NetworkAddress,
) -> Result<DataTypes, Error> {
    match address {
        NetworkAddress::ChunkAddress(_) => Ok(DataTypes::Chunk),
        NetworkAddress::GraphEntryAddress(_) => Ok(DataTypes::GraphEntry),
        NetworkAddress::PointerAddress(_) => Ok(DataTypes::Pointer),
        NetworkAddress::ScratchpadAddress(_) => Ok(DataTypes::Scratchpad),
        NetworkAddress::PeerId(_) | NetworkAddress::RecordKey(_) => {
            Err(Error::GetError(GetError::Configuration(
                "Cannot determine data type for PeerId or RecordKey addresses".to_string(),
            )))
        }
    }
}

/// Process tasks with maximum concurrency (batch processing).
///
/// This function takes an iterator of futures and processes them in batches of the specified size,
/// ensuring that only a maximum number of tasks run concurrently at any time.
pub async fn process_tasks_with_max_concurrency<I, R>(tasks: I, batch_size: usize) -> Vec<R>
where
    I: IntoIterator,
    I::Item: futures::Future<Output = R>,
{
    let mut futures = FuturesUnordered::new();
    let mut results = Vec::new();
    let mut task_iter = tasks.into_iter();

    // Fill the initial batch
    for _ in 0..batch_size {
        if let Some(task) = task_iter.next() {
            futures.push(task);
        } else {
            break;
        }
    }

    // Process tasks in batches
    while !futures.is_empty() {
        if let Some(result) = futures.next().await {
            results.push(result);

            // Add the next task if available
            if let Some(task) = task_iter.next() {
                futures.push(task);
            }
        }
    }

    results
}

// chunk_cache facility to cach chunks during upload or download.
// which allows continue from the last broken point.
pub(crate) mod chunk_cache {
    use ant_protocol::storage::{Chunk, ChunkAddress};

    use bytes::Bytes;

    use std::fs;
    use std::path::PathBuf;
    use tracing::{debug, error, warn};

    const CHUNK_CACHE_FOLDER: &str = "chunk_cache";

    #[derive(Debug, thiserror::Error)]
    pub enum ChunkCacheError {
        #[error("IO error: {0}")]
        Io(#[from] std::io::Error),
        #[error("Failed to create cache directory: {0}")]
        DirectoryCreation(String),
    }

    /// Get the default chunk cache directory for the Autonomi client
    pub fn default_cache_dir() -> Result<PathBuf, ChunkCacheError> {
        let mut cache_dir = dirs_next::data_dir().ok_or_else(|| {
            ChunkCacheError::DirectoryCreation(
                "Failed to obtain data dir, your OS might not be supported.".to_string(),
            )
        })?;
        cache_dir.push("autonomi");
        cache_dir.push("client");
        cache_dir.push(CHUNK_CACHE_FOLDER);
        Ok(cache_dir)
    }

    /// Get the file path for a cached chunk
    fn chunk_file_path(cache_dir: PathBuf, chunk_addr: &ChunkAddress) -> PathBuf {
        let chunk_hash = hex::encode(chunk_addr.xorname().0);
        cache_dir.join(format!("{chunk_hash}.chunk"))
    }

    /// Store a chunk in the cache
    pub fn store_chunk(
        cache_dir: PathBuf,
        chunk_addr: &ChunkAddress,
        chunk: &Chunk,
    ) -> Result<(), ChunkCacheError> {
        // Create the cache directory if it doesn't exist
        if !cache_dir.exists() {
            fs::create_dir_all(&cache_dir).map_err(|e| {
                ChunkCacheError::DirectoryCreation(format!(
                    "Failed to create cache directory {}: {}",
                    cache_dir.display(),
                    e
                ))
            })?;
        }

        let chunk_file_path = chunk_file_path(cache_dir, chunk_addr);

        // Write chunk data to file
        fs::write(&chunk_file_path, chunk.value())?;

        debug!(
            "Cached chunk {} at {}",
            chunk_addr.to_hex(),
            chunk_file_path.display()
        );
        Ok(())
    }

    /// Load a cached chunk
    pub fn load_chunk(
        cache_dir: PathBuf,
        chunk_addr: &ChunkAddress,
    ) -> Result<Option<Chunk>, ChunkCacheError> {
        let chunk_file_path = chunk_file_path(cache_dir, chunk_addr);

        if !chunk_file_path.exists() {
            return Ok(None);
        }

        match fs::read(&chunk_file_path) {
            Ok(data) => {
                let chunk = Chunk::new(Bytes::from(data));
                debug!(
                    "Loaded cached chunk {} from {}",
                    chunk_addr.to_hex(),
                    chunk_file_path.display()
                );
                Ok(Some(chunk))
            }
            Err(e) => {
                warn!("Failed to read cached chunk {}: {}", chunk_addr.to_hex(), e);
                Ok(None)
            }
        }
    }

    /// Delete multiple chunks from the cache
    pub fn delete_chunks(
        cache_dir: PathBuf,
        chunk_addrs: &[(usize, ChunkAddress)],
    ) -> Result<(), ChunkCacheError> {
        for (_idx, chunk_addr) in chunk_addrs {
            let chunk_file_path = chunk_file_path(cache_dir.clone(), chunk_addr);
            if chunk_file_path.exists() {
                if let Err(e) = fs::remove_file(&chunk_file_path) {
                    warn!(
                        "Failed to delete cached chunk {}: {}",
                        chunk_addr.to_hex(),
                        e
                    );
                } else {
                    debug!(
                        "Deleted cached chunk {} at {}",
                        chunk_addr.to_hex(),
                        chunk_file_path.display()
                    );
                }
            }
        }
        Ok(())
    }
}
