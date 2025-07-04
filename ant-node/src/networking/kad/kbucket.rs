// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! K-bucket implementation for Kademlia routing table.
//! 
//! This module provides the core k-bucket data structure that manages peers
//! at different distances in the Kademlia keyspace.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::networking::kad::transport::{KadPeerId, KadDistance, KadAddress, ConnectionStatus};

/// A key in the Kademlia keyspace with distance calculation capabilities
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KBucketKey {
    bytes: Vec<u8>,
}

impl KBucketKey {
    /// Create a new k-bucket key from raw bytes
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    /// Create from a peer ID
    pub fn from_peer_id(peer_id: &KadPeerId) -> Self {
        Self {
            bytes: peer_id.as_bytes().to_vec(),
        }
    }

    /// Get the raw bytes
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Calculate XOR distance to another key
    pub fn distance(&self, other: &KBucketKey) -> KadDistance {
        let mut distance_bytes = [0u8; 32];
        let max_len = std::cmp::min(self.bytes.len(), other.bytes.len()).min(32);
        
        for i in 0..max_len {
            distance_bytes[i] = self.bytes.get(i).unwrap_or(&0) ^ other.bytes.get(i).unwrap_or(&0);
        }
        
        // If one key is longer, XOR with zeros (no effect)
        KadDistance { bytes: distance_bytes }
    }

    /// Get the bucket index for this key relative to a local key
    pub fn bucket_index(&self, local_key: &KBucketKey) -> Option<u32> {
        let distance = local_key.distance(self);
        let leading_zeros = distance.leading_zeros();
        
        // If distance is 0, this is our own key - no bucket
        if leading_zeros == 256 {
            None
        } else {
            Some(255 - leading_zeros)
        }
    }
}

/// Entry in a k-bucket containing peer information
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KBucketEntry {
    /// The peer's identifier
    pub peer_id: KadPeerId,
    /// Known addresses for this peer
    pub addresses: Vec<KadAddress>,
    /// Current connection status
    pub status: ConnectionStatus,
    /// When this peer was last seen active
    pub last_seen: Instant,
    /// When this entry was first added
    pub added_at: Instant,
    /// Number of successful interactions
    pub successful_interactions: u32,
    /// Number of failed interactions
    pub failed_interactions: u32,
    /// Whether this peer is currently being queried
    pub querying: bool,
}

impl KBucketEntry {
    /// Create a new k-bucket entry
    pub fn new(peer_id: KadPeerId, addresses: Vec<KadAddress>) -> Self {
        let now = Instant::now();
        Self {
            peer_id,
            addresses,
            status: ConnectionStatus::Unknown,
            last_seen: now,
            added_at: now,
            successful_interactions: 0,
            failed_interactions: 0,
            querying: false,
        }
    }

    /// Update the entry after a successful interaction
    pub fn mark_successful(&mut self) {
        self.last_seen = Instant::now();
        self.successful_interactions += 1;
        self.status = ConnectionStatus::Connected;
        self.querying = false;
    }

    /// Update the entry after a failed interaction
    pub fn mark_failed(&mut self) {
        self.failed_interactions += 1;
        self.status = ConnectionStatus::Disconnected;
        self.querying = false;
    }

    /// Mark this peer as currently being queried
    pub fn mark_querying(&mut self) {
        self.querying = true;
    }

    /// Check if this peer is considered stale (not seen recently)
    pub fn is_stale(&self, timeout: Duration) -> bool {
        self.last_seen.elapsed() > timeout
    }

    /// Calculate a reliability score for this peer (0.0 to 1.0)
    pub fn reliability_score(&self) -> f64 {
        let total = self.successful_interactions + self.failed_interactions;
        if total == 0 {
            0.5 // Neutral score for new peers
        } else {
            self.successful_interactions as f64 / total as f64
        }
    }

    /// Update addresses for this peer
    pub fn update_addresses(&mut self, new_addresses: Vec<KadAddress>) {
        self.addresses = new_addresses;
        self.last_seen = Instant::now();
    }
}

