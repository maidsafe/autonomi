// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.
#![allow(clippy::mutable_key_type)] // for the Bytes in NetworkAddress

use crate::cmd::LocalSwarmCmd;
use crate::network_builder::MAX_PACKET_SIZE;
use crate::send_local_swarm_cmd;
use crate::time::{spawn, Instant};
use crate::{event::NetworkEvent, log_markers::Marker};
use aes_gcm_siv::{
    aead::{Aead, KeyInit},
    Aes256GcmSiv, Key as AesKey, Nonce,
};
use ant_evm::QuotingMetrics;
use ant_protocol::{
    storage::{DataTypes, RecordHeader, RecordKind, ValidationType},
    NetworkAddress, PrettyPrintRecordKey,
};
use hkdf::Hkdf;
use itertools::Itertools;
use libp2p::{
    identity::PeerId,
    kad::{
        store::{Error, RecordStore, Result},
        KBucketDistance as Distance, ProviderRecord, Record, RecordKey as Key,
    },
};
#[cfg(feature = "open-metrics")]
use prometheus_client::metrics::gauge::Gauge;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
    vec,
};
use tokio::{sync::mpsc, time::Duration};
use walkdir::{DirEntry, WalkDir};
use xor_name::XorName;

// A GraphEntry record is at the size of 4KB roughly.
// Given chunk record is maxed at size of 4MB.
// During Beta phase, it's almost one GraphEntry per chunk,
// which makes the average record size is around 2MB.
// Given we are targeting node size to be 32GB,
// this shall allow around 16K records.
const MAX_RECORDS_COUNT: usize = 16 * 1024;

/// The maximum number of records to cache in memory.
const MAX_RECORDS_CACHE_SIZE: usize = 25;

/// File name of the recorded historical quoting metrics.
const HISTORICAL_QUOTING_METRICS_FILENAME: &str = "historic_quoting_metrics";

/// Defines when the entries inside the cache shall be pruned to free space up.
/// Shall be two times of the PERIODIC_REPLICATION_INTERVAL_MAX_S
const CACHE_TIMEOUT: Duration = Duration::from_secs(360);

fn derive_aes256gcm_siv_from_seed(seed: &[u8; 16]) -> (Aes256GcmSiv, [u8; 4]) {
    // shall be unique for purpose.
    let salt = b"autonomi_record_store";

    let hk = Hkdf::<Sha256>::new(Some(salt), seed);

    let mut okm = [0u8; 32];
    hk.expand(b"", &mut okm)
        .expect("32 bytes is a valid length for HKDF output");

    let seeded_key = AesKey::<Aes256GcmSiv>::from_slice(&okm);

    let mut nonce_starter = [0u8; 4];
    let bytes_to_copy = seed.len().min(nonce_starter.len());
    nonce_starter[..bytes_to_copy].copy_from_slice(&seed[..bytes_to_copy]);

    trace!("seeded_key is {seeded_key:?}  nonce_starter is {nonce_starter:?}");

    (Aes256GcmSiv::new(seeded_key), nonce_starter)
}

/// FIFO simple cache of records to reduce read times
struct RecordCache {
    records_cache: HashMap<Key, (Record, SystemTime)>,
    cache_size: usize,
    cache_timeout: Duration,
}

impl RecordCache {
    fn new(cache_size: usize, cache_timeout: Duration) -> Self {
        RecordCache {
            records_cache: HashMap::new(),
            cache_size,
            cache_timeout,
        }
    }

    fn remove(&mut self, key: &Key) -> Option<(Record, SystemTime)> {
        self.records_cache.remove(key)
    }

    fn get(&self, key: &Key) -> Option<&(Record, SystemTime)> {
        self.records_cache.get(key)
    }

    fn push_back(&mut self, key: Key, record: Record) {
        self.free_up_space();

        let _ = self
            .records_cache
            .insert(key, (record, SystemTime::now() + self.cache_timeout));
    }

    fn free_up_space(&mut self) {
        let current = SystemTime::now();
        // Remove outdated entries first
        self.records_cache
            .retain(|_key, (_record, timestamp)| *timestamp > current);

        while self.records_cache.len() >= self.cache_size {
            self.remove_oldest_entry()
        }
    }

    fn remove_oldest_entry(&mut self) {
        let mut oldest_timestamp = SystemTime::now() + self.cache_timeout;
        let mut key_to_remove = None;

        for (key, (_record, timestamp)) in self.records_cache.iter() {
            if *timestamp < oldest_timestamp {
                oldest_timestamp = *timestamp;
                key_to_remove = Some(key.clone());
            }
        }

        if let Some(key) = key_to_remove {
            let _ = self.records_cache.remove(&key);
        }
    }
}

/// A `RecordStore` that stores records on disk.
pub struct NodeRecordStore {
    /// The address of the peer owning the store
    local_address: NetworkAddress,
    /// The configuration of the store.
    config: NodeRecordStoreConfig,
    /// Main records store remains unchanged for compatibility
    records: HashMap<Key, (NetworkAddress, ValidationType, DataTypes)>,
    /// Additional index organizing records by distance
    records_by_distance: BTreeMap<Distance, Key>,
    /// FIFO simple cache of records to reduce read times
    records_cache: RecordCache,
    /// Send network events to the node layer.
    network_event_sender: mpsc::Sender<NetworkEvent>,
    /// Send cmds to the network layer. Used to interact with self in an async fashion.
    local_swarm_cmd_sender: mpsc::Sender<LocalSwarmCmd>,
    /// ilog2 distance range of responsible records
    /// AKA: how many buckets of data do we consider "close"
    /// None means accept all records.
    responsible_distance_range: Option<Distance>,
    #[cfg(feature = "open-metrics")]
    /// Used to report the number of records held by the store to the metrics server.
    record_count_metric: Option<Gauge>,
    /// Counting how many times got paid
    received_payment_count: usize,
    /// Encyption cipher for the records, randomly generated at node startup
    /// Plus a 4 byte nonce starter
    encryption_details: (Aes256GcmSiv, [u8; 4]),
    /// Time that this record_store got started
    timestamp: SystemTime,
    /// Farthest record to self
    farthest_record: Option<(Key, Distance)>,
}

/// Configuration for a `DiskBackedRecordStore`.
#[derive(Debug, Clone)]
pub struct NodeRecordStoreConfig {
    /// The directory where the records are stored.
    pub storage_dir: PathBuf,
    /// The directory where the historic quote to be stored
    /// (normally to be the parent dir of the storage_dir)
    pub historic_quote_dir: PathBuf,
    /// The maximum number of records.
    pub max_records: usize,
    /// The maximum size of record values, in bytes.
    pub max_value_bytes: usize,
    /// The maximum number of records to cache in memory.
    pub records_cache_size: usize,
    /// The seed to generate record_store encryption_details
    pub encryption_seed: [u8; 16],
}

impl Default for NodeRecordStoreConfig {
    fn default() -> Self {
        let historic_quote_dir = std::env::temp_dir();
        Self {
            storage_dir: historic_quote_dir.clone(),
            historic_quote_dir,
            max_records: MAX_RECORDS_COUNT,
            max_value_bytes: MAX_PACKET_SIZE,
            records_cache_size: MAX_RECORDS_CACHE_SIZE,
            encryption_seed: [0u8; 16],
        }
    }
}

