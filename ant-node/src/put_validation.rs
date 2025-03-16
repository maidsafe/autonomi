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
use ant_evm::ProofOfPayment;
use ant_networking::NetworkError;
use ant_protocol::storage::GraphEntry;
use ant_protocol::{
    storage::{
        try_deserialize_record, try_serialize_record, Chunk, DataTypes, GraphEntryAddress, Pointer,
        PointerAddress, RecordHeader, RecordKind, Scratchpad, ValidationType,
    },
    NetworkAddress, PrettyPrintRecordKey,
};
use libp2p::kad::{Record, RecordKey, K_VALUE};
use xor_name::XorName;

// We retry the payment verification once after waiting this many seconds to rule out the possibility of an EVM node state desync
const RETRY_PAYMENT_VERIFICATION_WAIT_TIME_SECS: u64 = 5;

impl Node {
    /// Validate a record and its payment, and store the record to the RecordStore
    pub(crate) async fn validate_and_store_record(&self, record: Record) -> Result<()> {
        let record_header = RecordHeader::from_record(&record)?;

        match record_header.kind {
            RecordKind::DataWithPayment(DataTypes::Chunk) => {
                let record_key = record.key.clone();
                let (payment, chunk) = try_deserialize_record::<(ProofOfPayment, Chunk)>(&record)?;
                let already_exists = self
                    .validate_key_and_existence(&chunk.network_address(), &record_key)
                    .await?;

                // Validate the payment and that we received what we asked.
                // This stores any payments to disk
                let payment_res = self
                    .payment_for_us_exists_and_is_still_valid(
                        &chunk.network_address(),
                        DataTypes::Chunk,
                        payment.clone(),
                    )
                    .await;

                // Now that we've taken any money passed to us, regardless of the payment's validity,
                // if we already have the data we can return early
                if already_exists {
                    // Client changed to upload to ALL payees, hence no longer need this.
                    // May need again once client change back to upload to just one to save traffic.
                    // self.replicate_valid_fresh_record(
                    //     record_key,
                    //     DataTypes::Chunk,
                    //     ValidationType::Chunk,
                    //     Some(payment),
                    // );

                    // Notify replication_fetcher to mark the attempt as completed.
                    // Send the notification earlier to avoid it got skipped due to:
                    // the record becomes stored during the fetch because of other interleaved process.
                    self.network()
                        .notify_fetch_completed(record.key.clone(), ValidationType::Chunk);

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
                let store_chunk_result = self.store_chunk(&chunk, true);

                if store_chunk_result.is_ok() {
                    Marker::ValidPaidChunkPutFromClient(&PrettyPrintRecordKey::from(&record.key))
                        .log();
                    // Client changed to upload to ALL payees, hence no longer need this.
                    // May need again once client change back to upload to just one to save traffic.
                    // self.replicate_valid_fresh_record(
                    //     record_key,
                    //     DataTypes::Chunk,
                    //     ValidationType::Chunk,
                    //     Some(payment),
                    // );

                    // Notify replication_fetcher to mark the attempt as completed.
                    // Send the notification earlier to avoid it got skipped due to:
                    // the record becomes stored during the fetch because of other interleaved process.
                    self.network()
                        .notify_fetch_completed(record.key.clone(), ValidationType::Chunk);
                }

                store_chunk_result
            }

            RecordKind::DataOnly(DataTypes::Chunk) => {
                error!("Chunk should not be validated at this point");
                Err(Error::InvalidPutWithoutPayment(
                    PrettyPrintRecordKey::from(&record.key).into_owned(),
                ))
            }
            RecordKind::DataWithPayment(DataTypes::Scratchpad) => {
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
                        DataTypes::Scratchpad,
                        payment.clone(),
                    )
                    .await;

                // Finally before we store, lets bail for any payment issues
                payment_res?;

                // Writing records to disk takes time, hence try to execute it first.
                // So that when the replicate target asking for the copy,
                // the node can have a higher chance to respond.
                let store_scratchpad_result = self
                    .validate_and_store_scratchpad_record(
                        scratchpad,
                        record_key.clone(),
                        true,
                        Some(payment),
                    )
                    .await;

                match store_scratchpad_result {
                    // if we're receiving this scratchpad PUT again, and we have been paid,
                    // we eagerly retry replicaiton as it seems like other nodes are having trouble
                    // did not manage to get this scratchpad as yet.
                    Ok(_) | Err(Error::IgnoringOutdatedScratchpadPut) => {
                        let content_hash = XorName::from_content(&record.value);
                        Marker::ValidScratchpadRecordPutFromClient(&PrettyPrintRecordKey::from(
                            &record_key,
                        ))
                        .log();

                        // Notify replication_fetcher to mark the attempt as completed.
                        // Send the notification earlier to avoid it got skipped due to:
                        // the record becomes stored during the fetch because of other interleaved process.
                        self.network().notify_fetch_completed(
                            record_key,
                            ValidationType::NonChunk(content_hash),
                        );
                    }
                    Err(_) => {}
                }

                store_scratchpad_result
            }
            RecordKind::DataOnly(DataTypes::Scratchpad) => {
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
                self.validate_and_store_scratchpad_record(scratchpad, key, true, None)
                    .await
            }
            RecordKind::DataOnly(DataTypes::GraphEntry) => {
                // Transactions should always be paid for
                error!("Transaction should not be validated at this point");
                Err(Error::InvalidPutWithoutPayment(
                    PrettyPrintRecordKey::from(&record.key).into_owned(),
                ))
            }
            RecordKind::DataWithPayment(DataTypes::GraphEntry) => {
                let (payment, graph_entry) =
                    try_deserialize_record::<(ProofOfPayment, GraphEntry)>(&record)?;

                // check if the deserialized value's GraphEntryAddress matches the record's key
                let net_addr = NetworkAddress::from_graph_entry_address(graph_entry.address());
                let key = net_addr.to_record_key();
                let pretty_key = PrettyPrintRecordKey::from(&key);
                if record.key != key {
                    warn!(
                        "Record's key {pretty_key:?} does not match with the value's GraphEntryAddress, ignoring PUT."
                    );
                    return Err(Error::RecordKeyMismatch);
                }

                let already_exists = self.validate_key_and_existence(&net_addr, &key).await?;

                // The GraphEntry may already exist during the replication.
                // The payment shall get deposit to self even the GraphEntry already presents.
                // However, if the GraphEntry is already present, the incoming one shall be
                // appended with the existing one, if content is different.
                if let Err(err) = self
                    .payment_for_us_exists_and_is_still_valid(
                        &net_addr,
                        DataTypes::GraphEntry,
                        payment.clone(),
                    )
                    .await
                {
                    if already_exists {
                        debug!("Payment of the incoming existing GraphEntry {pretty_key:?} having error {err:?}");
                    } else {
                        error!("Payment of the incoming new GraphEntry {pretty_key:?} having error {err:?}");
                        return Err(err);
                    }
                }

                let res = self
                    .validate_merge_and_store_graphentries(vec![graph_entry], &key, true)
                    .await;
                if res.is_ok() {
                    let content_hash = XorName::from_content(&record.value);
                    Marker::ValidGraphEntryPutFromClient(&PrettyPrintRecordKey::from(&record.key))
                        .log();
                    // Client changed to upload to ALL payees, hence no longer need this.
                    // May need again once client change back to upload to just one to save traffic.
                    // self.replicate_valid_fresh_record(
                    //     record.key.clone(),
                    //     DataTypes::GraphEntry,
                    //     ValidationType::NonChunk(content_hash),
                    //     Some(payment),
                    // );

                    // Notify replication_fetcher to mark the attempt as completed.
                    // Send the notification earlier to avoid it got skipped due to:
                    // the record becomes stored during the fetch because of other interleaved process.
                    self.network().notify_fetch_completed(
                        record.key.clone(),
                        ValidationType::NonChunk(content_hash),
                    );
                }
                res
            }
            RecordKind::DataOnly(DataTypes::Pointer) => {
                let pointer = try_deserialize_record::<Pointer>(&record)?;
                let net_addr = NetworkAddress::from_pointer_address(pointer.address());
                let pretty_key = PrettyPrintRecordKey::from(&record.key);
                let already_exists = self
                    .validate_key_and_existence(&net_addr, &record.key)
                    .await?;

                if !already_exists {
                    warn!("Pointer at address: {:?}, key: {:?} does not exist locally, rejecting PUT without payment", pointer.address(), pretty_key);
                    return Err(Error::InvalidPutWithoutPayment(
                        PrettyPrintRecordKey::from(&record.key).into_owned(),
                    ));
                }

                let res = self
                    .validate_and_store_pointer_record(pointer, record.key.clone(), true, None)
                    .await;
                if res.is_ok() {
                    let content_hash = XorName::from_content(&record.value);
                    Marker::ValidPointerPutFromClient(&pretty_key).log();

                    // Notify replication_fetcher to mark the attempt as completed.
                    self.network().notify_fetch_completed(
                        record.key.clone(),
                        ValidationType::NonChunk(content_hash),
                    );
                }
                res
            }
            RecordKind::DataWithPayment(DataTypes::Pointer) => {
                let (payment, pointer) =
                    try_deserialize_record::<(ProofOfPayment, Pointer)>(&record)?;

                let net_addr = NetworkAddress::from_pointer_address(pointer.address());
                let pretty_key = PrettyPrintRecordKey::from(&record.key);
                let already_exists = self
                    .validate_key_and_existence(&net_addr, &record.key)
                    .await?;

                // The pointer may already exist during the replication.
                // The payment shall get deposit to self even if the pointer already exists.
                if let Err(err) = self
                    .payment_for_us_exists_and_is_still_valid(
                        &net_addr,
                        DataTypes::Pointer,
                        payment.clone(),
                    )
                    .await
                {
                    if already_exists {
                        debug!("Payment of the incoming exists pointer {pretty_key:?} having error {err:?}");
                    } else {
                        error!("Payment of the incoming non-exist pointer {pretty_key:?} having error {err:?}");
                        return Err(err);
                    }
                }

                let res = self
                    .validate_and_store_pointer_record(
                        pointer,
                        record.key.clone(),
                        true,
                        Some(payment),
                    )
                    .await;
                if res.is_ok() {
                    let content_hash = XorName::from_content(&record.value);
                    Marker::ValidPointerPutFromClient(&pretty_key).log();

                    // Notify replication_fetcher to mark the attempt as completed.
                    self.network().notify_fetch_completed(
                        record.key.clone(),
                        ValidationType::NonChunk(content_hash),
                    );
                }
                res
            }
        }
    }

