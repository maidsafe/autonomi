// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    client::{
        payment::{PaymentOption, Receipt},
        quote::CostError,
        utils::process_tasks_with_max_concurrency,
        ChunkBatchUploadState, GetError, PutError,
    },
    self_encryption::DataMapLevel,
    Client,
};
use ant_evm::{Amount, AttoTokens, ClientProofOfPayment};
pub use ant_protocol::storage::{Chunk, ChunkAddress};
use ant_protocol::{
    storage::{try_deserialize_record, try_serialize_record, DataTypes, RecordHeader, RecordKind},
    NetworkAddress,
};
use bytes::Bytes;
use libp2p::kad::Record;
use self_encryption::{decrypt_full_set, DataMap, EncryptedChunk};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    sync::LazyLock,
};

/// Number of chunks to upload in parallel.
///
/// Can be overridden by the `CHUNK_UPLOAD_BATCH_SIZE` environment variable.
pub(crate) static CHUNK_UPLOAD_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    let batch_size = std::env::var("CHUNK_UPLOAD_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
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
        .unwrap_or(1);
    info!("Chunk download batch size: {}", batch_size);
    batch_size
});

/// Private data on the network can be accessed with this
/// Uploading this data in a chunk makes it publicly accessible from the address of that Chunk
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DataMapChunk(pub(crate) Chunk);

impl DataMapChunk {
    /// Convert the chunk to a hex string.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0.value())
    }

    /// Convert a hex string to a [`DataMapChunk`].
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

impl std::fmt::Display for DataMapChunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.to_hex())
    }
}