/// Generate an encryption nonce for a given record key and nonce_starter bytes.
fn generate_nonce_for_record(nonce_starter: &[u8; 4], key: &Key) -> Nonce {
    let mut nonce_bytes = nonce_starter.to_vec();
    nonce_bytes.extend_from_slice(key.as_ref());
    // Ensure the final nonce is exactly 96 bits long by padding or truncating as necessary
    // https://crypto.stackexchange.com/questions/26790/how-bad-it-is-using-the-same-iv-twice-with-aes-gcm
    nonce_bytes.resize(12, 0); // 12 (u8) * 8 = 96 bits
    Nonce::from_iter(nonce_bytes)
}

#[derive(Clone, Serialize, Deserialize)]
struct HistoricQuotingMetrics {
    received_payment_count: usize,
    timestamp: SystemTime,
}

impl NodeRecordStore {
    /// If a directory for our node already exists, repopulate the records from the files in the dir
    fn update_records_from_an_existing_store(
        config: &NodeRecordStoreConfig,
        encryption_details: &(Aes256GcmSiv, [u8; 4]),
    ) -> HashMap<Key, (NetworkAddress, ValidationType, DataTypes)> {
        let process_entry = |entry: &DirEntry| -> _ {
            let path = entry.path();
            if path.is_file() {
                debug!("Existing record found: {path:?}");
                // if we've got a file, lets try and read it
                let filename = match path.file_name().and_then(|n| n.to_str()) {
                    Some(file_name) => file_name,
                    None => {
                        // warn and remove this file as it's not a valid record
                        warn!(
                            "Found a file in the storage dir that is not a valid record: {:?}",
                            path
                        );
                        if let Err(e) = fs::remove_file(path) {
                            warn!(
                                "Failed to remove invalid record file from storage dir: {:?}",
                                e
                            );
                        }
                        return None;
                    }
                };
                // get the record key from the filename
                let key = Self::get_data_from_filename(filename)?;
                let record = match fs::read(path) {
                    Ok(bytes) => {
                        // and the stored record
                        if let Some(record) =
                            Self::get_record_from_bytes(bytes, &key, encryption_details)
                        {
                            record
                        } else {
                            // This will be due to node restart, result in different encrypt_detail.
                            // Hence need to clean up the old copy.
                            info!("Failed to decrypt record from file {filename:?}, clean it up.");
                            if let Err(e) = fs::remove_file(path) {
                                warn!(
                                    "Failed to remove outdated record file {filename:?} from storage dir: {:?}",
                                    e
                                );
                            }
                            return None;
                        }
                    }
                    Err(err) => {
                        error!("Error while reading file. filename: {filename}, error: {err:?}");
                        return None;
                    }
                };

                match RecordHeader::get_data_type(&record) {
                    Ok(data_type) => {
                        let validate_type = match data_type {
                            DataTypes::Chunk => ValidationType::Chunk,
                            _ => {
                                let xorname_hash = XorName::from_content(&record.value);
                                ValidationType::NonChunk(xorname_hash)
                            }
                        };

                        let address = NetworkAddress::from(&key);
                        info!("Existing record {address:?} loaded from: {path:?}");
                        return Some((key, (address, validate_type, data_type)));
                    }
                    Err(error) => {
                        warn!(
                            "Failed to parse record type of record {filename:?}: {:?}",
                            error
                        );
                        // In correct decryption using different key could result in this.
                        // In that case, a cleanup shall be carried out.
                        if let Err(e) = fs::remove_file(path) {
                            warn!(
                                "Failed to remove invalid record file {filename:?} from storage dir: {:?}",
                                e
                            );
                        }
                        return None;
                    }
                }
            }
            None
        };

        info!("Attempting to repopulate records from existing store...");
        let records = WalkDir::new(&config.storage_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .collect_vec()
            .par_iter()
            .filter_map(process_entry)
            .collect();
        records
    }

    /// If quote_metrics file already exists, using the existing parameters.
    fn restore_quoting_metrics(storage_dir: &Path) -> Option<HistoricQuotingMetrics> {
        let file_path = storage_dir.join(HISTORICAL_QUOTING_METRICS_FILENAME);

        if let Ok(file) = fs::File::open(file_path) {
            if let Ok(quoting_metrics) = rmp_serde::from_read(&file) {
                return Some(quoting_metrics);
            }
        }

        None
    }

    fn flush_historic_quoting_metrics(&self) {
        let file_path = self
            .config
            .historic_quote_dir
            .join(HISTORICAL_QUOTING_METRICS_FILENAME);

        let historic_quoting_metrics = HistoricQuotingMetrics {
            received_payment_count: self.received_payment_count,
            timestamp: self.timestamp,
        };

        spawn(async move {
            if let Ok(mut file) = fs::File::create(file_path) {
                let mut serialiser = rmp_serde::encode::Serializer::new(&mut file);
                let _ = historic_quoting_metrics.serialize(&mut serialiser);
            }
        });
    }

    /// Creates a new `DiskBackedStore` with the given configuration.
    pub fn with_config(
        local_id: PeerId,
        config: NodeRecordStoreConfig,
        network_event_sender: mpsc::Sender<NetworkEvent>,
        swarm_cmd_sender: mpsc::Sender<LocalSwarmCmd>,
        #[cfg(feature = "open-metrics")] record_count_metric: Option<Gauge>,
    ) -> Self {
        info!("Using encryption_seed of {:?}", config.encryption_seed);
        let encryption_details = derive_aes256gcm_siv_from_seed(&config.encryption_seed);

        // Recover the quoting_metrics first, as the historical file will be cleaned by
        // the later on update_records_from_an_existing_store function
        let (received_payment_count, timestamp) = if let Some(historic_quoting_metrics) =
            Self::restore_quoting_metrics(&config.historic_quote_dir)
        {
            (
                historic_quoting_metrics.received_payment_count,
                historic_quoting_metrics.timestamp,
            )
        } else {
            (0, SystemTime::now())
        };

        let records = Self::update_records_from_an_existing_store(&config, &encryption_details);
        let local_address = NetworkAddress::from(local_id);

        // Initialize records_by_distance
        let mut records_by_distance: BTreeMap<Distance, Key> = BTreeMap::new();
        for (key, (addr, _record_type, _data_type)) in records.iter() {
            let distance = &local_address.distance(addr);
            let _ = records_by_distance.insert(*distance, key.clone());
        }

        let cache_size = config.records_cache_size;
        let mut record_store = NodeRecordStore {
            local_address,
            config,
            records,
            records_by_distance,
            records_cache: RecordCache::new(cache_size, CACHE_TIMEOUT),
            network_event_sender,
            local_swarm_cmd_sender: swarm_cmd_sender,
            responsible_distance_range: None,
            #[cfg(feature = "open-metrics")]
            record_count_metric,
            received_payment_count,
            encryption_details,
            timestamp,
            farthest_record: None,
        };

        record_store.farthest_record = record_store.calculate_farthest();

        record_store.flush_historic_quoting_metrics();

        #[cfg(feature = "open-metrics")]
        if let Some(metric) = &record_store.record_count_metric {
            let _ = metric.set(record_store.records.len() as i64);
        }

        record_store
    }

    /// Returns the current responsible distance range.
    pub(crate) fn get_responsible_distance_range(&self) -> Option<Distance> {
        self.responsible_distance_range
    }

    // Converts a Key into a Hex string.
    fn generate_filename(key: &Key) -> String {
        hex::encode(key.as_ref())
    }

    // Converts a Hex string back into a Key.
    fn get_data_from_filename(hex_str: &str) -> Option<Key> {
        match hex::decode(hex_str) {
            Ok(bytes) => Some(Key::from(bytes)),
            Err(error) => {
                error!("Error decoding hex string: {:?}", error);
                None
            }
        }
    }

    /// Upon read perform any data transformations required to return a `Record`.
    fn get_record_from_bytes<'a>(
        bytes: Vec<u8>,
        key: &Key,
        encryption_details: &(Aes256GcmSiv, [u8; 4]),
    ) -> Option<Cow<'a, Record>> {
        let (cipher, nonce_starter) = encryption_details;
        let nonce = generate_nonce_for_record(nonce_starter, key);

