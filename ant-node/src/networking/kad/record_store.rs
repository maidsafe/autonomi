// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Record storage abstraction for Kademlia DHT.
//! 
//! This module provides a transport-agnostic interface for storing and retrieving
//! DHT records locally.

#![allow(dead_code)]

use std::{
    collections::HashMap,
    time::{Duration, Instant, SystemTime},
    path::PathBuf,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::networking::kad::transport::{RecordKey, Record, KadError};

/// Configuration for record store behavior
#[derive(Clone, Debug)]
pub struct RecordStoreConfig {
    /// Maximum number of records to store
    pub max_records: usize,
    /// Maximum size of individual records (in bytes)
    pub max_record_size: usize,
    /// Maximum total size of all records (in bytes)
    pub max_total_size: usize,
    /// Default TTL for records without expiration
    pub default_ttl: Option<Duration>,
    /// Cleanup interval for expired records
    pub cleanup_interval: Duration,
    /// Whether to persist records to disk
    pub enable_persistence: bool,
    /// Directory for persistent storage
    pub storage_dir: Option<PathBuf>,
}

impl Default for RecordStoreConfig {
    fn default() -> Self {
        Self {
            max_records: 1000,
            max_record_size: 1024 * 1024, // 1MB
            max_total_size: 100 * 1024 * 1024, // 100MB
            default_ttl: Some(Duration::from_secs(3600)), // 1 hour
            cleanup_interval: Duration::from_secs(300), // 5 minutes
            enable_persistence: false,
            storage_dir: None,
        }
    }
}

/// Record storage statistics
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RecordStoreStats {
    /// Total number of records stored
    pub record_count: usize,
    /// Total size of all records in bytes
    pub total_size: usize,
    /// Number of records added since startup
    pub records_added: u64,
    /// Number of records retrieved since startup
    pub records_retrieved: u64,
    /// Number of records removed since startup
    pub records_removed: u64,
    /// Number of cleanup operations performed
    pub cleanup_operations: u64,
}

/// Errors that can occur in record storage operations
#[derive(Error, Debug, Clone)]
pub enum RecordStoreError {
    #[error("Record not found: {key}")]
    RecordNotFound { key: String },
    
    #[error("Record too large: {size} bytes (max: {max_size})")]
    RecordTooLarge { size: usize, max_size: usize },
    
    #[error("Storage full: {current_size} bytes (max: {max_size})")]
    StorageFull { current_size: usize, max_size: usize },
    
    #[error("Too many records: {count} (max: {max_count})")]
    TooManyRecords { count: usize, max_count: usize },
    
    #[error("IO error: {0}")]
    Io(String),
    
    #[error("Serialization error: {0}")]
    Serialization(String),
    
    #[error("Invalid record: {reason}")]
    InvalidRecord { reason: String },
}

impl From<RecordStoreError> for KadError {
    fn from(error: RecordStoreError) -> Self {
        KadError::Storage(error.to_string())
    }
}

/// Result type for record store operations
pub type RecordStoreResult<T> = Result<T, RecordStoreError>;

/// Abstract interface for storing DHT records
#[async_trait]
pub trait RecordStore: Send + Sync + 'static {
    /// Get a record by its key
    async fn get(&self, key: &RecordKey) -> RecordStoreResult<Option<Record>>;
    
    /// Store a record
    async fn put(&mut self, record: Record) -> RecordStoreResult<()>;
    
    /// Remove a record by its key
    async fn remove(&mut self, key: &RecordKey) -> RecordStoreResult<Option<Record>>;
    
    /// Check if a record exists
    async fn contains(&self, key: &RecordKey) -> bool;
    
    /// Get all stored record keys
    async fn keys(&self) -> Vec<RecordKey>;
    
    /// Get storage statistics
    async fn stats(&self) -> RecordStoreStats;
    
    /// Perform cleanup of expired records
    async fn cleanup(&mut self) -> RecordStoreResult<usize>;
    
    /// Get the configuration for this store
    fn config(&self) -> &RecordStoreConfig;
    
    /// Check if the store can accept a new record of the given size
    async fn can_store(&self, record_size: usize) -> bool;
}

/// In-memory record store implementation
#[derive(Debug)]
pub struct MemoryRecordStore {
    /// Configuration
    config: RecordStoreConfig,
    /// Stored records
    records: HashMap<RecordKey, StoredRecord>,
    /// Statistics
    stats: RecordStoreStats,
    /// Last cleanup time
    last_cleanup: Instant,
}

/// Internal representation of a stored record with metadata
#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredRecord {
    /// The actual record
    record: Record,
    /// When this record was stored (serializable)
    stored_at: SystemTime,
    /// When this record was last accessed (serializable)
    last_accessed: SystemTime,
    /// Size of the record in bytes
    size: usize,
}

