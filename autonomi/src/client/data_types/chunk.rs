// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::client::chunk_cache::{
    default_cache_dir, delete_chunks, is_chunk_cached, load_chunk, store_chunk,
};
use crate::client::config::{CHUNK_DOWNLOAD_BATCH_SIZE, CHUNK_UPLOAD_BATCH_SIZE};
use crate::networking::PeerInfo;
use crate::{
    Client,
    client::{
        ChunkBatchUploadState, GetError, PutError,
        payment::{PaymentOption, Receipt},
        quote::CostError,
    },
    utils::process_tasks_with_max_concurrency,
};
use ant_evm::{Amount, AttoTokens, ClientProofOfPayment};
pub use ant_protocol::storage::{Chunk, ChunkAddress};
use ant_protocol::{
    NetworkAddress,
    storage::{DataTypes, RecordHeader, RecordKind, try_deserialize_record, try_serialize_record},
};
use bytes::Bytes;
use libp2p::kad::Record;
use self_encryption::{DataMap, EncryptedChunk, decrypt};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
};

/// Private data on the network can be accessed with this
/// Uploading this data in a chunk makes it publicly accessible from the address of that Chunk
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DataMapChunk(pub Chunk);

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
    fn get_chunk_cache_dir(&self) -> Result<std::path::PathBuf, GetError> {
        match &self.config.chunk_cache_dir {
            Some(dir) => Ok(dir.clone()),
            None => {
                default_cache_dir().map_err(|_| {
                    GetError::Configuration(
                        "Chunk caching is enabled but no cache directory is specified. \
                         Please set a cache directory in the client config or disable chunk caching.".to_string()
                    )
                })
            }
        }
    }

    fn try_load_chunk_from_cache(&self, addr: &ChunkAddress) -> Result<Option<Chunk>, GetError> {
        if !self.config.chunk_cache_enabled {
            return Ok(None);
        }

        let cache_dir = self.get_chunk_cache_dir()?;
        if is_chunk_cached(cache_dir.clone(), addr)
            && let Ok(Some(cached_chunk)) = load_chunk(cache_dir, addr)
        {
            debug!("Loaded chunk from cache: {addr:?}");
            return Ok(Some(cached_chunk));
        }
        Ok(None)
    }

    fn try_cache_chunk(&self, addr: &ChunkAddress, chunk: &Chunk) -> Result<(), GetError> {
        if self.config.chunk_cache_enabled {
            let cache_dir = self.get_chunk_cache_dir()?;
            if let Err(e) = store_chunk(cache_dir, addr, chunk) {
                warn!("Failed to cache chunk {}: {}", addr.to_hex(), e);
            }
        }
        Ok(())
    }

    pub(crate) fn cleanup_cached_chunks(&self, chunk_addrs: &[ChunkAddress]) {
        if self.config.chunk_cache_enabled
            && let Ok(cache_dir) = self.get_chunk_cache_dir()
        {
            if let Err(e) = delete_chunks(cache_dir, chunk_addrs) {
                warn!("Failed to delete cached chunks after download: {e}");
            } else {
                debug!(
                    "Deleted {} cached chunks after successful download",
                    chunk_addrs.len()
                );
            }
        }
    }

    async fn fetch_chunk_from_network(&self, addr: &ChunkAddress) -> Result<Chunk, GetError> {
        let key = NetworkAddress::from(*addr);
        debug!("Fetching chunk from network at: {key:?}");

        let record = self
            .network
            .get_record_with_retries(key, &self.config.chunks)
            .await
            .inspect_err(|err| error!("Error fetching chunk: {err:?}"))?
            .ok_or(GetError::RecordNotFound)?;

        let header = RecordHeader::from_record(&record)?;

        if RecordHeader::is_record_of_type_chunk(&record)? {
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

    /// Get a chunk from the network.
    pub async fn chunk_get(&self, addr: &ChunkAddress) -> Result<Chunk, GetError> {
        info!("Getting chunk: {addr:?}");

        if let Some(cached_chunk) = self.try_load_chunk_from_cache(addr)? {
            return Ok(cached_chunk);
        }

        let chunk = self.fetch_chunk_from_network(addr).await?;
        self.try_cache_chunk(addr, &chunk)?;
        Ok(chunk)
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
            .map(|(peer_id, addrs)| PeerInfo {
                peer_id: *peer_id,
                addrs: addrs.clone(),
            })
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
                address: Box::new(address.clone()),
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

    /// Upload chunks in batches to the network. This is useful for pre-calculated payment proofs,
    /// in case of manual encryption or re-uploading certain chunks that were already paid for.
    ///
    /// This method requires a vector of chunks to be uploaded and the payment receipt. It returns a `PutError` for
    /// failures and `Ok(())` for successful uploads.
    ///
    /// # Example
    /// ```no_run
    /// # use ant_protocol::storage::DataTypes;
    /// # use autonomi::{Client, Wallet};
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::init_local().await?;
    /// # let wallet = Wallet::new_from_private_key(
    /// #     client.evm_network().clone(),
    /// #     "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
    /// # )?;
    ///
    /// // Step 1: Encrypt your data using self-encryption
    /// let (data_map, chunks) = autonomi::self_encryption::encrypt("Hello, World!".into())?;
    ///
    /// // Step 2: Collect all chunks (datamap + content chunks)
    /// let mut all_chunks = vec![&data_map];
    /// all_chunks.extend(chunks.iter());
    ///
    /// // Step 3: Get storage quotes for all chunks
    /// let quote = client.get_store_quotes(
    ///     DataTypes::Chunk,
    ///     all_chunks.iter().map(|chunk| (*chunk.address().xorname(), chunk.size())),
    /// ).await?;
    ///
    /// // Step 4: Pay for all chunks at once and get receipt
    /// wallet.pay_for_quotes(quote.payments()).await.map_err(|err| err.0)?;
    /// let receipt = autonomi::client::payment::receipt_from_store_quotes(quote);
    ///
    /// // Step 5: Upload all chunks with the payment receipt
    /// client.chunk_batch_upload(all_chunks, &receipt).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn chunk_batch_upload(
        &self,
        chunks: Vec<&Chunk>,
        receipt: &Receipt,
    ) -> Result<(), PutError> {
        let mut upload_tasks = vec![];
        #[cfg(feature = "loud")]
        let total_chunks = chunks.len();
        for (i, &chunk) in chunks.iter().enumerate() {
            let self_clone = self.clone();
            let address = *chunk.address();

            let Some((proof, price)) = receipt.get(chunk.name()) else {
                debug!(
                    "({}/{}) Chunk at {address:?} was already paid for so skipping",
                    i + 1,
                    chunks.len()
                );
                #[cfg(feature = "loud")]
                println!(
                    "({}/{}) Chunk stored at: {} (skipping, already exists)",
                    i + 1,
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
                            i + 1,
                            total_chunks,
                            chunk.address().to_hex()
                        );
                    }
                    Err((_, err)) => {
                        println!(
                            "({}/{}) Chunk failed to be stored at: {} ({err})",
                            i + 1,
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
            .map(|(peer_id, addrs)| PeerInfo {
                peer_id: *peer_id,
                addrs: addrs.clone(),
            })
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
                    address: Box::new(NetworkAddress::from(*chunk.address())),
                    network_error: err,
                    payment: Some(receipt),
                }
            })?;
        debug!("Successfully stored chunk: {chunk:?} to {storing_nodes:?}");
        Ok(*chunk.address())
    }

    /// Fetch and decrypt all chunks in the datamap.
    pub(crate) async fn fetch_from_data_map(&self, data_map: &DataMap) -> Result<Bytes, GetError> {
        let total_chunks = data_map.infos().len();
        #[cfg(feature = "loud")]
        println!("Fetching {total_chunks} encrypted data chunks from network.");
        debug!("Fetching {total_chunks} encrypted data chunks from datamap {data_map:?}");

        let mut download_tasks = vec![];
        let chunk_addrs: Vec<ChunkAddress> = data_map
            .infos()
            .iter()
            .map(|info| ChunkAddress::new(info.dst_hash))
            .collect();

        for (i, info) in data_map.infos().into_iter().enumerate() {
            download_tasks.push(async move {
                let idx = i + 1;
                let chunk_addr = ChunkAddress::new(info.dst_hash);

                #[cfg(feature = "loud")]
                println!("Fetching chunk {idx}/{total_chunks} ...");
                info!("Fetching chunk {idx}/{total_chunks}({chunk_addr:?})");

                match self.chunk_get(&chunk_addr).await {
                    Ok(chunk) => {
                        #[cfg(feature = "loud")]
                        println!("Fetching chunk {idx}/{total_chunks} [DONE]");
                        info!("Successfully fetched chunk {idx}/{total_chunks}({chunk_addr:?})");
                        Ok(EncryptedChunk {
                            content: chunk.value,
                        })
                    }
                    Err(err) => {
                        #[cfg(feature = "loud")]
                        println!("Error fetching chunk {idx}/{total_chunks}: {err:?}");
                        error!(
                            "Error fetching chunk {idx}/{total_chunks}({chunk_addr:?}): {err:?}"
                        );
                        Err(err)
                    }
                }
            });
        }
        let encrypted_chunks =
            process_tasks_with_max_concurrency(download_tasks, *CHUNK_DOWNLOAD_BATCH_SIZE)
                .await
                .into_iter()
                .collect::<Result<Vec<EncryptedChunk>, GetError>>()?;
        #[cfg(feature = "loud")]
        println!("Successfully fetched all {total_chunks} encrypted chunks");
        debug!("Successfully fetched all {total_chunks} encrypted chunks");

        let data = decrypt(data_map, &encrypted_chunks).map_err(|e| {
            error!("Error decrypting encrypted_chunks: {e:?}");
            GetError::Decryption(crate::self_encryption::Error::SelfEncryption(e))
        })?;
        #[cfg(feature = "loud")]
        println!("Successfully decrypted all {total_chunks} chunks");
        debug!("Successfully decrypted all {total_chunks} chunks");

        self.cleanup_cached_chunks(&chunk_addrs);

        Ok(data)
    }
}