        match cipher.decrypt(&nonce, bytes.as_slice()) {
            Ok(value) => {
                let record = Record {
                    key: key.clone(),
                    value,
                    publisher: None,
                    expires: None,
                };
                Some(Cow::Owned(record))
            }
            Err(error) => {
                error!("Error while decrypting record. key: {key:?}: {error:?}");
                None
            }
        }
    }

    fn read_from_disk<'a>(
        encryption_details: &(Aes256GcmSiv, [u8; 4]),
        key: &Key,
        storage_dir: &Path,
    ) -> Option<Cow<'a, Record>> {
        let start = Instant::now();
        let filename = Self::generate_filename(key);

        let file_path = storage_dir.join(&filename);

        // we should only be reading if we know the record is written to disk properly
        match fs::read(file_path) {
            Ok(bytes) => {
                // vdash metric (if modified please notify at https://github.com/happybeing/vdash/issues):
                info!(
                    "Retrieved record from disk! filename: {filename} after {:?}",
                    start.elapsed()
                );

                Self::get_record_from_bytes(bytes, key, encryption_details)
            }
            Err(err) => {
                error!("Error while reading file. filename: {filename}, error: {err:?}");
                None
            }
        }
    }

    // Returns the farthest record_key to self.
    pub fn get_farthest(&self) -> Option<Key> {
        if let Some((ref key, _distance)) = self.farthest_record {
            Some(key.clone())
        } else {
            None
        }
    }

    // Calculates the farthest record_key to self.
    fn calculate_farthest(&self) -> Option<(Key, Distance)> {
        // sort records by distance to our local key
        let mut sorted_records: Vec<_> = self.records.keys().collect();
        sorted_records.sort_by_key(|key| {
            let addr = NetworkAddress::from(*key);
            self.local_address.distance(&addr)
        });

        if let Some(key) = sorted_records.last() {
            let addr = NetworkAddress::from(*key);
            Some(((*key).clone(), self.local_address.distance(&addr)))
        } else {
            None
        }
    }

    /// Prune the records in the store to ensure that we free up space
    /// for the incoming record.
    /// Returns Ok if the record can be stored because it is closer to the local peer
    /// or we are not full.
    ///
    /// Err MaxRecords if we cannot store as it's farther than the farthest data we have
    fn prune_records_if_needed(&mut self, incoming_record_key: &Key) -> Result<()> {
        // we're not full, so we don't need to prune
        if self.records.len() < self.config.max_records {
            return Ok(());
        }

        if let Some((farthest_record, farthest_record_distance)) = self.farthest_record.clone() {
            // if the incoming record is farther than the farthest record, we can't store it
            if farthest_record_distance
                < self
                    .local_address
                    .distance(&NetworkAddress::from(incoming_record_key))
            {
                return Err(Error::MaxRecords);
            }

            info!(
                "Record {:?} will be pruned to free up space for new records",
                PrettyPrintRecordKey::from(&farthest_record)
            );
            self.remove(&farthest_record);
        }

        Ok(())
    }

    // When the accumulated record copies exceeds the `expotional pricing point` (max_records * 0.1)
    // those `out of range` records shall be cleaned up.
    // This is to avoid :
    //   * holding too many irrelevant record, which occupies disk space
    //   * `over-quoting` during restart, when RT is not fully populated,
    //     result in mis-calculation of relevant records.
    pub fn cleanup_irrelevant_records(&mut self) {
        let accumulated_records = self.records.len();
        if accumulated_records < MAX_RECORDS_COUNT / 10 {
            return;
        }

        let responsible_distance = if let Some(distance) = self.responsible_distance_range {
            distance
        } else {
            return;
        };

        // Collect keys to remove from buckets beyond our range
        let keys_to_remove: Vec<Key> = self
            .records_by_distance
            .range(responsible_distance..)
            .map(|(_distance, key)| key.clone())
            .collect();

        let keys_to_remove_len = keys_to_remove.len();

        // Remove collected keys
        for key in keys_to_remove {
            self.remove(&key);
        }

        info!("Cleaned up {} unrelevant records, among the original {accumulated_records} accumulated_records",
        keys_to_remove_len);
    }
}

impl NodeRecordStore {
    /// Returns `true` if the `Key` is present locally
    pub(crate) fn contains(&self, key: &Key) -> bool {
        self.records.contains_key(key)
    }

    /// Returns the set of `NetworkAddress::RecordKey` held by the store
    /// Use `record_addresses_ref` to get a borrowed type
    pub(crate) fn record_addresses(&self) -> HashMap<NetworkAddress, ValidationType> {
        self.records
            .iter()
            .map(|(_record_key, (addr, record_type, _data_type))| {
                (addr.clone(), record_type.clone())
            })
            .collect()
    }

    /// Returns the reference to the set of `NetworkAddress::RecordKey` held by the store
    pub(crate) fn record_addresses_ref(
        &self,
    ) -> &HashMap<Key, (NetworkAddress, ValidationType, DataTypes)> {
        &self.records
    }

    /// The follow up to `put_verified`, this only registers the RecordKey
    /// in the RecordStore records set. After this it should be safe
    /// to return the record as stored.
    pub(crate) fn mark_as_stored(
        &mut self,
        key: Key,
        validate_type: ValidationType,
        data_type: DataTypes,
    ) {
        let addr = NetworkAddress::from(&key);
        let distance = self.local_address.distance(&addr);

        // Update main records store
        self.records
            .insert(key.clone(), (addr.clone(), validate_type, data_type));

        #[cfg(feature = "open-metrics")]
        if let Some(metric) = &self.record_count_metric {
            let _ = metric.set(self.records.len() as i64);
        }

        // Update bucket index
        let _ = self.records_by_distance.insert(distance, key.clone());

        // Update farthest record if needed (unchanged)
        if let Some((_farthest_record, farthest_record_distance)) = self.farthest_record.clone() {
            if distance > farthest_record_distance {
                self.farthest_record = Some((key, distance));
            }
        } else {
            self.farthest_record = Some((key, distance));
        }
    }

