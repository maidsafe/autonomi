// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::collections::BTreeSet;

use crate::{node::Node, Error, Marker, Result};
use ant_evm::payment_vault::verify_data_payment;
use ant_evm::{AttoTokens, ProofOfPayment};
use ant_networking::NetworkError;
use ant_protocol::storage::Transaction;
use ant_protocol::{
    storage::{
        try_deserialize_record, try_serialize_record, Chunk, RecordHeader, RecordKind, RecordType,
        Scratchpad, TransactionAddress,
    },
    NetworkAddress, PrettyPrintRecordKey,
};
use ant_registers::SignedRegister;
use libp2p::kad::{Record, RecordKey};
use xor_name::XorName;

impl Node {
    /// Validate a record and its payment, and store the record to the RecordStore
    pub(crate) async fn validate_and_store_record(&self, record: Record) -> Result<()> {
        let record_header = RecordHeader::from_record(&record)?;

        match record_header.kind {
            RecordKind::ChunkWithPayment => {
                let record_key = record.key.clone();
                let (payment, chunk) = try_deserialize_record::<(ProofOfPayment, Chunk)>(&record)?;
                let already_exists = self
                    .validate_key_and_existence(&chunk.network_address(), &record_key)
                    .await?;

                // Validate the payment and that we received what we asked.
                // This stores any payments to disk
                let payment_res = self
                    .payment_for_us_exists_and_is_still_valid(&chunk.network_address(), payment)
                    .await;

                // Now that we've taken any money passed to us, regardless of the payment's validity,
                // if we already have the data we can return early
                if already_exists {
                    // if we're receiving this chunk PUT again, and we have been paid,
                    // we eagerly retry replicaiton as it seems like other nodes are having trouble
                    // did not manage to get this chunk as yet
                    self.replicate_valid_fresh_record(record_key, RecordType::Chunk);

                    // Notify replication_fetcher to mark the attempt as completed.
                    // Send the notification earlier to avoid it got skipped due to:
                    // the record becomes stored during the fetch because of other interleaved process.
                    self.network()
                        .notify_fetch_completed(record.key.clone(), RecordType::Chunk);

                    debug!(
                        "Chunk with addr {:?} already exists: {already_exists}, payment extracted.",
                        chunk.network_address()
                    );
                    return Ok(());
                }

                // Finally before we store, lets bail for any payment issues
                payment_res?;

                // Writing chunk to disk takes time, hence try to execute it first.
                // So that when the replicate target asking for the copy,
                // the node can have a higher chance to respond.
                let store_chunk_result = self.store_chunk(&chunk);

                if store_chunk_result.is_ok() {
                    Marker::ValidPaidChunkPutFromClient(&PrettyPrintRecordKey::from(&record.key))
                        .log();
                    self.replicate_valid_fresh_record(record_key, RecordType::Chunk);

                    // Notify replication_fetcher to mark the attempt as completed.
                    // Send the notification earlier to avoid it got skipped due to:
                    // the record becomes stored during the fetch because of other interleaved process.
                    self.network()
                        .notify_fetch_completed(record.key.clone(), RecordType::Chunk);
                }

                store_chunk_result
            }

            RecordKind::Chunk => {
                error!("Chunk should not be validated at this point");
                Err(Error::InvalidPutWithoutPayment(
                    PrettyPrintRecordKey::from(&record.key).into_owned(),
                ))
            }
            RecordKind::ScratchpadWithPayment => {
                let record_key = record.key.clone();
                let (payment, scratchpad) =
                    try_deserialize_record::<(ProofOfPayment, Scratchpad)>(&record)?;
                let _already_exists = self
                    .validate_key_and_existence(&scratchpad.network_address(), &record_key)
                    .await?;

                // Validate the payment and that we received what we asked.
                // This stores any payments to disk
                let payment_res = self
                    .payment_for_us_exists_and_is_still_valid(
                        &scratchpad.network_address(),
                        payment,
                    )
                    .await;

                // Finally before we store, lets bail for any payment issues
                payment_res?;

                // Writing records to disk takes time, hence try to execute it first.
                // So that when the replicate target asking for the copy,
                // the node can have a higher chance to respond.
                let store_scratchpad_result = self
                    .validate_and_store_scratchpad_record(scratchpad, record_key.clone(), true)
                    .await;

                match store_scratchpad_result {
                    // if we're receiving this scratchpad PUT again, and we have been paid,
                    // we eagerly retry replicaiton as it seems like other nodes are having trouble
                    // did not manage to get this scratchpad as yet.
                    Ok(_) | Err(Error::IgnoringOutdatedScratchpadPut) => {
                        Marker::ValidScratchpadRecordPutFromClient(&PrettyPrintRecordKey::from(
                            &record_key,
                        ))
                        .log();
                        self.replicate_valid_fresh_record(
                            record_key.clone(),
                            RecordType::Scratchpad,
                        );

                        // Notify replication_fetcher to mark the attempt as completed.
                        // Send the notification earlier to avoid it got skipped due to:
                        // the record becomes stored during the fetch because of other interleaved process.
                        self.network()
                            .notify_fetch_completed(record_key, RecordType::Scratchpad);
                    }
                    Err(_) => {}
                }

                store_scratchpad_result
            }
            RecordKind::Scratchpad => {
                // make sure we already have this scratchpad locally, else reject it as first time upload needs payment
                let key = record.key.clone();
                let scratchpad = try_deserialize_record::<Scratchpad>(&record)?;
                let net_addr = NetworkAddress::ScratchpadAddress(*scratchpad.address());
                let pretty_key = PrettyPrintRecordKey::from(&key);
                trace!("Got record to store without payment for scratchpad at {pretty_key:?}");
                if !self.validate_key_and_existence(&net_addr, &key).await? {
                    warn!("Ignore store without payment for scratchpad at {pretty_key:?}");
                    return Err(Error::InvalidPutWithoutPayment(
                        PrettyPrintRecordKey::from(&record.key).into_owned(),
                    ));
                }

                // store the scratchpad
                self.validate_and_store_scratchpad_record(scratchpad, key, false)
                    .await
            }
            RecordKind::Transaction => {
                // Transactions should always be paid for
                error!("Transaction should not be validated at this point");
                Err(Error::InvalidPutWithoutPayment(
                    PrettyPrintRecordKey::from(&record.key).into_owned(),
                ))
            }
            RecordKind::TransactionWithPayment => {
                let (payment, transaction) =
                    try_deserialize_record::<(ProofOfPayment, Transaction)>(&record)?;

                // check if the deserialized value's TransactionAddress matches the record's key
                let net_addr = NetworkAddress::from_transaction_address(transaction.address());
                let key = net_addr.to_record_key();
                let pretty_key = PrettyPrintRecordKey::from(&key);
                if record.key != key {
                    warn!(
                        "Record's key {pretty_key:?} does not match with the value's TransactionAddress, ignoring PUT."
                    );
                    return Err(Error::RecordKeyMismatch);
                }

                let already_exists = self.validate_key_and_existence(&net_addr, &key).await?;

                // The transaction may already exist during the replication.
                // The payment shall get deposit to self even the transaction already presents.
                // However, if the transaction is already present, the incoming one shall be
                // appended with the existing one, if content is different.
                if let Err(err) = self
                    .payment_for_us_exists_and_is_still_valid(&net_addr, payment)
                    .await
                {
                    if already_exists {
                        debug!("Payment of the incoming exists transaction {pretty_key:?} having error {err:?}");
                    } else {
                        error!("Payment of the incoming non-exist transaction {pretty_key:?} having error {err:?}");
                        return Err(err);
                    }
                }

                let res = self
                    .validate_merge_and_store_transactions(vec![transaction], &key)
                    .await;
                if res.is_ok() {
                    let content_hash = XorName::from_content(&record.value);
                    Marker::ValidTransactionPutFromClient(&PrettyPrintRecordKey::from(&record.key))
                        .log();
                    self.replicate_valid_fresh_record(
                        record.key.clone(),
                        RecordType::NonChunk(content_hash),
                    );

                    // Notify replication_fetcher to mark the attempt as completed.
                    // Send the notification earlier to avoid it got skipped due to:
                    // the record becomes stored during the fetch because of other interleaved process.
                    self.network().notify_fetch_completed(
                        record.key.clone(),
                        RecordType::NonChunk(content_hash),
                    );
                }
                res
            }
            RecordKind::Register => {
                let register = try_deserialize_record::<SignedRegister>(&record)?;

                // make sure we already have this register locally
                let net_addr = NetworkAddress::from_register_address(*register.address());
                let key = net_addr.to_record_key();
                let pretty_key = PrettyPrintRecordKey::from(&key);
                debug!("Got record to store without payment for register at {pretty_key:?}");
                if !self.validate_key_and_existence(&net_addr, &key).await? {
                    debug!("Ignore store without payment for register at {pretty_key:?}");
                    return Err(Error::InvalidPutWithoutPayment(
                        PrettyPrintRecordKey::from(&record.key).into_owned(),
                    ));
                }

                // store the update
                debug!("Store update without payment as we already had register at {pretty_key:?}");
                let result = self.validate_and_store_register(register, true).await;

                if result.is_ok() {
                    debug!("Successfully stored register update at {pretty_key:?}");
                    Marker::ValidPaidRegisterPutFromClient(&pretty_key).log();
                    // we dont try and force replicaiton here as there's state to be kept in sync
                    // which we leave up to the client to enforce

                    let content_hash = XorName::from_content(&record.value);

                    // Notify replication_fetcher to mark the attempt as completed.
                    // Send the notification earlier to avoid it got skipped due to:
                    // the record becomes stored during the fetch because of other interleaved process.
                    self.network().notify_fetch_completed(
                        record.key.clone(),
                        RecordType::NonChunk(content_hash),
                    );
                } else {
                    warn!("Failed to store register update at {pretty_key:?}");
                }
                result
            }
            RecordKind::RegisterWithPayment => {
                let (payment, register) =
                    try_deserialize_record::<(ProofOfPayment, SignedRegister)>(&record)?;

                // check if the deserialized value's RegisterAddress matches the record's key
                let net_addr = NetworkAddress::from_register_address(*register.address());
                let key = net_addr.to_record_key();
                let pretty_key = PrettyPrintRecordKey::from(&key);
                if record.key != key {
                    warn!(
                        "Record's key {pretty_key:?} does not match with the value's RegisterAddress, ignoring PUT."
                    );
                    return Err(Error::RecordKeyMismatch);
                }

                let already_exists = self.validate_key_and_existence(&net_addr, &key).await?;

                // The register may already exist during the replication.
                // The payment shall get deposit to self even the register already presents.
                // However, if the register already presents, the incoming one maybe for edit only.
                // Hence the corresponding payment error shall not be thrown out.
                if let Err(err) = self
                    .payment_for_us_exists_and_is_still_valid(&net_addr, payment)
                    .await
                {
                    if already_exists {
                        debug!("Payment of the incoming exists register {pretty_key:?} having error {err:?}");
                    } else {
                        error!("Payment of the incoming non-exist register {pretty_key:?} having error {err:?}");
                        return Err(err);
                    }
                }

                let res = self.validate_and_store_register(register, true).await;
                if res.is_ok() {
                    let content_hash = XorName::from_content(&record.value);

                    // Notify replication_fetcher to mark the attempt as completed.
                    // Send the notification earlier to avoid it got skipped due to:
                    // the record becomes stored during the fetch because of other interleaved process.
                    self.network().notify_fetch_completed(
                        record.key.clone(),
                        RecordType::NonChunk(content_hash),
                    );
                }
                res
            }
        }
    }

