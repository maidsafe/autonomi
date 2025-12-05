// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::client::data_types::chunk::DataMapChunk;
use crate::files::{Metadata, get_relative_file_path_from_abs_file_and_folder_path};
use crate::self_encryption::encrypt;
use ant_protocol::storage::Chunk;
use bytes::Bytes;
use self_encryption::MAX_CHUNK_SIZE;
use std::path::PathBuf;
use std::time::Instant;
use tokio::sync::oneshot;

use crate::client::config::{FILE_ENCRYPT_BATCH_SIZE, IN_MEMORY_ENCRYPTION_MAX_SIZE};
use crate::client::data::DataAddress;
use crate::utils::process_tasks_with_max_concurrency;

const STREAM_CHUNK_CHANNEL_CAPACITY: usize = 10;

pub enum EncryptionState {
    InMemory(Vec<Chunk>, DataMapChunk),
    StreamInProgress(StreamProgressState),
    /// StreamDone(DataMapChunk, total_chunk_count)
    StreamDone((DataMapChunk, usize)),
}

pub struct EncryptionStream {
    pub file_path: String,
    pub relative_path: PathBuf,
    pub metadata: Metadata,
    pub is_public: bool,
    state: EncryptionState,
}

pub struct StreamProgressState {
    /// Receiver for chunks
    chunk_receiver: std::sync::mpsc::Receiver<Chunk>,
    /// Receiver for the datamap once the stream is done
    datamap_receiver: oneshot::Receiver<DataMapChunk>,
    /// Number of chunks received so far
    chunk_count: usize,
    /// Total number of chunks estimated to be received
    total_estimated_chunks: usize,
}

/// Creates a data iterator from a file path that reads the file in chunks
fn create_file_data_iterator(file_path: &str) -> std::io::Result<impl Iterator<Item = Bytes>> {
    use std::fs::File;
    use std::io::{BufReader, Read};

    let file = File::open(file_path)?;
    let mut reader = BufReader::new(file);

    Ok(std::iter::from_fn(move || {
        let mut buffer = vec![0u8; 8192];
        match reader.read(&mut buffer) {
            Ok(0) => None,
            Ok(n) => {
                buffer.truncate(n);
                Some(Bytes::from(buffer))
            }
            Err(e) => {
                error!("Error reading file: {e}");
                None
            }
        }
    }))
}

impl EncryptionStream {
    pub fn total_chunks(&self) -> usize {
        match &self.state {
            EncryptionState::InMemory(chunks, _) => chunks.len(),
            EncryptionState::StreamInProgress(state) => state.total_estimated_chunks,
            EncryptionState::StreamDone((_, total_chunk_count)) => *total_chunk_count,
        }
    }

    pub fn next_batch(&mut self, batch_size: usize) -> Option<Vec<Chunk>> {
        if batch_size == 0 {
            return Some(vec![]);
        }

        let mut state_change: Option<EncryptionState> = None;

        let result = match &mut self.state {
            EncryptionState::InMemory(chunks, _) => {
                let batch: Vec<Chunk> = chunks
                    .drain(0..std::cmp::min(batch_size, chunks.len()))
                    .collect();
                if batch.is_empty() {
                    return None;
                }
                Some(batch)
            }
            EncryptionState::StreamInProgress(progress) => {
                let chunk_receiver = &mut progress.chunk_receiver;
                let datamap_receiver = &mut progress.datamap_receiver;
                let mut batch = Vec::with_capacity(batch_size);

                // Try to receive chunks up to batch_size
                for _ in 0..batch_size {
                    match chunk_receiver.recv() {
                        Ok(chunk) => batch.push(chunk),
                        Err(_) => {
                            // Chunk stream is done, check if we have the datamap
                            match datamap_receiver.try_recv() {
                                Ok(datamap_chunk) => {
                                    // The datamap_chunk shall be uploaded if as public
                                    if self.is_public {
                                        batch.push(datamap_chunk.0.clone());
                                        progress.chunk_count += 1;
                                    }

                                    // Transition to StreamDone state
                                    state_change = Some(EncryptionState::StreamDone((
                                        datamap_chunk,
                                        progress.chunk_count,
                                    )));
                                }
                                Err(oneshot::error::TryRecvError::Empty) => {
                                    error!("DataMap not available when chunk receiver was closed");
                                }
                                Err(oneshot::error::TryRecvError::Closed) => {
                                    error!("DataMap sender was dropped without sending data");
                                }
                            }
                            break;
                        }
                    }
                }

                progress.chunk_count += batch.len();
                if batch.is_empty() { None } else { Some(batch) }
            }
            EncryptionState::StreamDone(_) => None,
        };

        // Apply the state change if any
        if let Some(next_state) = state_change {
            self.state = next_state;
        }

        result
    }