    /// Prepare record bytes for storage
    /// This will encrypt the record for storage
    fn prepare_record_bytes(
        record: Record,
        encryption_details: (Aes256GcmSiv, [u8; 4]),
    ) -> Option<Vec<u8>> {
        let (cipher, nonce_starter) = encryption_details;
        let nonce = generate_nonce_for_record(&nonce_starter, &record.key);

        match cipher.encrypt(&nonce, record.value.as_ref()) {
            Ok(value) => Some(value),
            Err(error) => {
                warn!(
                    "Failed to encrypt record {:?} : {error:?}",
                    PrettyPrintRecordKey::from(&record.key),
                );
                None
            }
        }
    }

    /// Warning: Write's a `Record` to disk without validation
    /// Should be used in context where the `Record` is trusted
    ///
    /// The record is marked as written to disk once `mark_as_stored` is called,
    /// this avoids us returning half-written data or registering it as stored before it is.
    pub(crate) fn put_verified(
        &mut self,
        r: Record,
        record_type: ValidationType,
        is_client_put: bool,
    ) -> Result<()> {
        let key = &r.key;
        let record_key = PrettyPrintRecordKey::from(&r.key).into_owned();
        debug!("PUTting a verified Record: {record_key:?}");

        // if cache already has the record :
        //   * if with same content, do nothing and return early
        //   * if with different content, remove the existing one
        if let Some((existing_record, _timestamp)) = self.records_cache.remove(key) {
            if existing_record.value == r.value {
                // we actually just want to keep what we have, and can assume it's been stored properly.

                // so we put it back in the cache
                self.records_cache.push_back(key.clone(), existing_record);
                // and exit early.
                return Ok(());
            }
        }

        // Only cash the record that put by client. For a quick response to the ChunkProof check.
        if is_client_put {
            self.records_cache.push_back(key.clone(), r.clone());
        }

        self.prune_records_if_needed(key)?;

        let filename = Self::generate_filename(key);
        let file_path = self.config.storage_dir.join(&filename);

        let encryption_details = self.encryption_details.clone();
        let cloned_cmd_sender = self.local_swarm_cmd_sender.clone();

        let record_key2 = record_key.clone();
        spawn(async move {
            let key = r.key.clone();
            let data_type = match RecordHeader::get_data_type(&r) {
                Ok(data_type) => data_type,
                Err(err) => {
                    error!(
                        "Error get data_type of record {record_key2:?} filename: {filename}, error: {err:?}"
                    );
                    return;
                }
            };
            if let Some(bytes) = Self::prepare_record_bytes(r, encryption_details) {
                let cmd = match fs::write(&file_path, bytes) {
                    Ok(_) => {
                        // vdash metric (if modified please notify at https://github.com/happybeing/vdash/issues):
                        info!("Wrote record {record_key2:?} to disk! filename: {filename}");

                        LocalSwarmCmd::AddLocalRecordAsStored {
                            key,
                            record_type,
                            data_type,
                        }
                    }
                    Err(err) => {
                        error!(
                        "Error writing record {record_key2:?} filename: {filename}, error: {err:?}"
                    );
                        LocalSwarmCmd::RemoveFailedLocalRecord { key }
                    }
                };

                send_local_swarm_cmd(cloned_cmd_sender, cmd);
            }
        });

        Ok(())
    }

    /// Return the quoting metrics used to calculate the cost of storing a record
    /// and whether the record is already stored locally
    pub(crate) fn quoting_metrics(
        &self,
        key: &Key,
        data_type: u32,
        data_size: usize,
        network_size: Option<u64>,
    ) -> (QuotingMetrics, bool) {
        let records_stored = self.records.len();
        let records_per_type = self.records_per_type();

        let live_time = if let Ok(elapsed) = self.timestamp.elapsed() {
            elapsed.as_secs()
        } else {
            0
        };

        let mut quoting_metrics = QuotingMetrics {
            data_type,
            data_size,
            close_records_stored: records_stored,
            records_per_type,
            max_records: self.config.max_records,
            received_payment_count: self.received_payment_count,
            live_time,
            network_density: None,
            network_size,
        };

        if let Some(distance_range) = self.responsible_distance_range {
            let relevant_records = self.get_records_within_distance_range(distance_range);

            // The `responsible_range` is the network density
            quoting_metrics.network_density = Some(distance_range.0.to_big_endian());

            quoting_metrics.close_records_stored = relevant_records;
        } else {
            info!("Basing cost of _total_ records stored.");
        };

        // vdash metric (if modified please notify at https://github.com/happybeing/vdash/issues):
        info!("Quoting_metrics {quoting_metrics:?}");

        let is_stored = self.contains(key);
        (quoting_metrics, is_stored)
    }

    /// Notify the node received a payment.
    pub(crate) fn payment_received(&mut self) {
        self.received_payment_count = self.received_payment_count.saturating_add(1);

        self.flush_historic_quoting_metrics();
    }

    /// Calculate how many records are stored within a distance range
    pub fn get_records_within_distance_range(&self, range: Distance) -> usize {
        let within_range = self
            .records_by_distance
            .range(..range)
            .collect::<Vec<_>>()
            .len();

        Marker::CloseRecordsLen(within_range).log();

        within_range
    }

    /// Setup the distance range.
    pub(crate) fn set_responsible_distance_range(&mut self, responsible_distance: Distance) {
        self.responsible_distance_range = Some(responsible_distance);
    }

    fn records_per_type(&self) -> Vec<(u32, u32)> {
        let mut map = BTreeMap::new();
        for (_, _, data_type) in self.records.values() {
            *map.entry(data_type.get_index()).or_insert(0) += 1;
        }
        map.into_iter().collect()
    }
}

impl RecordStore for NodeRecordStore {
    type RecordsIter<'a> = vec::IntoIter<Cow<'a, Record>>;
    type ProvidedIter<'a> = vec::IntoIter<Cow<'a, ProviderRecord>>;

    fn get(&self, k: &Key) -> Option<Cow<'_, Record>> {
        // When a client calls GET, the request is forwarded to the nodes until one node returns
        // with the record. Thus a node can be bombarded with GET reqs for random keys. These can be safely
        // ignored if we don't have the record locally.
        let key = PrettyPrintRecordKey::from(k);

        let cached_record = self.records_cache.get(k);
        // first return from FIFO cache if existing there
        if let Some((record, _timestamp)) = cached_record {
            return Some(Cow::Owned(record.clone()));
        }

        if !self.records.contains_key(k) {
            debug!("Record not found locally: {key:?}");
            return None;
        }

        debug!("GET request for Record key: {key}");