    /// Store a pre-validated, and already paid record to the RecordStore
    pub(crate) async fn store_replicated_in_record(&self, record: Record) -> Result<()> {
        debug!("Storing record which was replicated to us {:?}", record.key);
        let record_header = RecordHeader::from_record(&record)?;
        match record_header.kind {
            // A separate flow handles payment for chunks and registers
            RecordKind::ChunkWithPayment
            | RecordKind::TransactionWithPayment
            | RecordKind::RegisterWithPayment
            | RecordKind::ScratchpadWithPayment => {
                warn!("Prepaid record came with Payment, which should be handled in another flow");
                Err(Error::UnexpectedRecordWithPayment(
                    PrettyPrintRecordKey::from(&record.key).into_owned(),
                ))
            }
            RecordKind::Chunk => {
                let chunk = try_deserialize_record::<Chunk>(&record)?;

                let record_key = record.key.clone();
                let already_exists = self
                    .validate_key_and_existence(&chunk.network_address(), &record_key)
                    .await?;
                if already_exists {
                    debug!(
                        "Chunk with addr {:?} already exists?: {already_exists}, do nothing",
                        chunk.network_address()
                    );
                    return Ok(());
                }

                self.store_chunk(&chunk)
            }
            RecordKind::Scratchpad => {
                let key = record.key.clone();
                let scratchpad = try_deserialize_record::<Scratchpad>(&record)?;
                self.validate_and_store_scratchpad_record(scratchpad, key, false)
                    .await
            }
            RecordKind::Transaction => {
                let record_key = record.key.clone();
                let transactions = try_deserialize_record::<Vec<Transaction>>(&record)?;
                self.validate_merge_and_store_transactions(transactions, &record_key)
                    .await
            }
            RecordKind::Register => {
                let register = try_deserialize_record::<SignedRegister>(&record)?;

                // check if the deserialized value's RegisterAddress matches the record's key
                let key =
                    NetworkAddress::from_register_address(*register.address()).to_record_key();
                if record.key != key {
                    warn!(
                        "Record's key does not match with the value's RegisterAddress, ignoring PUT."
                    );
                    return Err(Error::RecordKeyMismatch);
                }
                self.validate_and_store_register(register, false).await
            }
        }
    }