    /// Store a pre-validated, and already paid record to the RecordStore
    pub(crate) async fn store_replicated_in_record(&self, record: Record) -> Result<()> {
        debug!(
            "Storing record which was replicated to us {:?}",
            PrettyPrintRecordKey::from(&record.key)
        );
        let record_header = RecordHeader::from_record(&record)?;
        match record_header.kind {
            // A separate flow handles record with payment
            RecordKind::DataWithPayment(_) => {
                warn!("Prepaid record came with Payment, which should be handled in another flow");
                Err(Error::UnexpectedRecordWithPayment(
                    PrettyPrintRecordKey::from(&record.key).into_owned(),
                ))
            }
            RecordKind::DataOnly(DataTypes::Chunk) => {
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

                self.store_chunk(&chunk, false)
            }
            RecordKind::DataOnly(DataTypes::Scratchpad) => {
                let key = record.key.clone();
                let scratchpad = try_deserialize_record::<Scratchpad>(&record)?;
                self.validate_and_store_scratchpad_record(scratchpad, key, false, None)
                    .await
            }
            RecordKind::DataOnly(DataTypes::GraphEntry) => {
                let record_key = record.key.clone();
                let graph_entries = try_deserialize_record::<Vec<GraphEntry>>(&record)?;
                self.validate_merge_and_store_graphentries(graph_entries, &record_key, false)
                    .await
            }
            RecordKind::DataOnly(DataTypes::Pointer) => {
                let pointer = try_deserialize_record::<Pointer>(&record)?;
                let key = record.key.clone();
                self.validate_and_store_pointer_record(pointer, key, false, None)
                    .await
            }
        }
    }

