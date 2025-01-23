// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    client::{payment::Receipt, utils::process_tasks_with_max_concurrency, GetError, PutError},
    self_encryption::DataMapLevel,
    Client,
};
use ant_evm::ProofOfPayment;
use ant_networking::{GetRecordCfg, NetworkError, PutRecordCfg, ResponseQuorum, VerificationKind};
use ant_protocol::{
    messages::ChunkProof,
    storage::{try_deserialize_record, try_serialize_record, DataTypes, RecordHeader, RecordKind},
    NetworkAddress,
};
use bytes::Bytes;
use libp2p::kad::Record;
use rand::{thread_rng, Rng};
use self_encryption::{decrypt_full_set, DataMap, EncryptedChunk};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    hash::{DefaultHasher, Hash, Hasher},
    sync::LazyLock,
};

pub use ant_protocol::storage::{Chunk, ChunkAddress};

/// Number of retries to upload chunks.
pub(crate) const RETRY_ATTEMPTS: usize = 3;

/// Number of chunks to upload in parallel.
///
/// Can be overridden by the `CHUNK_UPLOAD_BATCH_SIZE` environment variable.
pub(crate) static CHUNK_UPLOAD_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    let batch_size = std::env::var("CHUNK_UPLOAD_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
                * 8,
        );
    info!("Chunk upload batch size: {}", batch_size);
    batch_size
});

/// Number of chunks to download in parallel.
///
/// Can be overridden by the `CHUNK_DOWNLOAD_BATCH_SIZE` environment variable.
pub static CHUNK_DOWNLOAD_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    let batch_size = std::env::var("CHUNK_DOWNLOAD_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
                * 8,
        );
    info!("Chunk download batch size: {}", batch_size);
    batch_size
});

/// Private data on the network can be accessed with this
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DataMapChunk(pub(crate) Chunk);

impl DataMapChunk {
    pub fn to_hex(&self) -> String {
        hex::encode(self.0.value())
    }

    pub fn from_hex(hex: &str) -> Result<Self, hex::FromHexError> {
        let data = hex::decode(hex)?;
        Ok(Self(Chunk::new(Bytes::from(data))))
    }

    /// Get a private address for [`DataMapChunk`]. Note that this is not a network address, it is only used for refering to private data client side.
    pub fn address(&self) -> String {
        hash_to_short_string(&self.to_hex())
    }
}

impl From<Chunk> for DataMapChunk {
    fn from(value: Chunk) -> Self {
        Self(value)
    }
}

fn hash_to_short_string(input: &str) -> String {
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    let hash_value = hasher.finish();
    hash_value.to_string()
}

impl Client {
    /// Get a chunk from the network.
    pub async fn chunk_get(&self, addr: ChunkAddress) -> Result<Chunk, GetError> {
        info!("Getting chunk: {addr:?}");

        let key = NetworkAddress::from_chunk_address(addr).to_record_key();
        debug!("Fetching chunk from network at: {key:?}");
        let get_cfg = GetRecordCfg {
            get_quorum: self.operation_config.chunk_operation_config.read_quorum,
            retry_strategy: self
                .operation_config
                .chunk_operation_config
                .read_retry_strategy,
            target_record: None,
            expected_holders: HashSet::new(),
        };

        let record = self
            .network
            .get_record_from_network(key, &get_cfg)
            .await
            .inspect_err(|err| error!("Error fetching chunk: {err:?}"))?;
        let header = RecordHeader::from_record(&record)?;

        if let Ok(true) = RecordHeader::is_record_of_type_chunk(&record) {
            let chunk: Chunk = try_deserialize_record(&record)?;
            Ok(chunk)
        } else {
            error!(
                "Record kind mismatch: expected Chunk, got {:?}",
                header.kind
            );
            Err(NetworkError::RecordKindMismatch(RecordKind::DataOnly(DataTypes::Chunk)).into())
        }
    }