impl StoredRecord {
    fn new(record: Record) -> Self {
        let size = bincode::serialized_size(&record).unwrap_or(0) as usize;
        let now = SystemTime::now();
        
        Self {
            record,
            stored_at: now,
            last_accessed: now,
            size,
        }
    }
    
    fn access(&mut self) -> &Record {
        self.last_accessed = SystemTime::now();
        &self.record
    }
    
    fn is_expired(&self, default_ttl: Option<Duration>) -> bool {
        // Check explicit expiration first
        if let Some(expires) = self.record.expires {
            return SystemTime::now() > expires;
        }
        
        // Check default TTL
        if let Some(ttl) = default_ttl {
            return self.stored_at.elapsed().unwrap_or_default() > ttl;
        }
        
        false
    }
}

impl MemoryRecordStore {
    /// Create a new in-memory record store
    pub fn new(config: RecordStoreConfig) -> Self {
        Self {
            config,
            records: HashMap::new(),
            stats: RecordStoreStats::default(),
            last_cleanup: Instant::now(),
        }
    }
    
    /// Calculate total size of all stored records
    fn total_size(&self) -> usize {
        self.records.values().map(|r| r.size).sum()
    }
    
    /// Update statistics
    fn update_stats(&mut self) {
        self.stats.record_count = self.records.len();
        self.stats.total_size = self.total_size();
    }
    
    /// Remove expired records
    fn remove_expired(&mut self) -> usize {
        let mut removed_count = 0;
        let mut to_remove = Vec::new();
        
        for (key, stored_record) in &self.records {
            if stored_record.is_expired(self.config.default_ttl) {
                to_remove.push(key.clone());
            }
        }
        
        for key in to_remove {
            if self.records.remove(&key).is_some() {
                removed_count += 1;
                self.stats.records_removed += 1;
            }
        }
        
        removed_count
    }
    
    /// Remove least recently used records to make space
    fn evict_lru(&mut self, space_needed: usize) -> RecordStoreResult<()> {
        let mut candidates: Vec<_> = self.records.iter().collect();
        
        // Sort by last accessed time (oldest first)
        candidates.sort_by_key(|(_, stored)| stored.last_accessed);
        
        let mut space_freed = 0;
        let mut to_remove = Vec::new();
        
        for (key, stored_record) in candidates {
            to_remove.push(key.clone());
            space_freed += stored_record.size;
            
            if space_freed >= space_needed {
                break;
            }
        }
        
        if space_freed < space_needed {
            return Err(RecordStoreError::StorageFull {
                current_size: self.total_size(),
                max_size: self.config.max_total_size,
            });
        }
        
        for key in to_remove {
            if self.records.remove(&key).is_some() {
                self.stats.records_removed += 1;
            }
        }
        
        Ok(())
    }
}

#[async_trait]
impl RecordStore for MemoryRecordStore {
    async fn get(&self, key: &RecordKey) -> RecordStoreResult<Option<Record>> {
        if let Some(stored_record) = self.records.get(key) {
            // Check if expired
            if stored_record.is_expired(self.config.default_ttl) {
                return Ok(None);
            }
            
            Ok(Some(stored_record.record.clone()))
        } else {
            Ok(None)
        }
    }
    
    async fn put(&mut self, record: Record) -> RecordStoreResult<()> {
        let record_size = bincode::serialized_size(&record)
            .map_err(|e| RecordStoreError::Serialization(e.to_string()))? as usize;
        
        // Check record size limit
        if record_size > self.config.max_record_size {
            return Err(RecordStoreError::RecordTooLarge {
                size: record_size,
                max_size: self.config.max_record_size,
            });
        }
        
        // Check if we need to make space
        let current_size = self.total_size();
        let size_after_insert = current_size + record_size;
        
        if size_after_insert > self.config.max_total_size {
            // Try to make space by evicting LRU records
            let space_needed = size_after_insert - self.config.max_total_size;
            self.evict_lru(space_needed)?;
        }
        
        // Check record count limit
        if self.records.len() >= self.config.max_records && !self.records.contains_key(&record.key) {
            return Err(RecordStoreError::TooManyRecords {
                count: self.records.len(),
                max_count: self.config.max_records,
            });
        }
        
        // Store the record
        let key = record.key.clone();
        let stored_record = StoredRecord::new(record);
        
        let is_new = !self.records.contains_key(&key);
        self.records.insert(key, stored_record);
        
        if is_new {
            self.stats.records_added += 1;
        }
        
        self.update_stats();
        Ok(())
    }
    
    async fn remove(&mut self, key: &RecordKey) -> RecordStoreResult<Option<Record>> {
        if let Some(stored_record) = self.records.remove(key) {
            self.stats.records_removed += 1;
            self.update_stats();
            Ok(Some(stored_record.record))
        } else {
            Ok(None)
        }
    }
    
