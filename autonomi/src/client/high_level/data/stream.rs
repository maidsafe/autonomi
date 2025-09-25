// Copyright 2025 MaidSafe.net limited.
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

        Ok(Self { streaming_decrypt })
    }

    /// Returns the original data size
    pub fn data_size(&self) -> usize {
        self.streaming_decrypt.file_size()
    }

    /// Decrypts and returns a specific byte range from the encrypted data.
    ///
    /// This method provides random access to any portion of the encrypted file
    /// without requiring sequential iteration through all preceding chunks.
    ///
    /// # Arguments
    ///
    /// * `start` - The starting byte position (inclusive)
    /// * `len` - The number of bytes to read
    ///
    /// # Returns
    ///
    /// * `Result<Bytes>` - The decrypted range of data or an error if chunks are missing/corrupted
    pub fn get_range(&self, start: usize, len: usize) -> Result<Bytes, GetError> {
        self.streaming_decrypt
            .get_range(start, len)
            .map_err(|e| GetError::Decryption(crate::self_encryption::Error::SelfEncryption(e)))
    }

    /// Convenience method to get a range using Range syntax.
    pub fn range(&self, range: std::ops::Range<usize>) -> Result<Bytes, GetError> {
        self.streaming_decrypt
            .range(range)
            .map_err(|e| GetError::Decryption(crate::self_encryption::Error::SelfEncryption(e)))
    }

    /// Convenience method to get a range from a starting position to the end of the file.
    pub fn range_from(&self, start: usize) -> Result<Bytes, GetError> {
        self.streaming_decrypt
            .range_from(start)
            .map_err(|e| GetError::Decryption(crate::self_encryption::Error::SelfEncryption(e)))
    }

    /// Convenience method to get a range from the beginning of the file to an end position.
    pub fn range_to(&self, end: usize) -> Result<Bytes, GetError> {
        self.streaming_decrypt
            .range_to(end)
            .map_err(|e| GetError::Decryption(crate::self_encryption::Error::SelfEncryption(e)))
    }

    /// Convenience method to get the entire file content.
    pub fn range_full(&self) -> Result<Bytes, GetError> {
        self.streaming_decrypt
            .range_full()
            .map_err(|e| GetError::Decryption(crate::self_encryption::Error::SelfEncryption(e)))
    }

    /// Convenience method to get an inclusive range.
    pub fn range_inclusive(&self, start: usize, end: usize) -> Result<Bytes, GetError> {
        self.streaming_decrypt
            .range_inclusive(start, end)
            .map_err(|e| GetError::Decryption(crate::self_encryption::Error::SelfEncryption(e)))
    }
}

impl Iterator for DataStream {
    type Item = Result<Bytes, GetError>;

