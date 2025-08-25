// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::Client;
use bytes::Bytes;
use std::path::PathBuf;
use std::time::Instant;

pub(crate) use autonomi_core::EncryptionStream;
pub use autonomi_core::client::upload::IN_MEMORY_ENCRYPTION_MAX_SIZE;

impl Client {
    /// Encrypts all files in a directory and returns the encryption results (common logic)
    pub(crate) async fn encrypt_directory_files_in_memory(
        &self,
        dir_path: PathBuf,
        is_public: bool,
    ) -> Result<Vec<Result<EncryptionStream, String>>, walkdir::Error> {
        use autonomi_core::process_tasks_with_max_concurrency;

        let mut encryption_tasks = vec![];

        for entry in walkdir::WalkDir::new(&dir_path) {
            let entry = entry?;

            if entry.file_type().is_dir() {
                continue;
            }

            encryption_tasks.push(async move {
                let file_path = entry.path().to_path_buf();
                info!("Encrypting file: {file_path:?}..");
                #[cfg(feature = "loud")]
                println!("Encrypting file: {file_path:?}..");

                let file_size = entry
                    .metadata()
                    .map_err(|err| format!("Error getting file size {file_path:?}: {err:?}"))?
                    .len() as usize;

                // choose encryption method
                if file_size > *IN_MEMORY_ENCRYPTION_MAX_SIZE {
                    encrypt_file_in_stream(file_path, is_public, file_size)
                } else {
                    encrypt_file_in_memory(file_path, is_public).await
                }
            });
        }

        let encryption_results = process_tasks_with_max_concurrency(encryption_tasks, 10).await;

        Ok(encryption_results)
    }
}

fn encrypt_file_in_stream(
    file_path: PathBuf,
    is_public: bool,
    file_size: usize,
) -> Result<EncryptionStream, String> {
    info!("Encrypting file in stream: {file_path:?}..");
    EncryptionStream::new_stream_from_file(file_path, is_public, file_size)
}

async fn encrypt_file_in_memory(
    file_path: PathBuf,
    is_public: bool,
) -> Result<EncryptionStream, String> {
    info!("Encrypting file in memory: {file_path:?}..");
    let data = tokio::fs::read(&file_path)
        .await
        .map_err(|err| format!("Could not read file {file_path:?}: {err:?}"))?;
    let data = Bytes::from(data);

    if data.len() < 3 {
        let err_msg = format!("Skipping file {file_path:?}, as it is smaller than 3 bytes");
        return Err(err_msg);
    }

    let start = Instant::now();
    let (file_chunk_iterator, _data_map) =
        EncryptionStream::new_in_memory(Some(file_path.clone()), data, is_public)
            .map_err(|err| format!("Error encrypting file {file_path:?}: {err:?}"))?;

    debug!("Encryption of {file_path:?} took: {:.2?}", start.elapsed());

    Ok(file_chunk_iterator)
}