        Self::read_from_disk(&self.encryption_details, k, &self.config.storage_dir)
    }

    fn put(&mut self, record: Record) -> Result<()> {
        let record_key = PrettyPrintRecordKey::from(&record.key);

        if record.value.len() >= self.config.max_value_bytes {
            warn!(
                "Record {record_key:?} not stored. Value too large: {} bytes",
                record.value.len()
            );
            return Err(Error::ValueTooLarge);
        }

        // Record with payment shall always get passed further
        // to allow the payment to be taken and credit into own wallet.
        match RecordHeader::from_record(&record) {
            Ok(record_header) => {
                match record_header.kind {
                    RecordKind::DataWithPayment(_) => {
                        debug!("Record {record_key:?} with payment shall always be processed.");
                    }
                    // Shall not use wildcard, to avoid mis-match during enum update.
                    RecordKind::DataOnly(_) => {
                        // Chunk with existing key do not to be stored again.
                        // Others with same content_hash do not to be stored again,
                        // otherwise shall be passed further to allow different version of nonchunk
                        // to be detected or updated.
                        match self.records.get(&record.key) {
                            Some((_addr, ValidationType::Chunk, _data_type)) => {
                                debug!("Chunk {record_key:?} already exists.");
                                return Ok(());
                            }
                            Some((
                                _addr,
                                ValidationType::NonChunk(existing_content_hash),
                                _data_type,
                            )) => {
                                let content_hash = XorName::from_content(&record.value);
                                if content_hash == *existing_content_hash {
                                    debug!("A non-chunk record {record_key:?} with same content_hash {content_hash:?} already exists.");
                                    return Ok(());
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            Err(err) => {
                error!("For record {record_key:?}, failed to parse record_header {err:?}");
                return Ok(());
            }
        }

        debug!("Unverified Record {record_key:?} try to validate and store");
        let event_sender = self.network_event_sender.clone();
        // push the event off thread so as to be non-blocking
        let _handle = spawn(async move {
            if let Err(error) = event_sender
                .send(NetworkEvent::UnverifiedRecord(record))
                .await
            {
                error!("SwarmDriver failed to send event: {}", error);
            }
        });

        Ok(())
    }

    fn remove(&mut self, k: &Key) {
        // Remove from main store
        if let Some((addr, _, _)) = self.records.remove(k) {
            let distance = self.local_address.distance(&addr);
            let _ = self.records_by_distance.remove(&distance);
        }

        self.records_cache.remove(k);

        #[cfg(feature = "open-metrics")]
        if let Some(metric) = &self.record_count_metric {
            let _ = metric.set(self.records.len() as i64);
        }

        if let Some((farthest_record, _)) = self.farthest_record.clone() {
            if farthest_record == *k {
                self.farthest_record = self.calculate_farthest();
            }
        }

        let filename = Self::generate_filename(k);
        let file_path = self.config.storage_dir.join(&filename);

        let _handle = spawn(async move {
            match fs::remove_file(file_path) {
                Ok(_) => {
                    info!("Removed record from disk! filename: {filename}");
                }
                Err(err) => {
                    error!("Error while removing file. filename: {filename}, error: {err:?}");
                }
            }
        });
    }

    fn records(&self) -> Self::RecordsIter<'_> {
        // the records iter is used only during kad replication which is turned off
        vec![].into_iter()
    }

    fn add_provider(&mut self, _record: ProviderRecord) -> Result<()> {
        // ProviderRecords are not used currently
        Ok(())
    }

    fn providers(&self, _key: &Key) -> Vec<ProviderRecord> {
        // ProviderRecords are not used currently
        vec![]
    }

    fn provided(&self) -> Self::ProvidedIter<'_> {
        // ProviderRecords are not used currently
        vec![].into_iter()
    }

    fn remove_provider(&mut self, _key: &Key, _provider: &PeerId) {
        // ProviderRecords are not used currently
    }
}

/// A place holder RecordStore impl for the client that does nothing
#[derive(Default, Debug)]
pub struct ClientRecordStore {}

impl RecordStore for ClientRecordStore {
    type RecordsIter<'a> = vec::IntoIter<Cow<'a, Record>>;
    type ProvidedIter<'a> = vec::IntoIter<Cow<'a, ProviderRecord>>;

    fn get(&self, _k: &Key) -> Option<Cow<'_, Record>> {
        None
    }

    fn put(&mut self, _record: Record) -> Result<()> {
        Ok(())
    }

    fn remove(&mut self, _k: &Key) {}

    fn records(&self) -> Self::RecordsIter<'_> {
        vec![].into_iter()
    }

    fn add_provider(&mut self, _record: ProviderRecord) -> Result<()> {
        Ok(())
    }

    fn providers(&self, _key: &Key) -> Vec<ProviderRecord> {
        vec![]
    }

    fn provided(&self) -> Self::ProvidedIter<'_> {
        vec![].into_iter()
    }

    fn remove_provider(&mut self, _key: &Key, _provider: &PeerId) {}
}

#[expect(trivial_casts)]
#[cfg(test)]
mod tests {
    use super::*;
    use bls::SecretKey;
    use xor_name::XorName;

    use ant_protocol::storage::{
        try_deserialize_record, try_serialize_record, Chunk, ChunkAddress, DataTypes, Scratchpad,
    };
    use assert_fs::{
        fixture::{PathChild, PathCreateDir},
        TempDir,
    };
    use bytes::Bytes;
    use eyre::ContextCompat;
    use libp2p::{core::multihash::Multihash, kad::RecordKey};
    use quickcheck::*;
    use tokio::runtime::Runtime;
    use tokio::time::{sleep, Duration};

    const MULITHASH_CODE: u64 = 0x12;

    #[derive(Clone, Debug)]
    struct ArbitraryKey(Key);
    #[derive(Clone, Debug)]
    struct ArbitraryRecord(Record);

    impl Arbitrary for ArbitraryKey {
        fn arbitrary(g: &mut Gen) -> ArbitraryKey {
            let hash: [u8; 32] = core::array::from_fn(|_| u8::arbitrary(g));
            ArbitraryKey(Key::from(
                Multihash::<64>::wrap(MULITHASH_CODE, &hash).expect("Failed to gen MultiHash"),
            ))
        }
    }

    impl Arbitrary for ArbitraryRecord {
        fn arbitrary(g: &mut Gen) -> ArbitraryRecord {
            let value = match try_serialize_record(
                &(0..50).map(|_| rand::random::<u8>()).collect::<Bytes>(),
                RecordKind::DataOnly(DataTypes::Chunk),
            ) {
                Ok(value) => value.to_vec(),
                Err(err) => panic!("Cannot generate record value {err:?}"),
            };
            let record = Record {
                key: ArbitraryKey::arbitrary(g).0,
                value,
                publisher: None,
                expires: None,
            };
            ArbitraryRecord(record)
        }
    }

    #[test]
    fn put_get_remove_record() {
        fn prop(r: ArbitraryRecord) {
            let rt = if let Ok(rt) = Runtime::new() {
                rt
            } else {
                panic!("Cannot create runtime");
            };
            rt.block_on(testing_thread(r));
        }
        quickcheck(prop as fn(_))
    }