    pub fn data_map_chunk(&self) -> Option<DataMapChunk> {
        match &self.state {
            EncryptionState::InMemory(_, data_map_chunk) => Some(data_map_chunk.clone()),
            EncryptionState::StreamInProgress(_) => None,
            EncryptionState::StreamDone((data_map_chunk, _)) => Some(data_map_chunk.clone()),
        }
    }

    /// Returns the data address of the file if the file is public and the stream is done.
    pub fn data_address(&self) -> Option<DataAddress> {
        let data_map_chunk = self.data_map_chunk()?;
        if self.is_public {
            let data_address = DataAddress::new(*data_map_chunk.0.address().xorname());
            Some(data_address)
        } else {
            None
        }
    }

    pub fn new_in_memory_with(
        file_path: String,
        relative_path: PathBuf,
        metadata: Metadata,
        is_public: bool,
        bytes: Bytes,
    ) -> Result<(Self, DataMapChunk), crate::self_encryption::Error> {
        let start = Instant::now();
        let (data_map_chunk, mut chunks) = encrypt(bytes)?;

        if is_public {
            chunks.push(data_map_chunk.clone());
        }

        let stream = EncryptionStream {
            file_path,
            relative_path,
            metadata,
            is_public,
            state: EncryptionState::InMemory(chunks, data_map_chunk.clone().into()),
        };

        debug!("Encryption took: {:.2?}", start.elapsed());
        Ok((stream, DataMapChunk(data_map_chunk)))
    }

    pub fn new_in_memory(
        bytes: Bytes,
        is_public: bool,
    ) -> Result<(Self, DataMapChunk), crate::self_encryption::Error> {
        Self::new_in_memory_with(
            "".to_string(),
            Default::default(),
            Metadata::default(),
            is_public,
            bytes,
        )
    }

    pub fn new_stream_from_file(
        file_path: String,
        relative_path: PathBuf,
        metadata: Metadata,
        is_public: bool,
        file_size: usize,
    ) -> Result<Self, String> {
        let start = Instant::now();
        let (chunk_sender, chunk_receiver) =
            std::sync::mpsc::sync_channel(STREAM_CHUNK_CHANNEL_CAPACITY);
        let (datamap_sender, datamap_receiver) = oneshot::channel();
        let file_path_clone = file_path.clone();

        // Spawn a thread to handle streaming encryption
        std::thread::spawn(move || {
            // Create iterator that reads file in chunks
            let data_iter = match create_file_data_iterator(&file_path_clone) {
                Ok(iter) => iter,
                Err(e) => {
                    error!("Failed to open file {file_path_clone}: {e}");
                    return;
                }
            };

            // Use stream_encrypt API
            let mut stream = match self_encryption::stream_encrypt(file_size, data_iter) {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to create encryption stream for {file_path_clone}: {e}");
                    return;
                }
            };

            // Process chunks from the stream
            for chunk_result in stream.chunks() {
                match chunk_result {
                    Ok((_hash, content)) => {
                        let chunk = Chunk::new(content);
                        if let Err(e) = chunk_sender.send(chunk) {
                            error!("Error sending chunk for {file_path_clone}: {e}");
                            return;
                        }
                    }
                    Err(e) => {
                        error!("Error encrypting chunk for {file_path_clone}: {e}");
                        return;
                    }
                }
            }

            // Get the datamap after all chunks are processed
            match stream.datamap() {
                Some(datamap) => {
                    // Convert DataMap to bytes and create a chunk
                    let datamap_bytes = match rmp_serde::to_vec(datamap) {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            error!("Error serializing datamap for {file_path_clone}: {e}");
                            return;
                        }
                    };
                    let datamap_chunk = DataMapChunk(Chunk::new(Bytes::from(datamap_bytes)));
                    if let Err(e) = datamap_sender.send(datamap_chunk) {
                        error!("Error sending datamap for {file_path_clone}: {e:?}");
                    }
                }
                None => {
                    error!("DataMap not available after encryption for {file_path_clone}");
                }
            }

            // Close the chunk sender to signal completion
            drop(chunk_sender);
        });

        #[cfg(feature = "loud")]
        println!("Streaming encryption of {file_path} ...");
        info!("Streaming encryption of {file_path} ...");

        let stream = EncryptionStream {
            file_path,
            relative_path,
            metadata,
            is_public,
            state: EncryptionState::StreamInProgress(StreamProgressState {
                chunk_receiver,
                datamap_receiver,
                chunk_count: 0,
                total_estimated_chunks: std::cmp::max(3, file_size / MAX_CHUNK_SIZE),
            }),
        };

        debug!(
            "Started streaming encryption for file (size: {} bytes) in: {:.2?}",
            file_size,
            start.elapsed()
        );
        Ok(stream)
    }
}