    /// Check key is valid compared to the network name, and if we already have this data or not.
    /// returns true if data already exists locally
    async fn validate_key_and_existence(
        &self,
        address: &NetworkAddress,
        expected_record_key: &RecordKey,
    ) -> Result<bool> {
        let data_key = address.to_record_key();
        let pretty_key = PrettyPrintRecordKey::from(&data_key);

        if expected_record_key != &data_key {
            warn!(
                "record key: {:?}, key: {:?}",
                PrettyPrintRecordKey::from(expected_record_key),
                pretty_key
            );
            warn!("Record's key does not match with the value's address, ignoring PUT.");
            return Err(Error::RecordKeyMismatch);
        }

        let present_locally = self
            .network()
            .is_record_key_present_locally(&data_key)
            .await?;

        if present_locally {
            // We may short circuit if the Record::key is present locally;
            debug!(
                "Record with addr {:?} already exists, not overwriting",
                address
            );
            return Ok(true);
        }

        Ok(false)
    }

    /// Store a `Chunk` to the RecordStore
    pub(crate) fn store_chunk(&self, chunk: &Chunk) -> Result<()> {
        let chunk_name = *chunk.name();
        let chunk_addr = *chunk.address();

        let key = NetworkAddress::from_chunk_address(*chunk.address()).to_record_key();
        let pretty_key = PrettyPrintRecordKey::from(&key).into_owned();

        let record = Record {
            key,
            value: try_serialize_record(&chunk, RecordKind::Chunk)?.to_vec(),
            publisher: None,
            expires: None,
        };

        // finally store the Record directly into the local storage
        debug!("Storing chunk {chunk_name:?} as Record locally");
        self.network().put_local_record(record);

        self.record_metrics(Marker::ValidChunkRecordPutFromNetwork(&pretty_key));

        self.events_channel()
            .broadcast(crate::NodeEvent::ChunkStored(chunk_addr));

        Ok(())
    }

