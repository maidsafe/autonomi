// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::client::payment::{receipt_from_store_quotes, Receipt};
use ant_evm::{EvmWallet, ProofOfPayment};
use ant_networking::{GetRecordCfg, PutRecordCfg, VerificationKind};
use ant_protocol::{
    messages::ChunkProof,
    storage::{try_serialize_record, Chunk, RecordKind, RetryStrategy},
};
use bytes::Bytes;
use futures::stream::{FuturesUnordered, StreamExt};
use libp2p::kad::{Quorum, Record};
use rand::{thread_rng, Rng};
use self_encryption::{decrypt_full_set, DataMap, EncryptedChunk};
use std::{future::Future, num::NonZero};
use xor_name::XorName;

use super::{
    data::{GetError, PayError, PutError, CHUNK_DOWNLOAD_BATCH_SIZE},
    Client,
};
use crate::self_encryption::DataMapLevel;

impl Client {
    /// Fetch and decrypt all chunks in the data map.
    pub(crate) async fn fetch_from_data_map(&self, data_map: &DataMap) -> Result<Bytes, GetError> {
        debug!("Fetching encrypted data chunks from data map {data_map:?}");
        let mut download_tasks = vec![];
        for info in data_map.infos() {
            download_tasks.push(async move {
                match self
                    .chunk_get(info.dst_hash)
                    .await
                    .inspect_err(|err| error!("Error fetching chunk {:?}: {err:?}", info.dst_hash))
                {
                    Ok(chunk) => Ok(EncryptedChunk {
                        index: info.index,
                        content: chunk.value,
                    }),
                    Err(err) => {
                        error!("Error fetching chunk {:?}: {err:?}", info.dst_hash);
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

    pub(crate) async fn chunk_upload_with_payment(
        &self,
        chunk: &Chunk,
        payment: ProofOfPayment,
    ) -> Result<(), PutError> {
        let storing_nodes = payment.payees();

        if storing_nodes.is_empty() {
            return Err(PutError::PayeesMissing);
        }

        debug!("Storing chunk: {chunk:?} to {:?}", storing_nodes);

        let key = chunk.network_address().to_record_key();

        let record_kind = RecordKind::ChunkWithPayment;
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
                get_quorum: Quorum::N(NonZero::new(2).expect("2 is non-zero")),
                retry_strategy: Some(RetryStrategy::Balanced),
                target_record: None,
                expected_holders: Default::default(),
                is_register: false,
            };

            let stored_on_node = try_serialize_record(&chunk, RecordKind::Chunk)
                .map_err(|e| PutError::Serialization(format!("Failed to serialize chunk: {e:?}")))?
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
            put_quorum: Quorum::One,
            retry_strategy: Some(RetryStrategy::Balanced),
            use_put_record_to: Some(storing_nodes.clone()),
            verification,
        };
        let payment_upload = Ok(self.network.put_record(record, &put_cfg).await?);

        match &payment_upload {
            Ok(_) => debug!("Successfully stored chunk: {chunk:?} to {storing_nodes:?}"),
            Err(err) => error!("Failed to store chunk: {chunk:?} to {storing_nodes:?}: {err:?}"),
        }
        payment_upload
    }

    /// Pay for the chunks and get the proof of payment.
    pub(crate) async fn pay(
        &self,
        content_addrs: impl Iterator<Item = XorName> + Clone,
        wallet: &EvmWallet,
    ) -> Result<Receipt, PayError> {
        let number_of_content_addrs = content_addrs.clone().count();
        let quotes = self.get_store_quotes(content_addrs).await?;

        // Make sure nobody else can use the wallet while we are paying
        debug!("Waiting for wallet lock");
        let lock_guard = wallet.lock().await;
        debug!("Locked wallet");

        // TODO: the error might contain some succeeded quote payments as well. These should be returned on err, so that they can be skipped when retrying.
        // TODO: retry when it fails?
        // Execute chunk payments
        let _payments = wallet
            .pay_for_quotes(quotes.payments())
            .await
            .map_err(|err| PayError::from(err.0))?;

        // payment is done, unlock the wallet for other threads
        drop(lock_guard);
        debug!("Unlocked wallet");

        let skipped_chunks = number_of_content_addrs - quotes.len();
        trace!(
            "Chunk payments of {} chunks completed. {} chunks were free / already paid for",
            quotes.len(),
            skipped_chunks
        );

        let receipt = receipt_from_store_quotes(quotes);

        Ok(receipt)
    }
}

pub(crate) async fn process_tasks_with_max_concurrency<I, R>(tasks: I, batch_size: usize) -> Vec<R>
where
    I: IntoIterator,
    I::Item: Future<Output = R> + Send,
    R: Send,
{
    let mut futures = FuturesUnordered::new();
    let mut results = Vec::new();

    for task in tasks.into_iter() {
        futures.push(task);

        if futures.len() >= batch_size {
            if let Some(result) = futures.next().await {
                results.push(result);
            }
        }
    }

    // Process remaining tasks
    while let Some(result) = futures.next().await {
        results.push(result);
    }

    results
}