    fn next(&mut self) -> Option<Self::Item> {
        // Get the next chunk from the streaming decrypt iterator
        match self.streaming_decrypt.next() {
            Some(Ok(chunk_bytes)) => {
                // Successfully got a decrypted chunk
                Some(Ok(chunk_bytes))
            }
            Some(Err(e)) => {
                // Error during decryption
                Some(Err(GetError::Decryption(
                    crate::self_encryption::Error::SelfEncryption(e),
                )))
            }
            None => {
                // Stream is exhausted
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    #[tokio::test]
    async fn test_data_stream_range_access() {
        use std::collections::HashMap;

        // Create test data - large enough to be split into multiple chunks
        let test_data = crate::Bytes::from(vec![42u8; 100_000]); // 100KB of data

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

        // Create chunk fetcher function that mimics the DataStream's behavior
        let chunk_fetcher: ChunkFetcher = Box::new(move |chunk_names: &[(usize, xor_name::XorName)]| -> self_encryption::Result<Vec<(usize, crate::Bytes)>> {
            let mut results = Vec::new();
            for (i, hash) in chunk_names {
                let chunk = chunk_storage.get(hash)
                    .ok_or_else(|| self_encryption::Error::Generic("Chunk not found".to_string()))?;
                results.push((*i, chunk.value.clone()));
            }
            Ok(results)
        });

        // Create streaming decrypt directly (same as DataStream would do internally)
        let streaming_decrypt =
            self_encryption::streaming_decrypt(&restored_data_map, chunk_fetcher)
                .expect("Failed to create streaming decrypt");

        // Create DataStream with our mocked streaming_decrypt
        let data_stream = DataStream { streaming_decrypt };

        // Test data_size method
        assert_eq!(data_stream.data_size(), test_data.len());

        // Test get_range method
        let range_data = data_stream.get_range(1000, 5000).unwrap();
        assert_eq!(range_data.len(), 5000);
        assert_eq!(range_data.as_ref(), &test_data[1000..6000]);

        // Test range method with Range syntax
        let range_data2 = data_stream.range(1000..6000).unwrap();
        assert_eq!(range_data2, range_data);

        // Test range_from method
        let from_data = data_stream.range_from(95000).unwrap();
        assert_eq!(from_data.len(), 5000);
        assert_eq!(from_data.as_ref(), &test_data[95000..]);

        // Test range_to method
        let to_data = data_stream.range_to(5000).unwrap();
        assert_eq!(to_data.len(), 5000);
        assert_eq!(to_data.as_ref(), &test_data[..5000]);

        // Test range_full method
        let full_data = data_stream.range_full().unwrap();
        assert_eq!(full_data.len(), test_data.len());
        assert_eq!(full_data.as_ref(), &test_data[..]);

        // Test range_inclusive method
        let inclusive_data = data_stream.range_inclusive(1000, 1999).unwrap();
        assert_eq!(inclusive_data.len(), 1000); // 1000 to 1999 inclusive = 1000 bytes
        assert_eq!(inclusive_data.as_ref(), &test_data[1000..2000]);
    }

    #[tokio::test]
    async fn test_data_stream_range_edge_cases() {
        use std::collections::HashMap;

        // Create smaller test data for edge case testing
        let test_data = crate::Bytes::from((0..=255u8).cycle().take(5000).collect::<Vec<u8>>());

        // Encrypt the data
        let (data_map_chunk, chunks) = crate::self_encryption::encrypt(test_data.clone())
            .expect("Failed to encrypt test data");

        // Create mock storage
        let mut chunk_storage = HashMap::new();
        for chunk in &chunks {
            let hash = xor_name::XorName::from_content(&chunk.value);
            chunk_storage.insert(hash, chunk.clone());
        }

        let data_map_hash = xor_name::XorName::from_content(&data_map_chunk.value);
        chunk_storage.insert(data_map_hash, data_map_chunk.clone());

        let data_map_chunk = crate::chunk::DataMapChunk(data_map_chunk);
        let restored_data_map: self_encryption::DataMap =
            rmp_serde::from_slice(&data_map_chunk.0.value).expect("Failed to deserialize data map");

        // Create chunk fetcher function that mimics the DataStream's behavior
        let chunk_fetcher: ChunkFetcher = Box::new(move |chunk_names: &[(usize, xor_name::XorName)]| -> self_encryption::Result<Vec<(usize, crate::Bytes)>> {
            let mut results = Vec::new();
            for (i, hash) in chunk_names {
                let chunk = chunk_storage.get(hash)
                    .ok_or_else(|| self_encryption::Error::Generic("Chunk not found".to_string()))?;
                results.push((*i, chunk.value.clone()));
            }
            Ok(results)
        });

        // Create streaming decrypt directly (same as DataStream would do internally)
        let streaming_decrypt =
            self_encryption::streaming_decrypt(&restored_data_map, chunk_fetcher)
                .expect("Failed to create streaming decrypt");

        // Create DataStream with our mocked streaming_decrypt
        let data_stream = DataStream { streaming_decrypt };

        // Test range beyond file size
        let beyond_range = data_stream.get_range(10000, 1000).unwrap();
        assert_eq!(beyond_range.len(), 0);

        // Test range starting at file size
        let at_end = data_stream.get_range(5000, 100).unwrap();
        assert_eq!(at_end.len(), 0);

        // Test range that partially exceeds file size
        let partial_exceed = data_stream.get_range(4800, 400).unwrap();
        assert_eq!(partial_exceed.len(), 200); // Only 200 bytes available from position 4800
        assert_eq!(partial_exceed.as_ref(), &test_data[4800..]);

        // Test zero-length range
        let zero_len = data_stream.get_range(2500, 0).unwrap();
        assert_eq!(zero_len.len(), 0);

        // Test range at start of file
        let at_start = data_stream.get_range(0, 100).unwrap();
        assert_eq!(at_start.len(), 100);
        assert_eq!(at_start.as_ref(), &test_data[0..100]);
    }

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