    /// Validate and store a `Scratchpad` to the RecordStore
    ///
    /// When a node receives an update packet:
    /// Verify Name: It MUST hash the provided public key and confirm it matches the name in the packet.
    /// Check Counter: It MUST ensure that the new counter value is strictly greater than the currently stored value to prevent replay attacks.
    /// Verify Signature: It MUST use the public key to verify the BLS12-381 signature against the content hash and the counter.
    /// Accept or Reject: If all verifications succeed, the node MUST accept the packet and replace any previous version. Otherwise, it MUST reject the update.
    pub(crate) async fn validate_and_store_scratchpad_record(
        &self,
        scratchpad: Scratchpad,
        record_key: RecordKey,
        is_client_put: bool,
    ) -> Result<()> {
        // owner PK is defined herein, so as long as record key and this match, we're good
        let addr = scratchpad.address();
        let count = scratchpad.count();
        debug!("Validating and storing scratchpad {addr:?} with count {count}");

        // check if the deserialized value's RegisterAddress matches the record's key
        let scratchpad_key = NetworkAddress::ScratchpadAddress(*addr).to_record_key();
        if scratchpad_key != record_key {
            warn!("Record's key does not match with the value's ScratchpadAddress, ignoring PUT.");
            return Err(Error::RecordKeyMismatch);
        }

        // check if the Scratchpad is present locally that we don't have a newer version
        if let Some(local_pad) = self.network().get_local_record(&scratchpad_key).await? {
            let local_pad = try_deserialize_record::<Scratchpad>(&local_pad)?;
            if local_pad.count() >= scratchpad.count() {
                warn!("Rejecting Scratchpad PUT with counter less than or equal to the current counter");
                return Err(Error::IgnoringOutdatedScratchpadPut);
            }
        }

        // ensure data integrity
        if !scratchpad.is_valid() {
            warn!("Rejecting Scratchpad PUT with invalid signature");
            return Err(Error::InvalidScratchpadSignature);
        }

        info!(
            "Storing sratchpad {addr:?} with content of {:?} as Record locally",
            scratchpad.encrypted_data_hash()
        );

        let record = Record {
            key: scratchpad_key.clone(),
            value: try_serialize_record(&scratchpad, RecordKind::Scratchpad)?.to_vec(),
            publisher: None,
            expires: None,
        };
        self.network().put_local_record(record);

        let pretty_key = PrettyPrintRecordKey::from(&scratchpad_key);

        self.record_metrics(Marker::ValidScratchpadRecordPutFromNetwork(&pretty_key));

        if is_client_put {
            self.replicate_valid_fresh_record(scratchpad_key, RecordType::Scratchpad);
        }

        Ok(())
    }
    /// Validate and store a `Register` to the RecordStore
    pub(crate) async fn validate_and_store_register(
        &self,
        register: SignedRegister,
        is_client_put: bool,
    ) -> Result<()> {
        let reg_addr = register.address();
        debug!("Validating and storing register {reg_addr:?}");

        // check if the Register is present locally
        let key = NetworkAddress::from_register_address(*reg_addr).to_record_key();
        let present_locally = self.network().is_record_key_present_locally(&key).await?;
        let pretty_key = PrettyPrintRecordKey::from(&key);

        // check register and merge if needed
        let updated_register = match self.register_validation(&register, present_locally).await? {
            Some(reg) => {
                debug!("Register {pretty_key:?} needed to be updated");
                reg
            }
            None => {
                debug!("No update needed for register");
                return Ok(());
            }
        };

        // store in kad
        let record = Record {
            key: key.clone(),
            value: try_serialize_record(&updated_register, RecordKind::Register)?.to_vec(),
            publisher: None,
            expires: None,
        };
        let content_hash = XorName::from_content(&record.value);

        info!("Storing register {reg_addr:?} with content of {content_hash:?} as Record locally");
        self.network().put_local_record(record);

        self.record_metrics(Marker::ValidRegisterRecordPutFromNetwork(&pretty_key));

        // Updated register needs to be replicated out as well,
        // to avoid `leaking` of old version due to the mismatch of
        // `close_range` and `replication_range`, combined with nodes churning
        //
        // However, to avoid `looping of replication`, a `replicated in` register
        // shall not trigger any further replication out.
        if is_client_put {
            self.replicate_valid_fresh_record(key, RecordType::NonChunk(content_hash));
        }

        Ok(())
    }