    async fn contains(&self, key: &RecordKey) -> bool {
        if let Some(stored_record) = self.records.get(key) {
            !stored_record.is_expired(self.config.default_ttl)
        } else {
            false
        }
    }
    
    async fn keys(&self) -> Vec<RecordKey> {
        self.records
            .iter()
            .filter(|(_, stored)| !stored.is_expired(self.config.default_ttl))
            .map(|(key, _)| key.clone())
            .collect()
    }
    
    async fn stats(&self) -> RecordStoreStats {
        let mut stats = self.stats.clone();
        stats.record_count = self.records.len();
        stats.total_size = self.total_size();
        stats
    }
    
    async fn cleanup(&mut self) -> RecordStoreResult<usize> {
        self.last_cleanup = Instant::now();
        let removed_count = self.remove_expired();
        self.stats.cleanup_operations += 1;
        self.update_stats();
        Ok(removed_count)
    }
    
    fn config(&self) -> &RecordStoreConfig {
        &self.config
    }
    
    async fn can_store(&self, record_size: usize) -> bool {
        if record_size > self.config.max_record_size {
            return false;
        }
        
        let current_size = self.total_size();
        let size_after_insert = current_size + record_size;
        
        size_after_insert <= self.config.max_total_size &&
        self.records.len() < self.config.max_records
    }
}

/// Persistent record store that saves records to disk
#[derive(Debug)]
pub struct PersistentRecordStore {
    /// In-memory cache
    memory_store: MemoryRecordStore,
    /// Storage directory
    storage_dir: PathBuf,
    /// Whether persistence is enabled
    persistence_enabled: bool,
}

impl PersistentRecordStore {
    /// Create a new persistent record store
    pub async fn new(mut config: RecordStoreConfig) -> RecordStoreResult<Self> {
        let storage_dir = config.storage_dir.take()
            .unwrap_or_else(|| std::env::temp_dir().join("kad_records"));
        
        // Create storage directory if it doesn't exist
        if config.enable_persistence {
            std::fs::create_dir_all(&storage_dir)
                .map_err(|e| RecordStoreError::Io(e.to_string()))?;
        }
        
        let memory_store = MemoryRecordStore::new(config.clone());
        
        let mut store = Self {
            memory_store,
            storage_dir,
            persistence_enabled: config.enable_persistence,
        };
        
        // Load existing records if persistence is enabled
        if store.persistence_enabled {
            store.load_from_disk().await?;
        }
        
        Ok(store)
    }
    