/// Encrypts all files in a directory and returns the encryption results (common logic)
pub async fn encrypt_directory_files(
    dir_path: PathBuf,
    is_public: bool,
) -> Result<Vec<Result<EncryptionStream, String>>, walkdir::Error> {
    let mut encryption_tasks = vec![];

    for entry in walkdir::WalkDir::new(&dir_path) {
        let entry = entry?;

        if !entry.file_type().is_file() {
            // Skip directories and symbolic links
            continue;
        }

        let dir_path = dir_path.clone();

        encryption_tasks.push(async move {
            let metadata = crate::client::files::fs_public::metadata_from_entry(&entry);
            let file_path = entry.path().to_path_buf();
            let relative_path =
                get_relative_file_path_from_abs_file_and_folder_path(&file_path, &dir_path);
            let file_size = entry
                .metadata()
                .map_err(|err| format!("Error getting file size {file_path:?}: {err:?}"))?
                .len() as usize;
            encrypt_file(relative_path, file_path, file_size, metadata, is_public).await
        });
    }

    let encryption_results =
        process_tasks_with_max_concurrency(encryption_tasks, *FILE_ENCRYPT_BATCH_SIZE).await;

    Ok(encryption_results)
}

pub(crate) async fn encrypt_file(
    relative_path: PathBuf,
    file_path: PathBuf,
    file_size: usize,
    metadata: Metadata,
    is_public: bool,
) -> Result<EncryptionStream, String> {
    info!("Encrypting file: {file_path:?}..");
    #[cfg(feature = "loud")]
    println!("Encrypting file: {file_path:?}..");

    // choose encryption method
    if file_size > *IN_MEMORY_ENCRYPTION_MAX_SIZE {
        encrypt_file_in_stream(file_path, is_public, metadata, relative_path, file_size)
    } else {
        encrypt_file_in_memory(file_path, is_public, metadata, relative_path).await
    }
}

fn encrypt_file_in_stream(
    file_path: PathBuf,
    is_public: bool,
    metadata: Metadata,
    relative_path: PathBuf,
    file_size: usize,
) -> Result<EncryptionStream, String> {
    info!("Encrypting file in stream: {file_path:?}..");
    EncryptionStream::new_stream_from_file(
        file_path.to_string_lossy().to_string(),
        relative_path,
        metadata,
        is_public,
        file_size,
    )
}

async fn encrypt_file_in_memory(
    file_path: PathBuf,
    is_public: bool,
    metadata: Metadata,
    relative_path: PathBuf,
) -> Result<EncryptionStream, String> {
    info!("Encrypting file in memory: {file_path:?}..");
    let data = tokio::fs::read(&file_path)
        .await
        .map_err(|err| format!("Could not read file {file_path:?}: {err:?}"))?;
    let data = Bytes::from(data);

    if data.len() < 3 {
        let err_msg = format!("Cannot encrypt file {file_path:?}, as it is smaller than 3 bytes");
        return Err(err_msg);
    }

    let start = Instant::now();
    let (file_chunk_iterator, _data_map) = EncryptionStream::new_in_memory_with(
        file_path.to_string_lossy().to_string(),
        relative_path,
        metadata,
        is_public,
        data,
    )
    .map_err(|err| format!("Error encrypting file {file_path:?}: {err:?}"))?;

    debug!("Encryption of {file_path:?} took: {:.2?}", start.elapsed());

    Ok(file_chunk_iterator)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;
    use tokio::time::{Duration, sleep};

    #[tokio::test]
    async fn test_streaming_state_transitions() {
        // Create a temporary file
        let mut temp_file = NamedTempFile::new().unwrap();
        let test_data = b"Small test data";
        temp_file.write_all(test_data).unwrap();
        temp_file.flush().unwrap();

        let file_path = temp_file.path().to_string_lossy().to_string();
        let relative_path = PathBuf::from("small.txt");
        let metadata = Metadata::default();
        let is_public = false;
        let file_size = test_data.len();

        let mut stream = EncryptionStream::new_stream_from_file(
            file_path,
            relative_path,
            metadata,
            is_public,
            file_size,
        )
        .unwrap();

        // Should start in StreamInProgress
        assert!(matches!(stream.state, EncryptionState::StreamInProgress(_)));

        // Give some time for the background task to potentially complete
        // (though it won't actually work due to the todo!() placeholder)
        sleep(Duration::from_millis(10)).await;

        // we should expect 3 chunks
        let total_chunks = stream.total_chunks();
        assert_eq!(total_chunks, 3);

        // the datamap should not be available yet
        assert!(stream.data_map_chunk().is_none());

        // Try to get a batch - this should handle the streaming logic
        let batch = stream.next_batch(5);

        // We expect 3 chunks
        match batch {
            Some(chunks) => assert_eq!(chunks.len(), 3),
            None => panic!("No chunks available when we expected 3"),
        }

        // we should have no more chunks
        let next_batch = stream.next_batch(5);
        assert_eq!(next_batch, None);

        // State should be StreamDone
        assert!(matches!(stream.state, EncryptionState::StreamDone(_)));

        // we should have the datamap now
        let data_map_chunk = stream.data_map_chunk();
        assert!(data_map_chunk.is_some());
    }
}