    /// Validate and store `Vec<Transaction>` to the RecordStore
    /// If we already have a transaction at this address, the Vec is extended and stored.
    pub(crate) async fn validate_merge_and_store_transactions(
        &self,
        transactions: Vec<Transaction>,
        record_key: &RecordKey,
    ) -> Result<()> {
        let pretty_key = PrettyPrintRecordKey::from(record_key);
        debug!("Validating transactions before storage at {pretty_key:?}");

        // only keep transactions that match the record key
        let transactions_for_key: Vec<Transaction> = transactions
            .into_iter()
            .filter(|s| {
                // get the record key for the transaction
                let transaction_address = s.address();
                let network_address = NetworkAddress::from_transaction_address(transaction_address);
                let transaction_record_key = network_address.to_record_key();
                let transaction_pretty = PrettyPrintRecordKey::from(&transaction_record_key);
                if &transaction_record_key != record_key {
                    warn!("Ignoring transaction for another record key {transaction_pretty:?} when verifying: {pretty_key:?}");
                    return false;
                }
                true
            })
            .collect();

        // if we have no transactions to verify, return early
        if transactions_for_key.is_empty() {
            warn!("Found no valid transactions to verify upon validation for {pretty_key:?}");
            return Err(Error::InvalidRequest(format!(
                "No transactions to verify when validating {pretty_key:?}"
            )));
        }

        // verify the transactions
        let mut validated_transactions: BTreeSet<Transaction> = transactions_for_key
            .into_iter()
            .filter(|t| t.verify())
            .collect();

        // skip if none are valid
        let addr = match validated_transactions.first() {
            None => {
                warn!("Found no validated transactions to store at {pretty_key:?}");
                return Ok(());
            }
            Some(t) => t.address(),
        };

        // add local transactions to the validated transactions, turn to Vec
        let local_txs = self.get_local_transactions(addr).await?;
        validated_transactions.extend(local_txs.into_iter());
        let validated_transactions: Vec<Transaction> = validated_transactions.into_iter().collect();

        // store the record into the local storage
        let record = Record {
            key: record_key.clone(),
            value: try_serialize_record(&validated_transactions, RecordKind::Transaction)?.to_vec(),
            publisher: None,
            expires: None,
        };
        self.network().put_local_record(record);
        debug!("Successfully stored validated transactions at {pretty_key:?}");

        // Just log the multiple transactions
        if validated_transactions.len() > 1 {
            debug!(
                "Got multiple transaction(s) of len {} at {pretty_key:?}",
                validated_transactions.len()
            );
        }

        self.record_metrics(Marker::ValidTransactionRecordPutFromNetwork(&pretty_key));
        Ok(())
    }