    async fn testing_thread(r: ArbitraryRecord) {
        let r = r.0;
        let (network_event_sender, mut network_event_receiver) = mpsc::channel(1);
        let (swarm_cmd_sender, _) = mpsc::channel(1);

        let mut store = NodeRecordStore::with_config(
            PeerId::random(),
            Default::default(),
            network_event_sender,
            swarm_cmd_sender,
            #[cfg(feature = "open-metrics")]
            None,
        );

        // An initial unverified put should not write to disk
        assert!(store.put(r.clone()).is_ok());
        assert!(store.get(&r.key).is_none());

        let returned_record = if let Some(event) = network_event_receiver.recv().await {
            if let NetworkEvent::UnverifiedRecord(record) = event {
                record
            } else {
                panic!("Unexpected network event {event:?}");
            }
        } else {
            panic!("Failed recevied the record for further verification");
        };

        let returned_record_key = returned_record.key.clone();

        assert!(store
            .put_verified(returned_record, ValidationType::Chunk, true)
            .is_ok());

        // We must also mark the record as stored (which would be triggered after the async write in nodes
        // via NetworkEvent::CompletedWrite)
        store.mark_as_stored(returned_record_key, ValidationType::Chunk, DataTypes::Chunk);

        // loop over store.get max_iterations times to ensure async disk write had time to complete.
        let max_iterations = 10;
        let mut iteration = 0;
        while iteration < max_iterations {
            // try to check if it is equal to the actual record. This is needed because, the file
            // might not be fully written to the fs and would cause intermittent failures.
            // If there is actually a problem with the PUT, the assert statement below would catch it.
            if store
                .get(&r.key)
                .is_some_and(|record| Cow::Borrowed(&r) == record)
            {
                break;
            }
            sleep(Duration::from_millis(100)).await;
            iteration += 1;
        }
        if iteration == max_iterations {
            panic!("record_store test failed with stored record cann't be read back");
        }

        assert_eq!(
            Some(Cow::Borrowed(&r)),
            store.get(&r.key),
            "record can be retrieved after put"
        );
        store.remove(&r.key);

        assert!(store.get(&r.key).is_none());
    }

    #[tokio::test]
    async fn can_store_after_restart() -> eyre::Result<()> {
        let tmp_dir = TempDir::new()?;
        let current_test_dir = tmp_dir.child("can_store_after_restart");
        current_test_dir.create_dir_all()?;

        let store_config = NodeRecordStoreConfig {
            storage_dir: current_test_dir.to_path_buf(),
            encryption_seed: [1u8; 16],
            ..Default::default()
        };
        let self_id = PeerId::random();

        // Create channels with proper receivers
        let (network_event_sender, _network_event_receiver) = mpsc::channel(1);
        let (swarm_cmd_sender, mut swarm_cmd_receiver) = mpsc::channel(1);

        let mut store = NodeRecordStore::with_config(
            self_id,
            store_config.clone(),
            network_event_sender.clone(),
            swarm_cmd_sender.clone(),
            #[cfg(feature = "open-metrics")]
            None,
        );

        // Create a chunk
        let chunk_data = Bytes::from_static(b"Test chunk data");
        let chunk = Chunk::new(chunk_data);
        let chunk_address = *chunk.address();

        // Create a record from the chunk
        let record = Record {
            key: NetworkAddress::ChunkAddress(chunk_address).to_record_key(),
            value: try_serialize_record(&chunk, RecordKind::DataOnly(DataTypes::Chunk))?.to_vec(),
            expires: None,
            publisher: None,
        };

        // Store the chunk using put_verified
        assert!(store
            .put_verified(record.clone(), ValidationType::Chunk, true)
            .is_ok());

        // Wait for the async write operation to complete
        if let Some(cmd) = swarm_cmd_receiver.recv().await {
            match cmd {
                LocalSwarmCmd::AddLocalRecordAsStored {
                    key,
                    record_type,
                    data_type,
                } => {
                    store.mark_as_stored(key, record_type, data_type);
                }
                _ => panic!("Unexpected command received"),
            }
        }

        // Verify the chunk is stored
        let stored_record = store.get(&record.key);
        assert!(stored_record.is_some(), "Chunk should be stored initially");

        // Sleep a while to let OS completes the flush to disk
        sleep(Duration::from_secs(1)).await;

        // Create new channels for the restarted store
        let (new_network_event_sender, _new_network_event_receiver) = mpsc::channel(1);
        let (new_swarm_cmd_sender, _new_swarm_cmd_receiver) = mpsc::channel(1);

        // Restart the store with same encrypt_seed but new channels
        drop(store);
        let store = NodeRecordStore::with_config(
            self_id,
            store_config,
            new_network_event_sender,
            new_swarm_cmd_sender,
            #[cfg(feature = "open-metrics")]
            None,
        );

        // Verify the record still exists
        let stored_record = store.get(&record.key);
        assert!(
            stored_record.is_some(),
            "Chunk should be stored after restart with same key"
        );

        // Create new channels for the different seed test
        let (diff_network_event_sender, _diff_network_event_receiver) = mpsc::channel(1);
        let (diff_swarm_cmd_sender, _diff_swarm_cmd_receiver) = mpsc::channel(1);

        // Restart the store with different encrypt_seed
        let self_id_diff = PeerId::random();
        let store_config_diff = NodeRecordStoreConfig {
            storage_dir: current_test_dir.to_path_buf(),
            encryption_seed: [2u8; 16],
            ..Default::default()
        };
        let store_diff = NodeRecordStore::with_config(
            self_id_diff,
            store_config_diff,
            diff_network_event_sender,
            diff_swarm_cmd_sender,
            #[cfg(feature = "open-metrics")]
            None,
        );

        // When encryption is enabled, the record should be gone because it can't be decrypted
        // with the different encryption seed
        assert!(
            store_diff.get(&record.key).is_none(),
            "Chunk should be gone with different encryption key"
        );

        Ok(())
    }

    #[tokio::test]
    async fn can_store_and_retrieve_chunk() {
        let temp_dir = std::env::temp_dir();
        let store_config = NodeRecordStoreConfig {
            storage_dir: temp_dir,
            ..Default::default()
        };
        let self_id = PeerId::random();
        let (network_event_sender, _) = mpsc::channel(1);
        let (swarm_cmd_sender, _) = mpsc::channel(1);

        let mut store = NodeRecordStore::with_config(
            self_id,
            store_config,
            network_event_sender,
            swarm_cmd_sender,
            #[cfg(feature = "open-metrics")]
            None,
        );

        // Create a chunk
        let chunk_data = Bytes::from_static(b"Test chunk data");
        let chunk = Chunk::new(chunk_data.clone());
        let chunk_address = *chunk.address();

        // Create a record from the chunk
        let record = Record {
            key: NetworkAddress::ChunkAddress(chunk_address).to_record_key(),
            value: chunk_data.to_vec(),
            expires: None,
            publisher: None,
        };

        // Store the chunk using put_verified
        assert!(store
            .put_verified(record.clone(), ValidationType::Chunk, true)
            .is_ok());

        // Mark as stored (simulating the CompletedWrite event)
        store.mark_as_stored(record.key.clone(), ValidationType::Chunk, DataTypes::Chunk);

        // Verify the chunk is stored
        let stored_record = store.get(&record.key);
        assert!(stored_record.is_some(), "Chunk should be stored");

        if let Some(stored) = stored_record {
            assert_eq!(
                stored.value, chunk_data,
                "Stored chunk data should match original"
            );

            let stored_address = ChunkAddress::new(XorName::from_content(&stored.value));
            assert_eq!(
                stored_address, chunk_address,
                "Stored chunk address should match original"
            );
        }

        // Clean up
        store.remove(&record.key);
        assert!(
            store.get(&record.key).is_none(),
            "Chunk should be removed after cleanup"
        );
    }

