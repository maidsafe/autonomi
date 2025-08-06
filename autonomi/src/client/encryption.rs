// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::Client;
use crate::client::{data_types::chunk::DataMapChunk, utils::process_tasks_with_max_concurrency};
use crate::files::{Metadata, get_relative_file_path_from_abs_file_and_folder_path};
use crate::self_encryption::encrypt;
use ant_protocol::storage::Chunk;
use bytes::Bytes;
use std::path::PathBuf;
use std::time::Instant;
use tokio::sync::mpsc;

use super::data::DataAddress;
use super::files::FILE_ENCRYPT_BATCH_SIZE;

enum EncryptionState {
    InMemory(Vec<Chunk>, DataMapChunk),
    #[allow(dead_code)]
    StreamInProgress(mpsc::Receiver<Chunk>),
    #[allow(dead_code)]
    StreamDone(DataMapChunk),
}

pub(crate) struct EncryptionStream {
    pub file_path: String,
    pub relative_path: PathBuf,
    pub metadata: Metadata,
    pub is_public: bool,
    state: EncryptionState,
}

impl EncryptionStream {
    pub fn total_chunks(&self) -> usize {
        match &self.state {
            EncryptionState::InMemory(chunks, _) => chunks.len(),
            EncryptionState::StreamInProgress(_) => 42, // NB TODO: implement
            EncryptionState::StreamDone(_datamap) => 42, // NB TODO: implement
        }
    }

    pub fn next_batch(&mut self, batch_size: usize) -> Option<Vec<Chunk>> {
        if batch_size == 0 {
            return Some(vec![]);
        }

        match &mut self.state {
            EncryptionState::InMemory(chunks, _) => {
                let batch: Vec<Chunk> = chunks
                    .drain(0..std::cmp::min(batch_size, chunks.len()))
                    .collect();
                if batch.is_empty() {
                    return None;
                }
                Some(batch)
            }
            EncryptionState::StreamInProgress(_receiver) => todo!(),
            EncryptionState::StreamDone(_) => todo!(),
        }
    }

    pub fn data_map_chunk(&self) -> Option<DataMapChunk> {
        match &self.state {
            EncryptionState::InMemory(_, data_map_chunk) => Some(data_map_chunk.clone()),
            EncryptionState::StreamInProgress(_) => None,
            EncryptionState::StreamDone(data_map_chunk) => Some(data_map_chunk.clone()),
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

    // pub fn new_stream_from_file(
    //     file_path: String,
    //     relative_path: PathBuf,
    //     metadata: Metadata,
    //     is_public: bool,
    // ) -> Result<Self, String> {
    //     let (sender, receiver) = mpsc::channel(100);

    //     self_encryption::streaming_encrypt_from_file(file_path, |(xorname, bytes)| {
    //         let chunk = Chunk::new(xorname, bytes);
    //         sender.send(chunk).await.map_err(|err| format!("Error sending chunk: {err:?}"))?;
    //         Ok(())
    //     })
    // }
}

impl Client {
    /// Encrypts all files in a directory and returns the encryption results (common logic)
    pub(crate) async fn encrypt_directory_files_in_memory(
        &self,
        dir_path: PathBuf,
        is_public: bool,
    ) -> Result<Vec<Result<EncryptionStream, String>>, walkdir::Error> {
        let mut encryption_tasks = vec![];

        for entry in walkdir::WalkDir::new(&dir_path) {
            let entry = entry?;

            if entry.file_type().is_dir() {
                continue;
            }

            let dir_path = dir_path.clone();

            encryption_tasks.push(async move {
                let file_path = entry.path().to_path_buf();

                info!("Encrypting file: {file_path:?}..");
                #[cfg(feature = "loud")]
                println!("Encrypting file: {file_path:?}..");

                let data = tokio::fs::read(&file_path)
                    .await
                    .map_err(|err| format!("Could not read file {file_path:?}: {err:?}"))?;
                let data = Bytes::from(data);

                if data.len() < 3 {
                    let err_msg =
                        format!("Skipping file {file_path:?}, as it is smaller than 3 bytes");
                    return Err(err_msg);
                }

                let metadata =
                    crate::client::high_level::files::fs_public::metadata_from_entry(&entry);
                let relative_path =
                    get_relative_file_path_from_abs_file_and_folder_path(&file_path, &dir_path);

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
            });
        }

        let encryption_results =
            process_tasks_with_max_concurrency(encryption_tasks, *FILE_ENCRYPT_BATCH_SIZE).await;

        Ok(encryption_results)
    }
}