    /// Check key is valid compared to the network name, and if we already have this data or not.
    /// returns true if data already exists locally
    pub(crate) async fn validate_key_and_existence(
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
    pub(crate) fn store_chunk(&self, chunk: &Chunk, is_client_put: bool) -> Result<()> {
        let key = NetworkAddress::from_chunk_address(*chunk.address()).to_record_key();
        let pretty_key = PrettyPrintRecordKey::from(&key).into_owned();

        let record = Record {
            key,
            value: try_serialize_record(&chunk, RecordKind::DataOnly(DataTypes::Chunk))?.to_vec(),
            publisher: None,
            expires: None,
        };

        // finally store the Record directly into the local storage
        self.network().put_local_record(record, is_client_put);

        self.record_metrics(Marker::ValidChunkRecordPutFromNetwork(&pretty_key));

        // TODO: currently ignored, re-enable once start to handle
        // self.events_channel()
        //     .broadcast(crate::NodeEvent::ChunkStored(chunk_addr));

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
        _payment: Option<ProofOfPayment>,
    ) -> Result<()> {
        // owner PK is defined herein, so as long as record key and this match, we're good
        let addr = scratchpad.address();
        let count = scratchpad.counter();
        debug!("Validating and storing scratchpad {addr:?} with count {count}");

        // check if the deserialized value's ScratchpadAddress matches the record's key
        let scratchpad_key = NetworkAddress::ScratchpadAddress(*addr).to_record_key();
        if scratchpad_key != record_key {
            warn!("Record's key does not match with the value's ScratchpadAddress, ignoring PUT.");
            return Err(Error::RecordKeyMismatch);
        }

        // check if the Scratchpad is present locally that we don't have a newer version
        if let Some(local_pad) = self.network().get_local_record(&scratchpad_key).await? {
            let local_pad = try_deserialize_record::<Scratchpad>(&local_pad)?;
            if local_pad.counter() >= scratchpad.counter() {
                warn!("Rejecting Scratchpad PUT with counter less than or equal to the current counter");
                return Err(Error::IgnoringOutdatedScratchpadPut);
            }
        }

        // ensure data integrity
        if !scratchpad.verify_signature() {
            warn!("Rejecting Scratchpad PUT with invalid signature");
            return Err(Error::InvalidScratchpadSignature);
        }

        // ensure the scratchpad is not too big
        if scratchpad.is_too_big() {
            warn!("Rejecting Scratchpad PUT with too big size");
            return Err(Error::ScratchpadTooBig(scratchpad.size()));
        }

        info!(
            "Storing sratchpad {addr:?} with content of {:?} as Record locally",
            scratchpad.encrypted_data_hash()
        );

        let record = Record {
            key: scratchpad_key.clone(),
            value: try_serialize_record(&scratchpad, RecordKind::DataOnly(DataTypes::Scratchpad))?
                .to_vec(),
            publisher: None,
            expires: None,
        };
        self.network()
            .put_local_record(record.clone(), is_client_put);

        let pretty_key = PrettyPrintRecordKey::from(&scratchpad_key);

        self.record_metrics(Marker::ValidScratchpadRecordPutFromNetwork(&pretty_key));

        // Client changed to upload to ALL payees, hence no longer need this.
        // May need again once client change back to upload to just one to save traffic.
        // if is_client_put {
        //     let content_hash = XorName::from_content(&record.value);
        //     // ScratchPad update is a special upload that without payment,
        //     // but must have an existing copy to update.
        //     self.replicate_valid_fresh_record(
        //         scratchpad_key,
        //         DataTypes::Scratchpad,
        //         ValidationType::NonChunk(content_hash),
        //         payment,
        //     );
        // }

        Ok(())
    }

    /// Validate and store `Vec<GraphEntry>` to the RecordStore
    /// If we already have a GraphEntry at this address, the Vec is extended and stored.
    pub(crate) async fn validate_merge_and_store_graphentries(
        &self,
        entries: Vec<GraphEntry>,
        record_key: &RecordKey,
        is_client_put: bool,
    ) -> Result<()> {
        let pretty_key = PrettyPrintRecordKey::from(record_key);
        debug!("Validating GraphEntries before storage at {pretty_key:?}");

        // only keep GraphEntries that match the record key
        let entries_for_key: Vec<GraphEntry> = entries
            .into_iter()
            .filter(|s| {
                // get the record key for the GraphEntry
                let graph_entry_address = s.address();
                let network_address = NetworkAddress::from_graph_entry_address(graph_entry_address);
                let graph_entry_record_key = network_address.to_record_key();
                let graph_entry_pretty = PrettyPrintRecordKey::from(&graph_entry_record_key);
                if &graph_entry_record_key != record_key {
                    warn!("Ignoring GraphEntry for another record key {graph_entry_pretty:?} when verifying: {pretty_key:?}");
                    return false;
                }
                true
            })
            .collect();

        // if we have no GraphEntries to verify, return early
        if entries_for_key.is_empty() {
            warn!("Found no valid GraphEntries to verify upon validation for {pretty_key:?}");
            return Err(Error::InvalidRequest(format!(
                "No GraphEntries to verify when validating {pretty_key:?}"
            )));
        }

        // verify the GraphEntries
        let mut validated_entries: BTreeSet<GraphEntry> = entries_for_key
            .into_iter()
            .filter(|t| t.verify_signature())
            .collect();

        // skip if none are valid
        let addr = match validated_entries.first() {
            None => {
                warn!("Found no validated GraphEntries to store at {pretty_key:?}");
                return Ok(());
            }
            Some(t) => t.address(),
        };

        // add local GraphEntries to the validated GraphEntries, turn to Vec
        let local_entries = self.get_local_graphentries(addr).await?;
        let existing_entry = local_entries.len();
        validated_entries.extend(local_entries.into_iter());
        let validated_entries: Vec<GraphEntry> = validated_entries.into_iter().collect();

        // No need to write to disk if nothing new.
        if existing_entry == validated_entries.len() {
            debug!("No new entry of the GraphEntry {pretty_key:?}");
            return Ok(());
        }

        // store the record into the local storage
        let record = Record {
            key: record_key.clone(),
            value: try_serialize_record(
                &validated_entries,
                RecordKind::DataOnly(DataTypes::GraphEntry),
            )?
            .to_vec(),
            publisher: None,
            expires: None,
        };
        self.network().put_local_record(record, is_client_put);
        debug!("Successfully stored validated GraphEntries at {pretty_key:?}");

        // Just log the multiple GraphEntries
        if validated_entries.len() > 1 {
            debug!(
                "Got multiple GraphEntry(s) of len {} at {pretty_key:?}",
                validated_entries.len()
            );
        }

        self.record_metrics(Marker::ValidGraphEntryRecordPutFromNetwork(&pretty_key));
        Ok(())
    }

    /// Perform validations on the provided `Record`.
    pub(crate) async fn payment_for_us_exists_and_is_still_valid(
        &self,
        address: &NetworkAddress,
        data_type: DataTypes,
        payment: ProofOfPayment,
    ) -> Result<()> {
        let key = address.to_record_key();
        let pretty_key = PrettyPrintRecordKey::from(&key).into_owned();

        // check if the quote is valid
        let self_peer_id = self.network().peer_id();
        if !payment.verify_for(self_peer_id) {
            warn!("Payment is not valid for record {pretty_key}");
            return Err(Error::InvalidRequest(format!(
                "Payment is not valid for record {pretty_key}"
            )));
        }

        // verify quote expiration
        if payment.has_expired() {
            warn!("Payment quote has expired for record {pretty_key}");
            return Err(Error::InvalidRequest(format!(
                "Payment quote has expired for record {pretty_key}"
            )));
        }

        // verify data type matches
        if !payment.verify_data_type(data_type.get_index()) {
            warn!("Payment quote has wrong data type for record {pretty_key}");
            return Err(Error::InvalidRequest(format!(
                "Payment quote has wrong data type for record {pretty_key}"
            )));
        }

        // verify the claimed payees are all known to us within the certain range.
        let mut closest_k_peers = self
            .network()
            .get_close_peers_to_the_target(address.clone(), K_VALUE.get())
            .await?;
        // push self in as the returned list doesn't contain self
        closest_k_peers.push((self_peer_id, Default::default()));
        let mut payees = payment.payees();
        payees.retain(|peer_id| !closest_k_peers.iter().any(|(p, _)| p == peer_id));
        if !payees.is_empty() {
            warn!("Payment quote has out-of-range payees for record {pretty_key}");
            return Err(Error::InvalidRequest(format!(
                "Payment quote has out-of-range payees {payees:?}"
            )));
        }

        let owned_payment_quotes: Vec<_> = payment
            .quotes_by_peer(&self_peer_id)
            .iter()
            .map(|quote| quote.hash())
            .collect();

        // check if payment is valid on chain
        let payments_to_verify = payment.digest();

        let reward_amount = match verify_data_payment(
            self.evm_network(),
            owned_payment_quotes.clone(),
            payments_to_verify.clone(),
        )
        .await
        {
            Ok(amount) => amount,
            Err(e) => {
                warn!("Failed to verify record payment on the first attempt: {e}");
                // Try again, because there could be a possible EVM node desync
                tokio::time::sleep(std::time::Duration::from_secs(
                    RETRY_PAYMENT_VERIFICATION_WAIT_TIME_SECS,
                ))
                .await;
                verify_data_payment(self.evm_network(), owned_payment_quotes, payments_to_verify)
                    .await
                    .inspect_err(|e| {
                        warn!("Failed to verify record payment on the second attempt: {e}");
                    })
                    .map_err(|e| {
                        Error::EvmNetwork(format!("Failed to verify record payment: {e}"))
                    })?
            }
        };

        debug!("Payment of {reward_amount:?} is valid for record {pretty_key}");

        if !reward_amount.is_zero() {
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

            // TODO: currently ignored, re-enable once going to handle this.
            // self.events_channel()
            //     .broadcast(crate::NodeEvent::RewardReceived(
            //         AttoTokens::from(reward_amount),
            //         address.clone(),
            //     ));

            // vdash metric (if modified please notify at https://github.com/happybeing/vdash/issues):
            info!(
                "Total payment of {reward_amount:?} atto tokens accepted for record {pretty_key}"
            );

            // loud mode: print a celebratory message to console
            #[cfg(feature = "loud")]
            {
                println!("🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟   RECEIVED REWARD   🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟");
                println!(
                    "Total payment of {reward_amount:?} atto tokens accepted for record {pretty_key}"
                );
                println!("🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟🌟");
            }
        }

        Ok(())
    }

    /// Get the local GraphEntries for the provided `GraphEntryAddress`
    /// This only fetches the GraphEntries from the local store and does not perform any network operations.
    async fn get_local_graphentries(&self, addr: GraphEntryAddress) -> Result<Vec<GraphEntry>> {
        // get the local GraphEntries
        let record_key = NetworkAddress::from_graph_entry_address(addr).to_record_key();
        debug!("Checking for local GraphEntries with key: {record_key:?}");
        let local_record = match self.network().get_local_record(&record_key).await? {
            Some(r) => r,
            None => {
                debug!("GraphEntry is not present locally: {record_key:?}");
                return Ok(vec![]);
            }
        };

        // deserialize the record and get the GraphEntries
        let local_header = RecordHeader::from_record(&local_record)?;
        let record_kind = local_header.kind;
        if !matches!(record_kind, RecordKind::DataOnly(DataTypes::GraphEntry)) {
            error!("Found a {record_kind} when expecting to find GraphEntry at {addr:?}");
            return Err(NetworkError::RecordKindMismatch(RecordKind::DataOnly(
                DataTypes::GraphEntry,
            ))
            .into());
        }
        let local_entries: Vec<GraphEntry> = try_deserialize_record(&local_record)?;
        Ok(local_entries)
    }

    /// Get the local Pointer for the provided `PointerAddress`
    /// This only fetches the Pointer from the local store and does not perform any network operations.
    /// If the local Pointer is not present or corrupted, returns `None`.
    async fn get_local_pointer(&self, addr: PointerAddress) -> Option<Pointer> {
        // get the local Pointer
        let record_key = NetworkAddress::from_pointer_address(addr).to_record_key();
        debug!("Checking for local Pointer with key: {record_key:?}");
        let local_record = match self.network().get_local_record(&record_key).await {
            Ok(Some(r)) => r,
            Ok(None) => {
                debug!("Pointer is not present locally: {record_key:?}");
                return None;
            }
            Err(e) => {
                error!("Failed to get Pointer record at {addr:?}: {e}");
                return None;
            }
        };

        // deserialize the record and get the Pointer
        let local_header = match RecordHeader::from_record(&local_record) {
            Ok(h) => h,
            Err(_) => {
                error!("Failed to deserialize Pointer record at {addr:?}");
                return None;
            }
        };
        let record_kind = local_header.kind;
        if !matches!(record_kind, RecordKind::DataOnly(DataTypes::Pointer)) {
            error!("Found a {record_kind} when expecting to find Pointer at {addr:?}");
            return None;
        }
        let local_pointer: Pointer = match try_deserialize_record(&local_record) {
            Ok(p) => p,
            Err(_) => {
                error!("Failed to deserialize Pointer record at {addr:?}");
                return None;
            }
        };
        Some(local_pointer)
    }

    /// Validate and store a pointer record
    pub(crate) async fn validate_and_store_pointer_record(
        &self,
        pointer: Pointer,
        key: RecordKey,
        is_client_put: bool,
        _payment: Option<ProofOfPayment>,
    ) -> Result<()> {
        // Verify the pointer's signature
        if !pointer.verify_signature() {
            warn!("Pointer signature verification failed");
            return Err(Error::InvalidSignature);
        }

        // Check if the pointer's address matches the record key
        let net_addr = NetworkAddress::from_pointer_address(pointer.address());
        if key != net_addr.to_record_key() {
            warn!("Pointer address does not match record key");
            return Err(Error::RecordKeyMismatch);
        }

        // Keep the pointer with the highest counter
        if let Some(local_pointer) = self.get_local_pointer(pointer.address()).await {
            if pointer.counter() <= local_pointer.counter() {
                info!(
                    "Ignoring Pointer PUT at {key:?} with counter less than or equal to the current counter ({} <= {})",
                    pointer.counter(),
                    local_pointer.counter()
                );
                return Ok(());
            }
        }

        // Store the pointer
        let record = Record {
            key: key.clone(),
            value: try_serialize_record(&pointer, RecordKind::DataOnly(DataTypes::Pointer))?
                .to_vec(),
            publisher: None,
            expires: None,
        };
        self.network()
            .put_local_record(record.clone(), is_client_put);

        // Client changed to upload to ALL payees, hence no longer need this.
        // May need again once client change back to upload to just one to save traffic.
        // if is_client_put {
        //     let content_hash = XorName::from_content(&record.value);
        //     self.replicate_valid_fresh_record(
        //         key.clone(),
        //         DataTypes::Pointer,
        //         ValidationType::NonChunk(content_hash),
        //         payment,
        //     );
        // }

        info!("Successfully stored Pointer record at {key:?}");
        Ok(())
    }
}