    #[tokio::test]
    async fn can_store_and_retrieve_scratchpad() -> eyre::Result<()> {
        let temp_dir = std::env::temp_dir();
        let store_config = NodeRecordStoreConfig {
            storage_dir: temp_dir,
            ..Default::default()
        };
        let self_id = PeerId::random();
        let (network_event_sender, _) = mpsc::channel(1);
        let (swarm_cmd_sender, _) = mpsc::channel(1);

        let mut store = NodeRecordStore::with_config(
            self_id,
            store_config,
            network_event_sender,
            swarm_cmd_sender,
            #[cfg(feature = "open-metrics")]
            None,
        );

        // Create a scratchpad
        let unencrypted_scratchpad_data = Bytes::from_static(b"Test scratchpad data");
        let owner_sk = SecretKey::random();

        let scratchpad = Scratchpad::new(&owner_sk, 0, &unencrypted_scratchpad_data, 0);

        let scratchpad_address = *scratchpad.address();

        // Create a record from the scratchpad
        let record = Record {
            key: NetworkAddress::ScratchpadAddress(scratchpad_address).to_record_key(),
            value: try_serialize_record(&scratchpad, RecordKind::DataOnly(DataTypes::Scratchpad))?
                .to_vec(),
            expires: None,
            publisher: None,
        };

        // Store the scratchpad using put_verified
        assert!(store
            .put_verified(
                record.clone(),
                ValidationType::NonChunk(XorName::from_content(&record.value)),
                true,
            )
            .is_ok());

        // Mark as stored (simulating the CompletedWrite event)
        store.mark_as_stored(
            record.key.clone(),
            ValidationType::NonChunk(XorName::from_content(&record.value)),
            DataTypes::Scratchpad,
        );

        // Verify the scratchpad is stored
        let stored_record = store.get(&record.key);
        assert!(stored_record.is_some(), "Scratchpad should be stored");

        if let Some(stored) = stored_record {
            let scratchpad = try_deserialize_record::<Scratchpad>(&stored)?;

            let stored_address = scratchpad.address();
            assert_eq!(
                stored_address, &scratchpad_address,
                "Stored scratchpad address should match original"
            );

            let decrypted_data = scratchpad.decrypt_data(&owner_sk)?;

            assert_eq!(
                decrypted_data, unencrypted_scratchpad_data,
                "Stored scratchpad data should match original"
            );
        }

        store.remove(&record.key);
        assert!(
            store.get(&record.key).is_none(),
            "Scratchpad should be removed after cleanup"
        );

        Ok(())
    }
    #[tokio::test]
    async fn pruning_on_full() -> Result<()> {
        let max_iterations = 10;
        // lower max records for faster testing
        let max_records = 50;

        let temp_dir = std::env::temp_dir();
        let unique_dir_name = uuid::Uuid::new_v4().to_string();
        let storage_dir = temp_dir.join(unique_dir_name);
        fs::create_dir_all(&storage_dir).expect("Failed to create directory");

        // Set the config::max_record to be 50, then generate 100 records
        // On storing the 51st to 100th record,
        // check there is an expected pruning behaviour got carried out.
        let store_config = NodeRecordStoreConfig {
            max_records,
            storage_dir,
            ..Default::default()
        };
        let self_id = PeerId::random();
        let (network_event_sender, _) = mpsc::channel(1);
        let (swarm_cmd_sender, _) = mpsc::channel(1);

        let mut store = NodeRecordStore::with_config(
            self_id,
            store_config.clone(),
            network_event_sender,
            swarm_cmd_sender,
            #[cfg(feature = "open-metrics")]
            None,
        );
        // keep track of everything ever stored, to check missing at the end are further away
        let mut stored_records_at_some_point: Vec<RecordKey> = vec![];
        let self_address = NetworkAddress::from(self_id);

        // keep track of fails to assert they're further than stored
        let mut failed_records = vec![];

        // try and put an excess of records
        for _ in 0..max_records * 2 {
            // println!("i: {i}");
            let record_key = NetworkAddress::from(PeerId::random()).to_record_key();
            let value = match try_serialize_record(
                &(0..50).map(|_| rand::random::<u8>()).collect::<Bytes>(),
                RecordKind::DataOnly(DataTypes::Chunk),
            ) {
                Ok(value) => value.to_vec(),
                Err(err) => panic!("Cannot generate record value {err:?}"),
            };
            let record = Record {
                key: record_key.clone(),
                value,
                publisher: None,
                expires: None,
            };

            // Will be stored anyway.
            let succeeded = store
                .put_verified(record, ValidationType::Chunk, true)
                .is_ok();

            if !succeeded {
                failed_records.push(record_key.clone());
                println!("failed {:?}", PrettyPrintRecordKey::from(&record_key));
            } else {
                // We must also mark the record as stored (which would be triggered
                // after the async write in nodes via NetworkEvent::CompletedWrite)
                store.mark_as_stored(record_key.clone(), ValidationType::Chunk, DataTypes::Chunk);

                println!("success sotred len: {:?} ", store.record_addresses().len());
                stored_records_at_some_point.push(record_key.clone());
                if stored_records_at_some_point.len() <= max_records {
                    assert!(succeeded);
                }
                // loop over max_iterations times to ensure async disk write had time to complete.
                let mut iteration = 0;
                while iteration < max_iterations {
                    if store.get(&record_key).is_some() {
                        break;
                    }
                    sleep(Duration::from_millis(100)).await;
                    iteration += 1;
                }
                if iteration == max_iterations {
                    panic!("record_store prune test failed with stored record {record_key:?} can't be read back");
                }
            }
        }

        let stored_data_at_end = store.record_addresses();
        assert!(
            stored_data_at_end.len() == max_records,
            "Stored records ({:?}) should be max_records, {max_records:?}",
            stored_data_at_end.len(),
        );

        // now assert that we've stored at _least_ max records (likely many more over the liftime of the store)
        assert!(
            stored_records_at_some_point.len() >= max_records,
            "we should have stored ata least max over time"
        );

        // now all failed records should be farther than the farthest stored record
        let mut sorted_stored_data = stored_data_at_end.iter().collect_vec();

        sorted_stored_data
            .sort_by(|(a, _), (b, _)| self_address.distance(a).cmp(&self_address.distance(b)));

        // next assert that all records stored are closer than the next closest of the failed records
        if let Some((most_distant_data, _)) = sorted_stored_data.last() {
            for failed_record in failed_records {
                let failed_data = NetworkAddress::from(&failed_record);
                assert!(
                    self_address.distance(&failed_data) > self_address.distance(most_distant_data),
                    "failed record {failed_data:?} should be farther than the farthest stored record {most_distant_data:?}"
                );
            }

            // now for any stored data. It either shoudl still be stored OR further away than `most_distant_data`
            for data in stored_records_at_some_point {
                let data_addr = NetworkAddress::from(&data);
                if !sorted_stored_data.contains(&(&data_addr, &ValidationType::Chunk)) {
                    assert!(
                        self_address.distance(&data_addr)
                            > self_address.distance(most_distant_data),
                        "stored record should be farther than the farthest stored record"
                    );
                }
            }
        }

        Ok(())
    }