/// Configuration for k-bucket behavior
#[derive(Clone, Debug)]
pub struct KBucketConfig {
    /// Maximum number of entries per bucket (k value)
    pub k_value: usize,
    /// How long to wait before considering a peer stale
    pub peer_timeout: Duration,
    /// Whether to allow bucket replacement
    pub allow_replacement: bool,
    /// Maximum size of replacement cache
    pub replacement_cache_size: usize,
}

impl Default for KBucketConfig {
    fn default() -> Self {
        Self {
            k_value: 20,
            peer_timeout: Duration::from_secs(300), // 5 minutes
            allow_replacement: true,
            replacement_cache_size: 5,
        }
    }
}

/// Result of attempting to insert a peer into a k-bucket
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertResult {
    /// Peer was successfully inserted
    Inserted,
    /// Peer already existed and was updated
    Updated,
    /// Bucket is full, peer added to replacement cache
    Replacement { evicted: KadPeerId },
    /// Bucket is full and no replacements allowed
    Full,
    /// Peer was ignored (e.g., because it's our own ID)
    Ignored,
}

/// A single k-bucket that holds peers at a specific distance range
#[derive(Clone, Debug)]
pub struct KBucket {
    /// Configuration for this bucket
    config: KBucketConfig,
    /// Active peers in this bucket (limited to k_value)
    peers: VecDeque<KBucketEntry>,
    /// Replacement cache for when bucket is full
    replacement_cache: VecDeque<KBucketEntry>,
    /// Last time this bucket was updated
    last_updated: Instant,
}

impl KBucket {
    /// Create a new k-bucket with the given configuration
    pub fn new(config: KBucketConfig) -> Self {
        Self {
            config,
            peers: VecDeque::new(),
            replacement_cache: VecDeque::new(),
            last_updated: Instant::now(),
        }
    }

    /// Try to insert a peer into this bucket
    pub fn insert(&mut self, entry: KBucketEntry) -> InsertResult {
        self.last_updated = Instant::now();

        // Check if peer already exists
        if let Some(pos) = self.peers.iter().position(|e| e.peer_id == entry.peer_id) {
            // Update existing entry
            if let Some(existing) = self.peers.get_mut(pos) {
                existing.update_addresses(entry.addresses.clone());
                existing.mark_successful();
                
                // Move to end (most recently seen)
                let updated_entry = self.peers.remove(pos).unwrap();
                self.peers.push_back(updated_entry);
                return InsertResult::Updated;
            }
        }

        // Try to insert new peer
        if self.peers.len() < self.config.k_value {
            // Bucket has space
            self.peers.push_back(entry);
            InsertResult::Inserted
        } else if self.config.allow_replacement {
            // Bucket is full, try to find a stale peer to replace
            if let Some(stale_pos) = self.find_stale_peer() {
                let evicted = self.peers.remove(stale_pos).unwrap();
                self.peers.push_back(entry);
                InsertResult::Replacement { evicted: evicted.peer_id }
            } else {
                // No stale peers, add to replacement cache
                self.add_to_replacement_cache(entry);
                // Return the least recently seen peer as potentially evictable
                let lru = self.peers.front().unwrap();
                InsertResult::Replacement { evicted: lru.peer_id.clone() }
            }
        } else {
            InsertResult::Full
        }
    }

    /// Remove a peer from this bucket
    pub fn remove(&mut self, peer_id: &KadPeerId) -> Option<KBucketEntry> {
        self.last_updated = Instant::now();

        // Try to remove from main bucket
        if let Some(pos) = self.peers.iter().position(|e| e.peer_id == *peer_id) {
            let removed = self.peers.remove(pos);
            
            // Try to promote from replacement cache
            if let Some(replacement) = self.replacement_cache.pop_front() {
                self.peers.push_back(replacement);
            }
            
            return removed;
        }

        // Try to remove from replacement cache
        if let Some(pos) = self.replacement_cache.iter().position(|e| e.peer_id == *peer_id) {
            return self.replacement_cache.remove(pos);
        }

        None
    }