    /// Load records from disk
    async fn load_from_disk(&mut self) -> RecordStoreResult<()> {
        if !self.storage_dir.exists() {
            return Ok(());
        }
        
        let read_dir = std::fs::read_dir(&self.storage_dir)
            .map_err(|e| RecordStoreError::Io(e.to_string()))?;
        
        for entry in read_dir {
            let entry = entry.map_err(|e| RecordStoreError::Io(e.to_string()))?;
            let path = entry.path();
            
            if path.extension().and_then(|s| s.to_str()) == Some("record") {
                match self.load_record_file(&path).await {
                    Ok(record) => {
                        let _ = self.memory_store.put(record).await;
                    }
                    Err(e) => {
                        eprintln!("Failed to load record from {:?}: {}", path, e);
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Load a single record file
    async fn load_record_file(&self, path: &std::path::Path) -> RecordStoreResult<Record> {
        let data = std::fs::read(path)
            .map_err(|e| RecordStoreError::Io(e.to_string()))?;
        
        bincode::deserialize(&data)
            .map_err(|e| RecordStoreError::Serialization(e.to_string()))
    }
    
    /// Save a record to disk
    async fn save_record_to_disk(&self, record: &Record) -> RecordStoreResult<()> {
        if !self.persistence_enabled {
            return Ok(());
        }
        
        let filename = format!("{}.record", hex::encode(&record.key.key));
        let path = self.storage_dir.join(filename);
        
        let data = bincode::serialize(record)
            .map_err(|e| RecordStoreError::Serialization(e.to_string()))?;
        
        std::fs::write(&path, data)
            .map_err(|e| RecordStoreError::Io(e.to_string()))?;
        
        Ok(())
    }
    
    /// Remove a record file from disk
    async fn remove_record_from_disk(&self, key: &RecordKey) -> RecordStoreResult<()> {
        if !self.persistence_enabled {
            return Ok(());
        }
        
        let filename = format!("{}.record", hex::encode(&key.key));
        let path = self.storage_dir.join(filename);
        
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| RecordStoreError::Io(e.to_string()))?;
        }
        
        Ok(())
    }
}

#[async_trait]
impl RecordStore for PersistentRecordStore {
    async fn get(&self, key: &RecordKey) -> RecordStoreResult<Option<Record>> {
        self.memory_store.get(key).await
    }
    
    async fn put(&mut self, record: Record) -> RecordStoreResult<()> {
        // Save to disk first if persistence is enabled
        self.save_record_to_disk(&record).await?;
        
        // Then store in memory
        self.memory_store.put(record).await
    }
    
    async fn remove(&mut self, key: &RecordKey) -> RecordStoreResult<Option<Record>> {
        // Remove from disk first
        self.remove_record_from_disk(key).await?;
        
        // Then remove from memory
        self.memory_store.remove(key).await
    }
    
    async fn contains(&self, key: &RecordKey) -> bool {
        self.memory_store.contains(key).await
    }
    
    async fn keys(&self) -> Vec<RecordKey> {
        self.memory_store.keys().await
    }
    
    async fn stats(&self) -> RecordStoreStats {
        self.memory_store.stats().await
    }
    
    async fn cleanup(&mut self) -> RecordStoreResult<usize> {
        self.memory_store.cleanup().await
    }
    
    fn config(&self) -> &RecordStoreConfig {
        self.memory_store.config()
    }
    
    async fn can_store(&self, record_size: usize) -> bool {
        self.memory_store.can_store(record_size).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_record(key: &[u8], value: &[u8]) -> Record {
        Record::new(RecordKey::new(key.to_vec()), value.to_vec())
    }

    #[tokio::test]
    async fn test_memory_store_basic_operations() {
        let config = RecordStoreConfig::default();
        let mut store = MemoryRecordStore::new(config);
        
        let record = create_test_record(b"test-key", b"test-value");
        let key = record.key.clone();
        
        // Test put and get
        assert!(store.put(record.clone()).await.is_ok());
        assert!(store.contains(&key).await);
        
        let retrieved = store.get(&key).await.unwrap();
        assert_eq!(retrieved, Some(record.clone()));
        
        // Test remove
        let removed = store.remove(&key).await.unwrap();
        assert_eq!(removed, Some(record));
        assert!(!store.contains(&key).await);
        assert_eq!(store.get(&key).await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_memory_store_size_limits() {
        let config = RecordStoreConfig {
            max_record_size: 100,
            max_total_size: 200,
            ..Default::default()
        };
        let mut store = MemoryRecordStore::new(config);
        
        // Test record size limit
        let large_record = create_test_record(b"large", &vec![0u8; 200]);
        let result = store.put(large_record).await;
        assert!(matches!(result, Err(RecordStoreError::RecordTooLarge { .. })));
        
        // Test total size limit with LRU eviction
        let record1 = create_test_record(b"key1", &vec![0u8; 50]);
        let record2 = create_test_record(b"key2", &vec![0u8; 50]);
        let record3 = create_test_record(b"key3", &vec![0u8; 50]);
        let record4 = create_test_record(b"key4", &vec![0u8; 50]);
        
        assert!(store.put(record1).await.is_ok());
        assert!(store.put(record2).await.is_ok());
        assert!(store.put(record3).await.is_ok());
        
        // Adding record4 should trigger LRU eviction
        assert!(store.put(record4).await.is_ok());
        
        // First record should have been evicted
        let key1 = RecordKey::new(b"key1".to_vec());
        assert!(!store.contains(&key1).await);
    }

    #[tokio::test]
    async fn test_memory_store_expiration() {
        let config = RecordStoreConfig {
            default_ttl: Some(Duration::from_millis(100)),
            ..Default::default()
        };
        let mut store = MemoryRecordStore::new(config);
        
        let record = create_test_record(b"expiring-key", b"expiring-value");
        let key = record.key.clone();
        
        assert!(store.put(record).await.is_ok());
        assert!(store.contains(&key).await);
        
        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(150)).await;
        
        // Record should be expired
        assert!(!store.contains(&key).await);
        assert_eq!(store.get(&key).await.unwrap(), None);
        
        // Cleanup should remove it
        let removed_count = store.cleanup().await.unwrap();
        assert_eq!(removed_count, 1);
    }

    #[tokio::test]
    async fn test_memory_store_stats() {
        let config = RecordStoreConfig::default();
        let mut store = MemoryRecordStore::new(config);
        
        let record1 = create_test_record(b"key1", b"value1");
        let record2 = create_test_record(b"key2", b"value2");
        
        assert!(store.put(record1).await.is_ok());
        assert!(store.put(record2).await.is_ok());
        
        let stats = store.stats().await;
        assert_eq!(stats.record_count, 2);
        assert_eq!(stats.records_added, 2);
        assert!(stats.total_size > 0);
        
        let key1 = RecordKey::new(b"key1".to_vec());
        assert!(store.remove(&key1).await.is_ok());
        
        let stats = store.stats().await;
        assert_eq!(stats.record_count, 1);
        assert_eq!(stats.records_removed, 1);
    }
}