    /// Perform validations on the provided `Record`.
    async fn payment_for_us_exists_and_is_still_valid(
        &self,
        address: &NetworkAddress,
        payment: ProofOfPayment,
    ) -> Result<()> {
        let key = address.to_record_key();
        let pretty_key = PrettyPrintRecordKey::from(&key).into_owned();
        info!("Validating record payment for {pretty_key}");

        // check if the quote is valid
        let self_peer_id = self.network().peer_id();
        if !payment.verify_for(self_peer_id) {
            warn!("Payment is not valid for record {pretty_key}");
            return Err(Error::InvalidRequest(format!(
                "Payment is not valid for record {pretty_key}"
            )));
        }
        debug!("Payment is valid for record {pretty_key}");

        // verify quote expiration
        if payment.has_expired() {
            warn!("Payment quote has expired for record {pretty_key}");
            return Err(Error::InvalidRequest(format!(
                "Payment quote has expired for record {pretty_key}"
            )));
        }

        // verify the claimed payees are all known to us within the certain range.
        let closest_k_peers = self.network().get_closest_k_value_local_peers().await?;
        let mut payees = payment.payees();
        payees.retain(|peer_id| !closest_k_peers.contains(peer_id));
        if !payees.is_empty() {
            info!("Payment quote has out-of-range payees {payees:?}");
            return Err(Error::InvalidRequest(format!(
                "Payment quote has out-of-range payees {payees:?}"
            )));
        }

        let owned_payment_quotes = payment
            .quotes_by_peer(&self_peer_id)
            .iter()
            .map(|quote| quote.hash())
            .collect();
        // check if payment is valid on chain
        let payments_to_verify = payment.digest();
        debug!("Verifying payment for record {pretty_key}");
        let verify_data_payment_result =
            verify_data_payment(self.evm_network(), owned_payment_quotes, payments_to_verify)
                .await
                .map_err(|e| Error::EvmNetwork(format!("Failed to verify chunk payment: {e}")));
        info!("Data payemnt of record {pretty_key} verification result is {verify_data_payment_result:?}.");
        let reward_amount = verify_data_payment_result?;
        debug!("Payment of {reward_amount:?} is valid for record {pretty_key}");

        // Notify `record_store` that the node received a payment.
        self.network().notify_payment_received();

        #[cfg(feature = "open-metrics")]
        if let Some(metrics_recorder) = self.metrics_recorder() {
            // FIXME: We would reach the MAX if the storecost is scaled up.
            let current_value = metrics_recorder.current_reward_wallet_balance.get();
            let new_value =
                current_value.saturating_add(reward_amount.try_into().unwrap_or(i64::MAX));
            let _ = metrics_recorder
                .current_reward_wallet_balance
                .set(new_value);
        }
        self.events_channel()
            .broadcast(crate::NodeEvent::RewardReceived(
                AttoTokens::from(reward_amount),
                address.clone(),
            ));

        // vdash metric (if modified please notify at https://github.com/happybeing/vdash/issues):
        info!("Total payment of {reward_amount:?} atto tokens accepted for record {pretty_key}");

        // loud mode: print a celebratory message to console
        #[cfg(feature = "loud")]
        {
            println!("🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟   RECEIVED REWARD   🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟");
            println!(
                "Total payment of {reward_amount:?} atto tokens accepted for record {pretty_key}"
            );
            println!("🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟");
        }

        Ok(())
    }

