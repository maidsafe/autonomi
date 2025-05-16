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
        utils::{
            process_request_tasks_expect_majority_succeeds, process_tasks_with_max_concurrency,
        },
        ChunkBatchUploadState, GetError, PutError,
    },
    networking::common::Addresses,
    self_encryption::DataMapLevel,
    Client, XorName,
};
use ant_evm::{Amount, AttoTokens, ClientProofOfPayment, ProofOfPayment};
pub use ant_protocol::storage::{Chunk, ChunkAddress};
use ant_protocol::{
    messages::{Query, Request},
    storage::{
        try_deserialize_record, try_serialize_record, DataTypes, RecordHeader, RecordKind,
        ValidationType,
    },
    NetworkAddress,
};
use bytes::Bytes;
use self_encryption::{decrypt_full_set, DataMap, EncryptedChunk};
use serde::{Deserialize, Serialize};
use std::{
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
        let (_proof, price) = match payment_proofs.get(&xor_name) {
            Some((proof, price)) => (proof, price),
            None => {
                info!("Chunk at address: {address:?} was already stored.");
                return Ok((AttoTokens::zero(), *chunk.address()));
            }
        };
        let total_cost = *price;

        self.send_payment_notifications(&payment_proofs, DataTypes::Chunk, ValidationType::Chunk)
            .await?;

        let mut serialized_records = vec![];
        let serialized_record =
            try_serialize_record(&chunk, RecordKind::DataOnly(DataTypes::Chunk))?.to_vec();
        serialized_records.push((*chunk.name(), *chunk.address(), serialized_record));
        self.upload_records_content(serialized_records, &payment_proofs)
            .await?;

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
        self.send_payment_notifications(receipt, DataTypes::Chunk, ValidationType::Chunk)
            .await?;

        let mut serialized_records = vec![];
        for chunk in chunks.iter() {
            let serialized_record =
                try_serialize_record(&chunk, RecordKind::DataOnly(DataTypes::Chunk))?.to_vec();
            serialized_records.push((*chunk.name(), *chunk.address(), serialized_record));
        }
        self.upload_records_content(serialized_records, receipt)
            .await
    }

    /// Send payment_notifications rquests of one record
    async fn send_payment_notification_of_one_record(
        &self,
        proof: &ClientProofOfPayment,
        record_info: &(NetworkAddress, DataTypes, ValidationType, ProofOfPayment),
    ) -> Result<(), PutError> {
        let mut tasks = vec![];
        for (peer_id, addrs) in proof.payees() {
            let request = Request::Query(Query::PaymentNotification {
                holder: NetworkAddress::from(peer_id),
                record_info: record_info.clone(),
            });
            let self_clone = self.clone();
            tasks.push(async move {
                self_clone
                    .network
                    .send_request(peer_id, Addresses(addrs), request)
                    .await
            });
        }

        process_request_tasks_expect_majority_succeeds(tasks.len(), tasks).await
    }

    /// Send payment_notifications rquests of one record
    async fn upload_content_of_one_record(
        &self,
        address: NetworkAddress,
        proof: &ClientProofOfPayment,
        serialized_record: &[u8],
    ) -> Result<(), PutError> {
        let mut tasks = vec![];
        for (peer_id, addrs) in proof.payees() {
            let request = Request::Query(Query::UploadRecord {
                holder: NetworkAddress::from(peer_id),
                address: address.clone(),
                serialized_record: serialized_record.to_owned(),
            });
            let self_clone = self.clone();
            tasks.push(async move {
                self_clone
                    .network
                    .send_request(peer_id, Addresses(addrs), request)
                    .await
            });
        }

        process_request_tasks_expect_majority_succeeds(tasks.len(), tasks).await
    }

    /// Send payment notifications in batches
    async fn send_payment_notifications(
        &self,
        receipt: &Receipt,
        data_type: DataTypes,
        validation_type: ValidationType,
    ) -> Result<(), PutError> {
        #[cfg(feature = "loud")]
        let total_payments = receipt.len();

        let mut payment_notification_tasks = vec![];
        // Send PaymentNotification, those `0` balance quotes are already paid and can be skipped
        for (_i, (name, (proof, balance))) in receipt.iter().enumerate() {
            if !balance.is_zero() {
                let record_info = (
                    NetworkAddress::from(ChunkAddress::new(*name)),
                    data_type,
                    validation_type.clone(),
                    proof.to_proof_of_payment(),
                );
                let self_clone = self.clone();
                payment_notification_tasks.push(async move {
                    let res = self_clone.send_payment_notification_of_one_record(proof, &record_info).await;
                    #[cfg(feature = "loud")]
                    match &res {
                        Ok(_) => {
                            println!(
                                "({}/{total_payments}) Payment of Chunk {name:?} notified to payees",
                                _i + 1,
                            );
                        }
                        Err(err) => {
                            println!(
                                "({}/{total_payments}) Payment of Chunk {name:?} failed notify payees ({err})",
                                _i + 1,
                            );
                        }
                    }
                    (ChunkAddress::new(*name), res)
                });
            } else {
                debug!("Chunk of {name:?} was already paid, only need upload");
                #[cfg(feature = "loud")]
                debug!("Chunk of {name:?} was already paid, only need upload");
            }
        }

        let payment_notifications = process_tasks_with_max_concurrency(
            payment_notification_tasks,
            *CHUNK_UPLOAD_BATCH_SIZE,
        )
        .await;

        // return errors if any
        if payment_notifications.iter().any(|(_, res)| res.is_err()) {
            let mut state = ChunkBatchUploadState::default();
            for (chunk_addr, res) in payment_notifications.into_iter() {
                match res {
                    Ok(_) => state.successful.push(chunk_addr),
                    Err(err) => state.push_error(chunk_addr, err),
                }
            }
            Err(PutError::Batch(state))
        } else {
            Ok(())
        }
    }

    /// Upload records (content only) in batches
    async fn upload_records_content(
        &self,
        serialized_records: Vec<(XorName, ChunkAddress, Vec<u8>)>,
        receipt: &Receipt,
    ) -> Result<(), PutError> {
        #[cfg(feature = "loud")]
        let total_records = serialized_records.len();

        let mut upload_tasks = vec![];
        // Upload chunks
        for (_i, (name, address, serialized_record)) in serialized_records.into_iter().enumerate() {
            let Some((proof, _price)) = receipt.get(&name) else {
                debug!("Record at {name:?} was already uploaded so skipping");
                #[cfg(feature = "loud")]
                println!(
                    "({}/{total_records}) Record stored at: {name:} (skipping, already exists)",
                    _i + 1,
                );
                continue;
            };

            let self_clone = self.clone();

            upload_tasks.push(async move {
                let res = self_clone
                    .upload_content_of_one_record(
                        NetworkAddress::from(address),
                        proof,
                        &serialized_record,
                    )
                    .await;
                #[cfg(feature = "loud")]
                match &res {
                    Ok(_) => {
                        println!("({}/{total_records}) Record {address:?} stored", _i + 1,);
                    }
                    Err(err) => {
                        println!(
                            "({}/{total_records}) Record {address:?} failed upload ({err})",
                            _i + 1,
                        );
                    }
                }
                (address, res)
            });
        }
        let uploads =
            process_tasks_with_max_concurrency(upload_tasks, *CHUNK_UPLOAD_BATCH_SIZE).await;

        // return errors if any
        if uploads.iter().any(|(_, res)| res.is_err()) {
            let mut state = ChunkBatchUploadState::default();
            for (addr, res) in uploads.into_iter() {
                match res {
                    Ok(_) => state.successful.push(addr),
                    Err(err) => state.push_error(addr, err),
                }
            }
            Err(PutError::Batch(state))
        } else {
            Ok(())
        }
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