    /// Upload chunks and retry failed uploads up to `RETRY_ATTEMPTS` times.
    pub async fn upload_chunks_with_retries<'a>(
        &self,
        mut chunks: Vec<&'a Chunk>,
        receipt: &Receipt,
    ) -> Vec<(&'a Chunk, PutError)> {
        let mut current_attempt: usize = 1;

        loop {
            let mut upload_tasks = vec![];
            for chunk in chunks {
                let self_clone = self.clone();
                let address = *chunk.address();

                let Some((proof, _)) = receipt.get(chunk.name()) else {
                    debug!("Chunk at {address:?} was already paid for so skipping");
                    continue;
                };

                upload_tasks.push(async move {
                    self_clone
                        .chunk_upload_with_payment(chunk, proof.clone())
                        .await
                        .inspect_err(|err| error!("Error uploading chunk {address:?} :{err:?}"))
                        // Return chunk reference too, to re-use it next attempt/iteration
                        .map_err(|err| (chunk, err))
                });
            }
            let uploads =
                process_tasks_with_max_concurrency(upload_tasks, *CHUNK_UPLOAD_BATCH_SIZE).await;

            // Check for errors.
            let total_uploads = uploads.len();
            let uploads_failed: Vec<_> = uploads.into_iter().filter_map(|up| up.err()).collect();
            info!(
                "Uploaded {} chunks out of {total_uploads}",
                total_uploads - uploads_failed.len()
            );

            // All uploads succeeded.
            if uploads_failed.is_empty() {
                return vec![];
            }

            // Max retries reached.
            if current_attempt > RETRY_ATTEMPTS {
                return uploads_failed;
            }

            tracing::info!(
                "Retrying putting {} failed chunks (attempt {current_attempt}/3)",
                uploads_failed.len()
            );

            // Re-iterate over the failed chunks
            chunks = uploads_failed.into_iter().map(|(chunk, _)| chunk).collect();
            current_attempt += 1;
        }
    }

    pub(crate) async fn chunk_upload_with_payment(
        &self,
        chunk: &Chunk,
        payment: ProofOfPayment,
    ) -> Result<ChunkAddress, PutError> {
        let storing_nodes = payment.payees();

        if storing_nodes.is_empty() {
            return Err(PutError::PayeesMissing);
        }

        debug!("Storing chunk: {chunk:?} to {:?}", storing_nodes);

        let key = chunk.network_address().to_record_key();

        let record_kind = RecordKind::DataWithPayment(DataTypes::Chunk);
        let record = Record {
            key: key.clone(),
            value: try_serialize_record(&(payment, chunk.clone()), record_kind)
                .map_err(|e| {
                    PutError::Serialization(format!(
                        "Failed to serialize chunk with payment: {e:?}"
                    ))
                })?
                .to_vec(),
            publisher: None,
            expires: None,
        };

        let verification = {
            let verification_cfg = GetRecordCfg {
                get_quorum: self
                    .operation_config
                    .chunk_operation_config
                    .verification_quorum,
                retry_strategy: self
                    .operation_config
                    .chunk_operation_config
                    .verification_retry_strategy,
                target_record: None,
                expected_holders: Default::default(),
            };

            let stored_on_node =
                try_serialize_record(&chunk, RecordKind::DataOnly(DataTypes::Chunk))
                    .map_err(|e| {
                        PutError::Serialization(format!("Failed to serialize chunk: {e:?}"))
                    })?
                    .to_vec();
            let random_nonce = thread_rng().gen::<u64>();
            let expected_proof = ChunkProof::new(&stored_on_node, random_nonce);

            Some((
                VerificationKind::ChunkProof {
                    expected_proof,
                    nonce: random_nonce,
                },
                verification_cfg,
            ))
        };

        let put_cfg = PutRecordCfg {
            put_quorum: ResponseQuorum::One,
            retry_strategy: self
                .operation_config
                .chunk_operation_config
                .write_retry_strategy,
            use_put_record_to: Some(storing_nodes.clone()),
            verification,
        };
        self.network.put_record(record, &put_cfg).await?;
        debug!("Successfully stored chunk: {chunk:?} to {storing_nodes:?}");
        Ok(*chunk.address())
    }

    /// Unpack a wrapped data map and fetch all bytes using self-encryption.
    pub(crate) async fn fetch_from_data_map_chunk(
        &self,
        data_map_bytes: &Bytes,
    ) -> Result<Bytes, GetError> {
        let mut data_map_level: DataMapLevel = rmp_serde::from_slice(data_map_bytes)
            .map_err(GetError::InvalidDataMap)
            .inspect_err(|err| error!("Error deserializing data map: {err:?}"))?;

        loop {
            let data_map = match &data_map_level {
                DataMapLevel::First(map) => map,
                DataMapLevel::Additional(map) => map,
            };
            let data = self.fetch_from_data_map(data_map).await?;

            match &data_map_level {
                DataMapLevel::First(_) => break Ok(data),
                DataMapLevel::Additional(_) => {
                    data_map_level = rmp_serde::from_slice(&data).map_err(|err| {
                        error!("Error deserializing data map: {err:?}");
                        GetError::InvalidDataMap(err)
                    })?;
                    continue;
                }
            };
        }
    }

    /// Fetch and decrypt all chunks in the data map.
    pub(crate) async fn fetch_from_data_map(&self, data_map: &DataMap) -> Result<Bytes, GetError> {
        debug!("Fetching encrypted data chunks from data map {data_map:?}");
        let mut download_tasks = vec![];
        for info in data_map.infos() {
            download_tasks.push(async move {
                match self
                    .chunk_get(ChunkAddress::new(info.dst_hash))
                    .await
                    .inspect_err(|err| {
                        error!(
                            "Error fetching chunk {:?}: {err:?}",
                            ChunkAddress::new(info.dst_hash)
                        )
                    }) {
                    Ok(chunk) => Ok(EncryptedChunk {
                        index: info.index,
                        content: chunk.value,
                    }),
                    Err(err) => {
                        error!(
                            "Error fetching chunk {:?}: {err:?}",
                            ChunkAddress::new(info.dst_hash)
                        );
                        Err(err)
                    }
                }
            });
        }
        debug!("Successfully fetched all the encrypted chunks");
        let encrypted_chunks =
            process_tasks_with_max_concurrency(download_tasks, *CHUNK_DOWNLOAD_BATCH_SIZE)
                .await
                .into_iter()
                .collect::<Result<Vec<EncryptedChunk>, GetError>>()?;

        let data = decrypt_full_set(data_map, &encrypted_chunks).map_err(|e| {
            error!("Error decrypting encrypted_chunks: {e:?}");
            GetError::Decryption(crate::self_encryption::Error::SelfEncryption(e))
        })?;
        debug!("Successfully decrypted all the chunks");
        Ok(data)
    }
}