impl std::fmt::Debug for DataMapChunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.to_hex())
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
    pub async fn chunk_get(&self, addr: &ChunkAddress) -> Result<Chunk, GetError> {
        info!("Getting chunk: {addr:?}");

        let key = NetworkAddress::from(*addr);

        debug!("Fetching chunk from network at: {key:?}");

        let record = self
            .network
            .get_record_with_retries(key, &self.config.chunks)
            .await
            .inspect_err(|err| error!("Error fetching chunk: {err:?}"))?
            .ok_or(GetError::RecordNotFound)?;

        let header = RecordHeader::from_record(&record)?;

        if let Ok(true) = RecordHeader::is_record_of_type_chunk(&record) {
            let chunk: Chunk = try_deserialize_record(&record)?;
            Ok(chunk)
        } else {
            error!(
                "Record kind mismatch: expected Chunk, got {:?}",
                header.kind
            );
            Err(GetError::RecordKindMismatch(RecordKind::DataOnly(
                DataTypes::Chunk,
            )))
        }
    }

    /// Manually upload a chunk to the network.
    /// It is recommended to use the [`Client::data_put`] method instead to upload data.
    pub async fn chunk_put(
        &self,
        chunk: &Chunk,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, ChunkAddress), PutError> {
        let address = chunk.network_address();

        if chunk.size() > Chunk::MAX_SIZE {
            return Err(PutError::Serialization(format!(
                "Chunk is too large: {} bytes, when max size is {}",
                chunk.size(),
                Chunk::MAX_SIZE
            )));
        }

        // pay for the chunk storage
        let xor_name = *chunk.name();
        debug!("Paying for chunk at address: {address:?}");
        let (payment_proofs, _skipped_payments) = self
            .pay_for_content_addrs(
                DataTypes::Chunk,
                std::iter::once((xor_name, chunk.size())),
                payment_option,
            )
            .await
            .inspect_err(|err| error!("Error paying for chunk {address:?} :{err:?}"))?;

        // verify payment was successful
        let (proof, price) = match payment_proofs.get(&xor_name) {
            Some((proof, price)) => (proof, price),
            None => {
                info!("Chunk at address: {address:?} was already paid for");
                return Ok((AttoTokens::zero(), *chunk.address()));
            }
        };
        let total_cost = *price;

        let payees = proof
            .payees()
            .iter()
            .map(|(peer_id, _addrs)| *peer_id)
            .collect();

        let record = Record {
            key: address.to_record_key(),
            value: try_serialize_record(
                &(proof.to_proof_of_payment(), chunk),
                RecordKind::DataWithPayment(DataTypes::Chunk),
            )
            .map_err(|_| {
                PutError::Serialization("Failed to serialize chunk with payment".to_string())
            })?
            .to_vec(),
            publisher: None,
            expires: None,
        };

        // store the chunk on the network
        debug!("Storing chunk at address: {address:?} to the network");

        self.network
            .put_record_with_retries(record, payees, &self.config.chunks)
            .await
            .inspect_err(|err| {
                error!("Failed to put record - chunk {address:?} to the network: {err}")
            })
            .map_err(|err| PutError::Network {
                address: address.clone(),
                network_error: err.clone(),
                payment: Some(payment_proofs),
            })?;

        Ok((total_cost, *chunk.address()))
    }

    /// Get the cost of a chunk.
    pub async fn chunk_cost(&self, addr: &ChunkAddress) -> Result<AttoTokens, CostError> {
        trace!("Getting cost for chunk of {addr:?}");

        let xor = *addr.xorname();
        let store_quote = self
            .get_store_quotes(DataTypes::Chunk, std::iter::once((xor, Chunk::MAX_SIZE)))
            .await?;
        let total_cost = AttoTokens::from_atto(
            store_quote
                .0
                .values()
                .map(|quote| quote.price())
                .sum::<Amount>(),
        );
        debug!("Calculated the cost to create chunk of {addr:?} is {total_cost}");
        Ok(total_cost)
    }

    /// Upload chunks in batches
    pub(crate) async fn chunk_batch_upload(
        &self,
        chunks: Vec<&Chunk>,
        receipt: &Receipt,
    ) -> Result<(), PutError> {
        let mut upload_tasks = vec![];
        #[cfg(feature = "loud")]
        let total_chunks = chunks.len();
        for (_i, &chunk) in chunks.iter().enumerate() {
            let self_clone = self.clone();
            let address = *chunk.address();

            let Some((proof, price)) = receipt.get(chunk.name()) else {
                debug!("Chunk at {address:?} was already paid for so skipping");
                #[cfg(feature = "loud")]
                println!(
                    "({}/{}) Chunk stored at: {} (skipping, already exists)",
                    _i + 1,
                    chunks.len(),
                    chunk.address().to_hex()
                );
                continue;
            };

            upload_tasks.push(async move {
                let res = self_clone
                    .chunk_upload_with_payment(chunk, proof.clone(), *price)
                    .await
                    .inspect_err(|err| error!("Error uploading chunk {address:?} :{err:?}"))
                    .map_err(|e| (chunk, e));
                #[cfg(feature = "loud")]
                match &res {
                    Ok(_addr) => {
                        println!(
                            "({}/{}) Chunk stored at: {}",
                            _i + 1,
                            total_chunks,
                            chunk.address().to_hex()
                        );
                    }
                    Err((_, err)) => {
                        println!(
                            "({}/{}) Chunk failed to be stored at: {} ({err})",
                            _i + 1,
                            total_chunks,
                            chunk.address().to_hex()
                        );
                    }
                }
                res
            });
        }
        let uploads =
            process_tasks_with_max_concurrency(upload_tasks, *CHUNK_UPLOAD_BATCH_SIZE).await;

        // return errors
        if uploads.iter().any(|res| res.is_err()) {
            let mut state = ChunkBatchUploadState::default();
            for res in uploads.into_iter() {
                match res {
                    Ok(addr) => state.successful.push(addr),
                    Err((chunk, err)) => state.push_error(*chunk.address(), err),
                }
            }
            return Err(PutError::Batch(state));
        }

        Ok(())
    }

    pub(crate) async fn chunk_upload_with_payment(
        &self,
        chunk: &Chunk,
        payment: ClientProofOfPayment,
        price: AttoTokens,
    ) -> Result<ChunkAddress, PutError> {
        let storing_nodes: Vec<_> = payment
            .payees()
            .iter()
            .map(|(peer_id, _addrs)| *peer_id)
            .collect();

        if storing_nodes.is_empty() {
            return Err(PutError::PayeesMissing);
        }

        debug!("Storing chunk: {chunk:?} to {:?}", storing_nodes);

        let key = chunk.network_address().to_record_key();

        let record_kind = RecordKind::DataWithPayment(DataTypes::Chunk);
        let record = Record {
            key: key.clone(),
            value: try_serialize_record(
                &(payment.to_proof_of_payment(), chunk.clone()),
                record_kind,
            )
            .map_err(|e| {
                PutError::Serialization(format!("Failed to serialize chunk with payment: {e:?}"))
            })?
            .to_vec(),
            publisher: None,
            expires: None,
        };

        self.network
            .put_record_with_retries(record, storing_nodes.clone(), &self.config.chunks)
            .await
            .map_err(|err| {
                let receipt = HashMap::from_iter([(*chunk.name(), (payment, price))]);
                PutError::Network {
                    address: NetworkAddress::from(*chunk.address()),
                    network_error: err,
                    payment: Some(receipt),
                }
            })?;
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
                    .chunk_get(&ChunkAddress::new(info.dst_hash))
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
