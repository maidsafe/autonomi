// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::client::data_types::chunk::ChunkAddress;
use xor_name::XorName;

use crate::Bytes;
use crate::Client;
use crate::client::GetError;

type ChunkFetcher =
    Box<dyn Fn(&[(usize, XorName)]) -> self_encryption::Result<Vec<(usize, Bytes)>> + Send + Sync>;

pub struct DataStream {
    streaming_decrypt: self_encryption::DecryptionStream<ChunkFetcher>,
    finished: bool,
}

impl DataStream {
    pub(crate) fn new(client: Client, datamap: self_encryption::DataMap) -> Result<Self, GetError> {
        let client_clone = client.clone();

        // Create the chunk fetcher function that the streaming decrypt will use
        let chunk_fetcher: ChunkFetcher = Box::new(move |chunk_names: &[(usize, XorName)]| -> self_encryption::Result<Vec<(usize, Bytes)>> {
            let chunk_addresses: Vec<(usize, ChunkAddress)> = chunk_names
                .iter()
                .map(|(i, name)| (*i, ChunkAddress::new(*name)))
                .collect();

            // Use tokio::task::block_in_place to handle async in sync context
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    client_clone
                        .fetch_chunks_parallel(&chunk_addresses, chunk_names.len())
                        .await
                })
            })
        });

        // Create the streaming decrypt iterator
        let streaming_decrypt = self_encryption::streaming_decrypt(&datamap, chunk_fetcher)
            .map_err(|e| GetError::Decryption(crate::self_encryption::Error::SelfEncryption(e)))?;

        Ok(Self {
            streaming_decrypt,
            finished: false,
        })
    }
}

impl Iterator for DataStream {
    type Item = Result<Bytes, GetError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        // Get the next chunk from the streaming decrypt iterator
        match self.streaming_decrypt.next() {
            Some(Ok(chunk_bytes)) => {
                // Successfully got a decrypted chunk
                Some(Ok(chunk_bytes))
            }
            Some(Err(e)) => {
                // Error during decryption
                self.finished = true;
                Some(Err(GetError::Decryption(
                    crate::self_encryption::Error::SelfEncryption(e),
                )))
            }
            None => {
                // Stream is exhausted
                self.finished = true;
                None
            }
        }
    }
}

mod tests {
    #[tokio::test]
    async fn test_data_stream_vs_data_get() {
        use std::collections::HashMap;

        // Create test data - large enough to be split into multiple chunks
        let test_data = crate::Bytes::from(vec![42u8; 1_000_000]); // 1MB of data

        // Encrypt the data to get a data map and chunks
        let (data_map_chunk, chunks) = crate::self_encryption::encrypt(test_data.clone())
            .expect("Failed to encrypt test data");

        // Create a mock storage for chunks
        let mut chunk_storage = HashMap::new();
        for chunk in &chunks {
            let hash = xor_name::XorName::from_content(&chunk.value);
            chunk_storage.insert(hash, chunk.clone());
        }

        // Also store the data map chunk itself (needed for recursive data maps)
        let data_map_hash = xor_name::XorName::from_content(&data_map_chunk.value);
        chunk_storage.insert(data_map_hash, data_map_chunk.clone());

        let data_map_chunk = crate::chunk::DataMapChunk(data_map_chunk);

        // Restore the data map
        let restored_data_map: self_encryption::DataMap =
            rmp_serde::from_slice(&data_map_chunk.0.value).expect("Failed to deserialize data map");

        // Test 1: Use self_encryption::decrypt (simulate data_get)
        let encrypted_chunks: Vec<_> = restored_data_map
            .infos()
            .iter()
            .map(|info| {
                let chunk_data = chunk_storage
                    .get(&info.dst_hash)
                    .expect("Chunk not found in storage");
                self_encryption::EncryptedChunk {
                    content: chunk_data.value.clone(),
                }
            })
            .collect();

        let data_from_get = self_encryption::decrypt(&restored_data_map, &encrypted_chunks)
            .expect("Failed to decrypt with data_get method");

        // Test 2: Use streaming decrypt
        let chunk_fetcher = |chunk_names: &[(usize, xor_name::XorName)]| -> self_encryption::Result<Vec<(usize, crate::Bytes)>> {
            let mut results = Vec::new();
            for (i, hash) in chunk_names {
                let chunk = chunk_storage.get(hash)
                    .ok_or_else(|| self_encryption::Error::Generic("Chunk not found".to_string()))?;
                results.push((*i, chunk.value.clone()));
            }
            Ok(results)
        };

        let streaming_decrypt =
            self_encryption::streaming_decrypt(&restored_data_map, chunk_fetcher)
                .expect("Failed to create streaming decrypt");

        // Collect all data from stream
        let mut data_from_stream = Vec::new();
        for chunk_result in streaming_decrypt {
            let chunk = chunk_result.expect("Failed to get chunk from stream");
            data_from_stream.extend_from_slice(&chunk);
        }
        let data_from_stream = crate::Bytes::from(data_from_stream);

        // Verify both methods return the same data
        assert_eq!(
            data_from_get.len(),
            test_data.len(),
            "data_get length mismatch"
        );
        assert_eq!(
            data_from_stream.len(),
            test_data.len(),
            "data_stream length mismatch"
        );
        assert_eq!(data_from_get, test_data, "data_get content mismatch");
        assert_eq!(data_from_stream, test_data, "data_stream content mismatch");
        assert_eq!(
            data_from_get, data_from_stream,
            "data_get and data_stream results don't match"
        );
    }
}