    #[tokio::test]
    async fn get_records_within_range() -> eyre::Result<()> {
        let max_records = 50;

        let temp_dir = std::env::temp_dir();
        let unique_dir_name = uuid::Uuid::new_v4().to_string();
        let storage_dir = temp_dir.join(unique_dir_name);

        // setup the store
        let store_config = NodeRecordStoreConfig {
            max_records,
            storage_dir,
            ..Default::default()
        };
        let self_id = PeerId::random();
        let (network_event_sender, _) = mpsc::channel(1);
        let (swarm_cmd_sender, _) = mpsc::channel(1);
        let mut store = NodeRecordStore::with_config(
            self_id,
            store_config,
            network_event_sender,
            swarm_cmd_sender,
            #[cfg(feature = "open-metrics")]
            None,
        );

        let mut stored_records: Vec<RecordKey> = vec![];
        let self_address = NetworkAddress::from(self_id);

        // add records...
        // minus one here as if we hit max, the store will fail
        for _ in 0..max_records - 1 {
            let record_key = NetworkAddress::from(PeerId::random()).to_record_key();
            let value = match try_serialize_record(
                &(0..max_records)
                    .map(|_| rand::random::<u8>())
                    .collect::<Bytes>(),
                RecordKind::DataOnly(DataTypes::Chunk),
            ) {
                Ok(value) => value.to_vec(),
                Err(err) => panic!("Cannot generate record value {err:?}"),
            };
            let record = Record {
                key: record_key.clone(),
                value,
                publisher: None,
                expires: None,
            };
            assert!(store
                .put_verified(record, ValidationType::Chunk, true)
                .is_ok());
            // We must also mark the record as stored (which would be triggered after the async write in nodes
            // via NetworkEvent::CompletedWrite)
            store.mark_as_stored(record_key.clone(), ValidationType::Chunk, DataTypes::Chunk);

            stored_records.push(record_key.clone());
            stored_records.sort_by(|a, b| {
                let a = NetworkAddress::from(a);
                let b = NetworkAddress::from(b);
                self_address.distance(&a).cmp(&self_address.distance(&b))
            });
        }

        // get a record halfway through the list
        let halfway_record_address = NetworkAddress::from(
            stored_records
                .get(max_records / 2)
                .wrap_err("Could not parse record store key")?,
        );
        // get the distance to this record from our local key
        let distance = &self_address.distance(&halfway_record_address);

        // must be plus one bucket from the halfway record
        store.set_responsible_distance_range(*distance);

        let records_in_range = store.get_records_within_distance_range(*distance);

        // check that the number of records returned is larger than half our records
        // (ie, that we cover _at least_ all the records within our distance range)
        assert!(
            records_in_range >= max_records / 2,
            "Not enough records in range {records_in_range}/{}",
            max_records / 2
        );

        Ok(())
    }

    #[tokio::test]
    async fn historic_quoting_metrics() -> Result<()> {
        let temp_dir = std::env::temp_dir();
        let unique_dir_name = uuid::Uuid::new_v4().to_string();
        let storage_dir = temp_dir.join(unique_dir_name);
        fs::create_dir_all(&storage_dir).expect("Failed to create directory");
        let historic_quote_dir = storage_dir.clone();

        let store_config = NodeRecordStoreConfig {
            storage_dir,
            historic_quote_dir,
            ..Default::default()
        };
        let self_id = PeerId::random();
        let (network_event_sender, _) = mpsc::channel(1);
        let (swarm_cmd_sender, _) = mpsc::channel(1);

        let mut store = NodeRecordStore::with_config(
            self_id,
            store_config.clone(),
            network_event_sender.clone(),
            swarm_cmd_sender.clone(),
            #[cfg(feature = "open-metrics")]
            None,
        );

        store.payment_received();

        // Wait for a while to allow the file written to disk.
        sleep(Duration::from_millis(5000)).await;

        let new_store = NodeRecordStore::with_config(
            self_id,
            store_config,
            network_event_sender,
            swarm_cmd_sender,
            #[cfg(feature = "open-metrics")]
            None,
        );

        assert_eq!(1, new_store.received_payment_count);
        assert_eq!(store.timestamp, new_store.timestamp);

        Ok(())
    }

    #[tokio::test]
    async fn test_cache_pruning_and_size_limit() {
        // Create cache with small size and short timeout for testing
        let cache_size = 3;
        let cache_timeout = Duration::from_millis(100);
        let mut cache = RecordCache::new(cache_size, cache_timeout);

        // Create test records
        let record1 = Record {
            key: RecordKey::new(b"key1"),
            value: b"value1".to_vec(),
            publisher: None,
            expires: None,
        };
        let record2 = Record {
            key: RecordKey::new(b"key2"),
            value: b"value2".to_vec(),
            publisher: None,
            expires: None,
        };
        let record3 = Record {
            key: RecordKey::new(b"key3"),
            value: b"value3".to_vec(),
            publisher: None,
            expires: None,
        };
        let record4 = Record {
            key: RecordKey::new(b"key4"),
            value: b"value4".to_vec(),
            publisher: None,
            expires: None,
        };

        // Add records up to cache size
        cache.push_back(record1.key.clone(), record1.clone());
        sleep(Duration::from_millis(1)).await;
        cache.push_back(record2.key.clone(), record2.clone());
        sleep(Duration::from_millis(1)).await;
        cache.push_back(record3.key.clone(), record3.clone());
        sleep(Duration::from_millis(1)).await;

        // Verify all records are present
        assert!(cache.get(&record1.key).is_some());
        assert!(cache.get(&record2.key).is_some());
        assert!(cache.get(&record3.key).is_some());

        // Add one more record to trigger size-based pruning
        cache.push_back(record4.key.clone(), record4.clone());

        // Verify cache size is maintained
        assert_eq!(cache.records_cache.len(), cache_size);

        // Verify oldest record was removed
        assert!(cache.get(&record1.key).is_none());

        // Wait for timeout to expire
        sleep(cache_timeout + Duration::from_millis(10)).await;

        // Add another record to trigger time-based pruning
        let record5 = Record {
            key: RecordKey::new(b"key5"),
            value: b"value5".to_vec(),
            publisher: None,
            expires: None,
        };
        cache.push_back(record5.key.clone(), record5.clone());

        // Verify all timed-out records were removed
        assert!(cache.get(&record2.key).is_none());
        assert!(cache.get(&record3.key).is_none());
        assert!(cache.get(&record4.key).is_none());

        // Verify new record is present
        assert!(cache.get(&record5.key).is_some());
    }
}