    /// Get a peer entry if it exists in this bucket
    pub fn get(&self, peer_id: &KadPeerId) -> Option<&KBucketEntry> {
        self.peers.iter()
            .chain(self.replacement_cache.iter())
            .find(|e| e.peer_id == *peer_id)
    }

    /// Get a mutable reference to a peer entry
    pub fn get_mut(&mut self, peer_id: &KadPeerId) -> Option<&mut KBucketEntry> {
        self.last_updated = Instant::now();
        
        self.peers.iter_mut()
            .chain(self.replacement_cache.iter_mut())
            .find(|e| e.peer_id == *peer_id)
    }

    /// Get all peers in this bucket (active + replacement cache)
    pub fn peers(&self) -> impl Iterator<Item = &KBucketEntry> {
        self.peers.iter().chain(self.replacement_cache.iter())
    }

    /// Get only active peers (not replacement cache)
    pub fn active_peers(&self) -> impl Iterator<Item = &KBucketEntry> {
        self.peers.iter()
    }

    /// Get the number of active peers
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    /// Check if the bucket is empty
    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// Check if the bucket is full
    pub fn is_full(&self) -> bool {
        self.peers.len() >= self.config.k_value
    }

    /// Get the closest peers to a target key
    pub fn closest_peers(&self, target: &KBucketKey, count: usize) -> Vec<KBucketEntry> {
        let mut peers: Vec<_> = self.active_peers().cloned().collect();
        
        // Sort by distance to target
        peers.sort_by(|a, b| {
            let dist_a = KBucketKey::from_peer_id(&a.peer_id).distance(target);
            let dist_b = KBucketKey::from_peer_id(&b.peer_id).distance(target);
            dist_a.cmp(&dist_b)
        });

        peers.into_iter().take(count).collect()
    }

    /// Find a stale peer that can be replaced
    fn find_stale_peer(&self) -> Option<usize> {
        self.peers.iter().position(|entry| {
            entry.is_stale(self.config.peer_timeout) && !entry.querying
        })
    }

    /// Add an entry to the replacement cache
    fn add_to_replacement_cache(&mut self, entry: KBucketEntry) {
        // Remove if already in cache
        if let Some(pos) = self.replacement_cache.iter().position(|e| e.peer_id == entry.peer_id) {
            self.replacement_cache.remove(pos);
        }

        // Add to front (most recent)
        self.replacement_cache.push_front(entry);

        // Trim cache if too large
        while self.replacement_cache.len() > self.config.replacement_cache_size {
            self.replacement_cache.pop_back();
        }
    }