    async fn register_validation(
        &self,
        register: &SignedRegister,
        present_locally: bool,
    ) -> Result<Option<SignedRegister>> {
        // check if register is valid
        let reg_addr = register.address();
        register.verify()?;

        // if we don't have it locally return it
        if !present_locally {
            debug!("Register with addr {reg_addr:?} is valid and doesn't exist locally");
            return Ok(Some(register.to_owned()));
        }
        debug!("Register with addr {reg_addr:?} exists locally, comparing with local version");

        let key = NetworkAddress::from_register_address(*reg_addr).to_record_key();

        // get local register
        let maybe_record = self.network().get_local_record(&key).await?;
        let record = match maybe_record {
            Some(r) => r,
            None => {
                error!("Register with addr {reg_addr:?} already exists locally, but not found in local storage");
                return Err(Error::InvalidRequest(format!(
                    "Register with addr {reg_addr:?} claimed to be existing locally was not found"
                )));
            }
        };
        let local_register: SignedRegister = try_deserialize_record(&record)?;

        // merge the two registers
        let mut merged_register = local_register.clone();
        merged_register.verified_merge(register)?;
        if merged_register == local_register {
            debug!("Register with addr {reg_addr:?} is the same as the local version");
            Ok(None)
        } else {
            debug!("Register with addr {reg_addr:?} is different from the local version");
            Ok(Some(merged_register))
        }
    }

    /// Get the local transactions for the provided `TransactionAddress`
    /// This only fetches the transactions from the local store and does not perform any network operations.
    async fn get_local_transactions(&self, addr: TransactionAddress) -> Result<Vec<Transaction>> {
        // get the local transactions
        let record_key = NetworkAddress::from_transaction_address(addr).to_record_key();
        debug!("Checking for local transactions with key: {record_key:?}");
        let local_record = match self.network().get_local_record(&record_key).await? {
            Some(r) => r,
            None => {
                debug!("Transaction is not present locally: {record_key:?}");
                return Ok(vec![]);
            }
        };

        // deserialize the record and get the transactions
        let local_header = RecordHeader::from_record(&local_record)?;
        let record_kind = local_header.kind;
        if !matches!(record_kind, RecordKind::Transaction) {
            error!("Found a {record_kind} when expecting to find Spend at {addr:?}");
            return Err(NetworkError::RecordKindMismatch(RecordKind::Transaction).into());
        }
        let local_transactions: Vec<Transaction> = try_deserialize_record(&local_record)?;
        Ok(local_transactions)
    }
}