    /// Refresh stale entries by marking them for querying
    pub fn refresh_stale_entries(&mut self) -> Vec<KadPeerId> {
        let mut to_refresh = Vec::new();
        
        for entry in &mut self.peers {
            if entry.is_stale(self.config.peer_timeout) && !entry.querying {
                entry.mark_querying();
                to_refresh.push(entry.peer_id.clone());
            }
        }
        
        to_refresh
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::networking::kad::transport::KadAddress;

    fn create_test_peer_id(id: u8) -> KadPeerId {
        KadPeerId::new(vec![id])
    }

    fn create_test_entry(id: u8) -> KBucketEntry {
        KBucketEntry::new(
            create_test_peer_id(id),
            vec![KadAddress::new("tcp".to_string(), format!("127.0.0.1:{}", 4000 + id))],
        )
    }

    #[test]
    fn test_kbucket_key_distance() {
        let key1 = KBucketKey::new(vec![0b10101010]);
        let key2 = KBucketKey::new(vec![0b11001100]);
        
        let distance = key1.distance(&key2);
        assert_eq!(distance.bytes[0], 0b01100110);
    }

    #[test]
    fn test_kbucket_key_bucket_index() {
        let local_key = KBucketKey::new(vec![0b00000000]);
        let remote_key = KBucketKey::new(vec![0b10000000]); // Distance has leading bit set
        
        let bucket_index = remote_key.bucket_index(&local_key);
        assert_eq!(bucket_index, Some(255)); // Highest bucket for max distance
    }

    #[test]
    fn test_kbucket_insert_and_get() {
        let mut bucket = KBucket::new(KBucketConfig::default());
        let entry = create_test_entry(1);
        let peer_id = entry.peer_id.clone();

        let result = bucket.insert(entry);
        assert_eq!(result, InsertResult::Inserted);
        assert_eq!(bucket.len(), 1);
        assert!(bucket.get(&peer_id).is_some());
    }

    #[test]
    fn test_kbucket_update_existing() {
        let mut bucket = KBucket::new(KBucketConfig::default());
        let entry1 = create_test_entry(1);
        let peer_id = entry1.peer_id.clone();

        bucket.insert(entry1);
        
        let mut entry2 = create_test_entry(1);
        entry2.addresses = vec![KadAddress::new("tcp".to_string(), "192.168.1.1:4001".to_string())];
        
        let result = bucket.insert(entry2);
        assert_eq!(result, InsertResult::Updated);
        assert_eq!(bucket.len(), 1);
        
        let updated = bucket.get(&peer_id).unwrap();
        assert_eq!(updated.addresses[0].address, "192.168.1.1:4001");
    }

    #[test]
    fn test_kbucket_full_replacement() {
        let config = KBucketConfig {
            k_value: 2,
            ..Default::default()
        };
        let mut bucket = KBucket::new(config);

        // Fill bucket
        bucket.insert(create_test_entry(1));
        bucket.insert(create_test_entry(2));
        assert!(bucket.is_full());

        // Try to insert third peer
        let result = bucket.insert(create_test_entry(3));
        match result {
            InsertResult::Replacement { evicted } => {
                // Should evict the least recently seen peer
                assert_eq!(evicted, create_test_peer_id(1));
            }
            _ => panic!("Expected replacement result"),
        }
    }

    #[test]
    fn test_kbucket_remove() {
        let mut bucket = KBucket::new(KBucketConfig::default());
        let entry = create_test_entry(1);
        let peer_id = entry.peer_id.clone();

        bucket.insert(entry);
        assert_eq!(bucket.len(), 1);

        let removed = bucket.remove(&peer_id);
        assert!(removed.is_some());
        assert_eq!(bucket.len(), 0);
        assert!(bucket.get(&peer_id).is_none());
    }

    #[test]
    fn test_kbucket_closest_peers() {
        let mut bucket = KBucket::new(KBucketConfig::default());
        
        // Insert peers with different IDs
        bucket.insert(create_test_entry(1));   // 00000001
        bucket.insert(create_test_entry(2));   // 00000010
        bucket.insert(create_test_entry(4));   // 00000100
        bucket.insert(create_test_entry(8));   // 00001000

        let target = KBucketKey::new(vec![3]); // 00000011
        let closest = bucket.closest_peers(&target, 2);

        assert_eq!(closest.len(), 2);
        // Should be sorted by distance - peers 1 and 2 are closest to 3
        assert_eq!(closest[0].peer_id, create_test_peer_id(2)); // distance 1
        assert_eq!(closest[1].peer_id, create_test_peer_id(1)); // distance 2
    }

    #[test]
    fn test_kbucket_entry_reliability_score() {
        let mut entry = create_test_entry(1);
        
        // New peer should have neutral score
        assert_eq!(entry.reliability_score(), 0.5);
        
        // After successful interactions
        entry.mark_successful();
        entry.mark_successful();
        assert_eq!(entry.reliability_score(), 1.0);
        
        // After some failures
        entry.mark_failed();
        assert_eq!(entry.reliability_score(), 2.0 / 3.0);
    }
}