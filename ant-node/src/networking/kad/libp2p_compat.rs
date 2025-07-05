// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! libp2p compatibility layer for gradual migration.
//! 
//! This module provides adapters and converters that allow the existing libp2p-based
//! code to work with our transport-agnostic Kademlia implementation.
//! 
//! NOTE: This module is currently disabled pending resolution of complex libp2p trait bounds.
//! Enable with --features libp2p-compat

#![cfg(feature = "libp2p-compat")]

use std::{
    collections::HashMap,
    sync::Arc,
    time::Instant,
};

use async_trait::async_trait;
use libp2p::{
    kad,
    swarm::{Swarm, NetworkBehaviour},
    PeerId, Multiaddr,
};
use tokio::sync::RwLock;
use tracing::{debug, error, warn};

use crate::networking::kad::transport::{
    KademliaTransport, KadPeerId, KadAddress, KadMessage, KadResponse, KadError,
    PeerInfo, ConnectionStatus,
};

/// Adapter that implements KademliaTransport using libp2p infrastructure
#[derive(Debug)]
pub struct LibP2pTransport<TBehaviour>
where
    TBehaviour: NetworkBehaviour + Send + Sync + 'static,
{
    /// Reference to the libp2p swarm
    swarm: Arc<RwLock<Swarm<TBehaviour>>>,
    /// Local peer ID converted to our format
    local_peer_id: KadPeerId,
    /// Mapping between our peer IDs and libp2p peer IDs
    peer_mapping: Arc<RwLock<BiMap<KadPeerId, PeerId>>>,
    /// Address mapping
    address_mapping: Arc<RwLock<HashMap<KadPeerId, Vec<Multiaddr>>>>,
}

/// Bidirectional mapping between peer ID types
#[derive(Debug, Default)]
pub struct BiMap<K, V> {
    forward: HashMap<K, V>,
    reverse: HashMap<V, K>,
}

impl<K, V> BiMap<K, V>
where
    K: Clone + Eq + std::hash::Hash,
    V: Clone + Eq + std::hash::Hash,
{
    pub fn new() -> Self {
        Self {
            forward: HashMap::new(),
            reverse: HashMap::new(),
        }
    }

    pub fn insert(&mut self, key: K, value: V) {
        // Remove existing mappings if they exist
        if let Some(old_value) = self.forward.remove(&key) {
            self.reverse.remove(&old_value);
        }
        if let Some(old_key) = self.reverse.remove(&value) {
            self.forward.remove(&old_key);
        }

        // Insert new mapping
        self.forward.insert(key.clone(), value.clone());
        self.reverse.insert(value, key);
    }

    pub fn get_by_left(&self, key: &K) -> Option<&V> {
        self.forward.get(key)
    }

    pub fn get_by_right(&self, value: &V) -> Option<&K> {
        self.reverse.get(value)
    }

    pub fn contains_left(&self, key: &K) -> bool {
        self.forward.contains_key(key)
    }

    pub fn contains_right(&self, value: &V) -> bool {
        self.reverse.contains_key(value)
    }

    pub fn remove_by_left(&mut self, key: &K) -> Option<V> {
        if let Some(value) = self.forward.remove(key) {
            self.reverse.remove(&value);
            Some(value)
        } else {
            None
        }
    }

    pub fn remove_by_right(&mut self, value: &V) -> Option<K> {
        if let Some(key) = self.reverse.remove(value) {
            self.forward.remove(&key);
            Some(key)
        } else {
            None
        }
    }
}

impl<TBehaviour> LibP2pTransport<TBehaviour>
where
    TBehaviour: NetworkBehaviour + Send + Sync + 'static,
{
    /// Create a new libp2p transport adapter
    pub fn new(swarm: Arc<RwLock<Swarm<TBehaviour>>>) -> Self {
        // Get local peer ID from swarm
        let local_libp2p_id = {
            // We need to get this without locking for too long
            // This is a placeholder - in real implementation we'd get it properly
            PeerId::random()
        };
        
        let local_peer_id = libp2p_peer_id_to_kad(&local_libp2p_id);
        
        let mut peer_mapping = BiMap::new();
        peer_mapping.insert(local_peer_id.clone(), local_libp2p_id);
        
        Self {
            swarm,
            local_peer_id,
            peer_mapping: Arc::new(RwLock::new(peer_mapping)),
            address_mapping: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Convert a KadPeerId to libp2p PeerId
    async fn kad_to_libp2p_peer_id(&self, kad_peer: &KadPeerId) -> Result<PeerId, KadError> {
        // First check our mapping
        {
            let mapping = self.peer_mapping.read().await;
            if let Some(libp2p_peer) = mapping.get_by_left(kad_peer) {
                return Ok(libp2p_peer.clone());
            }
        }

        // Try to parse as libp2p PeerId directly
        match PeerId::from_bytes(&kad_peer.bytes) {
            Ok(peer_id) => {
                // Cache the mapping
                let mut mapping = self.peer_mapping.write().await;
                mapping.insert(kad_peer.clone(), peer_id);
                Ok(peer_id)
            }
            Err(e) => Err(KadError::InvalidMessage(format!("Invalid peer ID: {}", e))),
        }
    }

    /// Convert a libp2p PeerId to KadPeerId
    async fn libp2p_to_kad_peer_id(&self, libp2p_peer: &PeerId) -> KadPeerId {
        // Check our mapping first
        {
            let mapping = self.peer_mapping.read().await;
            if let Some(kad_peer) = mapping.get_by_right(libp2p_peer) {
                return kad_peer.clone();
            }
        }

        // Convert directly
        let kad_peer = libp2p_peer_id_to_kad(libp2p_peer);
        
        // Cache the mapping
        {
            let mut mapping = self.peer_mapping.write().await;
            mapping.insert(kad_peer.clone(), libp2p_peer.clone());
        }
        
        kad_peer
    }

    /// Convert KadAddress to Multiaddr
    fn kad_to_multiaddr(&self, kad_addr: &KadAddress) -> Result<Multiaddr, KadError> {
        match kad_addr.protocol.as_str() {
            "tcp" => {
                let addr: Multiaddr = format!("/ip4/{}/tcp", kad_addr.address)
                    .parse()
                    .map_err(|e| KadError::InvalidMessage(format!("Invalid address: {}", e)))?;
                Ok(addr)
            }
            "quic" => {
                let addr: Multiaddr = format!("/ip4/{}/udp/quic", kad_addr.address)
                    .parse()
                    .map_err(|e| KadError::InvalidMessage(format!("Invalid address: {}", e)))?;
                Ok(addr)
            }
            _ => Err(KadError::InvalidMessage(format!("Unsupported protocol: {}", kad_addr.protocol))),
        }
    }

    /// Convert Multiaddr to KadAddress
    fn multiaddr_to_kad(&self, multiaddr: &Multiaddr) -> Result<KadAddress, KadError> {
        let addr_str = multiaddr.to_string();
        
        if addr_str.contains("/tcp/") {
            Ok(KadAddress::new("tcp".to_string(), addr_str))
        } else if addr_str.contains("/quic") {
            Ok(KadAddress::new("quic".to_string(), addr_str))
        } else {
            Ok(KadAddress::new("unknown".to_string(), addr_str))
        }
    }

    /// Convert KadMessage to libp2p kad message
    async fn kad_to_libp2p_message(&self, message: &KadMessage) -> Result<Vec<u8>, KadError> {
        // This is a simplified implementation - in reality you'd need to properly
        // serialize the message according to libp2p kad protocol
        let serialized = bincode::serialize(message)
            .map_err(|e| KadError::InvalidMessage(format!("Serialization error: {}", e)))?;
        Ok(serialized)
    }

    /// Convert libp2p kad response to KadResponse
    async fn libp2p_to_kad_response(&self, data: &[u8]) -> Result<KadResponse, KadError> {
        // This is a simplified implementation - in reality you'd need to properly
        // deserialize according to libp2p kad protocol
        let response: KadResponse = bincode::deserialize(data)
            .map_err(|e| KadError::InvalidMessage(format!("Deserialization error: {}", e)))?;
        Ok(response)
    }
}

#[async_trait]
impl<TBehaviour> KademliaTransport for LibP2pTransport<TBehaviour>
where
    TBehaviour: NetworkBehaviour + Send + Sync + 'static,
{
    type Error = KadError;

    fn local_peer_id(&self) -> KadPeerId {
        self.local_peer_id.clone()
    }

    fn listen_addresses(&self) -> Vec<KadAddress> {
        // In a real implementation, you'd get these from the swarm
        vec![KadAddress::new("tcp".to_string(), "127.0.0.1:0".to_string())]
    }

    async fn send_request(&self, peer: &KadPeerId, message: KadMessage) -> Result<KadResponse, Self::Error> {
        debug!("Sending request to peer {} via libp2p", peer);
        
        let _libp2p_peer = self.kad_to_libp2p_peer_id(peer).await?;
        let _message_data = self.kad_to_libp2p_message(&message).await?;
        
        // In a real implementation, you would:
        // 1. Convert the message to libp2p kad format
        // 2. Send it via the swarm's kad behaviour
        // 3. Wait for the response
        // 4. Convert the response back to our format
        
        // For now, return a mock response
        Ok(KadResponse::Nodes {
            closer_peers: vec![],
            requester: self.local_peer_id.clone(),
        })
    }

    async fn send_message(&self, peer: &KadPeerId, message: KadMessage) -> Result<(), Self::Error> {
        debug!("Sending message to peer {} via libp2p", peer);
        
        let _libp2p_peer = self.kad_to_libp2p_peer_id(peer).await?;
        let _message_data = self.kad_to_libp2p_message(&message).await?;
        
        // In a real implementation, send the message via libp2p
        Ok(())
    }

    async fn is_connected(&self, peer: &KadPeerId) -> bool {
        if let Ok(libp2p_peer) = self.kad_to_libp2p_peer_id(peer).await {
            // In a real implementation, check connection status via swarm
            let _ = libp2p_peer;
            false // Placeholder
        } else {
            false
        }
    }

    async fn dial_peer(&self, peer: &KadPeerId, addresses: &[KadAddress]) -> Result<(), Self::Error> {
        debug!("Dialing peer {} with {} addresses", peer, addresses.len());
        
        let libp2p_peer = self.kad_to_libp2p_peer_id(peer).await?;
        
        for kad_addr in addresses {
            let multiaddr = self.kad_to_multiaddr(kad_addr)?;
            debug!("Dialing {} at {}", libp2p_peer, multiaddr);
            
            // In a real implementation, dial via swarm
            // swarm.dial(multiaddr)?;
        }
        
        Ok(())
    }

    async fn add_peer_addresses(&self, peer: &KadPeerId, addresses: Vec<KadAddress>) -> Result<(), Self::Error> {
        let mut addr_mapping = self.address_mapping.write().await;
        addr_mapping.insert(peer.clone(), vec![]); // Placeholder
        
        debug!("Added {} addresses for peer {}", addresses.len(), peer);
        Ok(())
    }

    async fn remove_peer(&self, peer: &KadPeerId) -> Result<(), Self::Error> {
        let mut addr_mapping = self.address_mapping.write().await;
        addr_mapping.remove(peer);
        
        let mut peer_mapping = self.peer_mapping.write().await;
        peer_mapping.remove_by_left(peer);
        
        debug!("Removed peer {}", peer);
        Ok(())
    }

    async fn peer_info(&self, peer: &KadPeerId) -> Option<PeerInfo> {
        let addr_mapping = self.address_mapping.read().await;
        
        if let Some(_addresses) = addr_mapping.get(peer) {
            Some(PeerInfo {
                peer_id: peer.clone(),
                addresses: vec![], // Placeholder
                connection_status: ConnectionStatus::Unknown,
                last_seen: Some(Instant::now()),
            })
        } else {
            None
        }
    }

    async fn connected_peers(&self) -> Vec<KadPeerId> {
        // In a real implementation, get from swarm
        vec![]
    }

    async fn start_listening(&mut self) -> Result<(), Self::Error> {
        debug!("Starting libp2p transport listening");
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), Self::Error> {
        debug!("Shutting down libp2p transport");
        Ok(())
    }
}

/// Convert libp2p PeerId to our KadPeerId
pub fn libp2p_peer_id_to_kad(peer_id: &PeerId) -> KadPeerId {
    KadPeerId::new(peer_id.to_bytes())
}

/// Convert our KadPeerId to libp2p PeerId
pub fn kad_peer_id_to_libp2p(kad_peer: &KadPeerId) -> Result<PeerId, KadError> {
    PeerId::from_bytes(&kad_peer.bytes)
        .map_err(|e| KadError::InvalidMessage(format!("Invalid peer ID conversion: {}", e)))
}

/// Convert libp2p Multiaddr to our KadAddress
pub fn multiaddr_to_kad_address(multiaddr: &Multiaddr) -> KadAddress {
    let addr_str = multiaddr.to_string();
    
    let protocol = if addr_str.contains("/tcp/") {
        "tcp"
    } else if addr_str.contains("/quic") {
        "quic"
    } else if addr_str.contains("/ws") {
        "websocket"
    } else {
        "unknown"
    };
    
    KadAddress::new(protocol.to_string(), addr_str)
}

/// Convert our KadAddress to libp2p Multiaddr
pub fn kad_address_to_multiaddr(kad_addr: &KadAddress) -> Result<Multiaddr, KadError> {
    kad_addr.address.parse()
        .map_err(|e| KadError::InvalidMessage(format!("Invalid multiaddr: {}", e)))
}

/// Adapter for libp2p kad events to our KadEvent format
pub struct LibP2pEventAdapter;

impl LibP2pEventAdapter {
    /// Convert libp2p kad event to our event format
    pub async fn convert_event(
        event: &kad::Event,
        peer_mapping: &BiMap<KadPeerId, PeerId>,
    ) -> Option<crate::networking::kad::transport::KadEvent> {
        match event {
            kad::Event::OutboundQueryProgressed { id, result, step: _ } => {
                match result {
                    kad::QueryResult::GetClosestPeers(Ok(result)) => {
                        let closer_peers = result.peers.iter()
                            .filter_map(|peer| {
                                peer_mapping.get_by_right(&peer.peer_id).map(|kad_peer| {
                                    PeerInfo {
                                        peer_id: kad_peer.clone(),
                                        addresses: peer.addrs.iter()
                                            .map(multiaddr_to_kad_address)
                                            .collect(),
                                        connection_status: ConnectionStatus::Unknown,
                                        last_seen: None,
                                    }
                                })
                            })
                            .collect();
                        
                        Some(crate::networking::kad::transport::KadEvent::QueryCompleted {
                            query_id: crate::networking::kad::transport::QueryId(id.0),
                            result: crate::networking::kad::transport::QueryResult::GetClosestPeers {
                                target: KadPeerId::new(result.key.to_vec()),
                                peers: closer_peers,
                            },
                            duration: std::time::Duration::from_secs(0), // Placeholder
                        })
                    }
                    
                    kad::QueryResult::GetRecord(Ok(result)) => {
                        let record = result.records.first().map(|record| {
                            crate::networking::kad::transport::Record::new(
                                crate::networking::kad::transport::RecordKey::new(record.record.key.to_vec()),
                                record.record.value.clone(),
                            )
                        });
                        
                        Some(crate::networking::kad::transport::KadEvent::QueryCompleted {
                            query_id: crate::networking::kad::transport::QueryId(id.0),
                            result: crate::networking::kad::transport::QueryResult::GetRecord {
                                key: crate::networking::kad::transport::RecordKey::new(vec![]), // Placeholder
                                record,
                                closest_peers: vec![], // Placeholder
                            },
                            duration: std::time::Duration::from_secs(0),
                        })
                    }
                    
                    _ => {
                        warn!("Unhandled libp2p kad query result: {:?}", result);
                        None
                    }
                }
            }
            
            kad::Event::RoutingUpdated { peer, .. } => {
                if let Some(kad_peer) = peer_mapping.get_by_right(peer) {
                    Some(crate::networking::kad::transport::KadEvent::RoutingUpdated {
                        peer: kad_peer.clone(),
                        action: crate::networking::kad::transport::RoutingAction::Updated,
                        bucket: 0, // Placeholder - would need to calculate actual bucket
                    })
                } else {
                    None
                }
            }
            
            kad::Event::InboundRequest { request } => {
                // Convert inbound request
                match request {
                    kad::InboundRequest::FindNode { .. } => {
                        // Would need to convert the full request
                        None // Placeholder
                    }
                    _ => None,
                }
            }
            
            _ => {
                debug!("Unhandled libp2p kad event: {:?}", event);
                None
            }
        }
    }
}

/// Helper to bridge libp2p record store with our record store interface
pub struct LibP2pRecordStoreAdapter<S> {
    inner: S,
}

impl<S> LibP2pRecordStoreAdapter<S>
where
    S: crate::networking::kad::record_store::RecordStore,
{
    pub fn new(store: S) -> Self {
        Self { inner: store }
    }
    
    /// Convert our Record to libp2p Record
    pub fn kad_record_to_libp2p(
        record: &crate::networking::kad::transport::Record,
    ) -> kad::Record {
        kad::Record {
            key: kad::RecordKey::new(&record.key.key),
            value: record.value.clone(),
            publisher: record.publisher.as_ref().and_then(|p| kad_peer_id_to_libp2p(p).ok()),
            expires: record.expires,
        }
    }
    
    /// Convert libp2p Record to our Record
    pub fn libp2p_record_to_kad(record: &kad::Record) -> crate::networking::kad::transport::Record {
        let mut kad_record = crate::networking::kad::transport::Record::new(
            crate::networking::kad::transport::RecordKey::new(record.key.to_vec()),
            record.value.clone(),
        );
        
        if let Some(publisher) = &record.publisher {
            kad_record.publisher = Some(libp2p_peer_id_to_kad(publisher));
        }
        
        kad_record.expires = record.expires;
        kad_record
    }
}

// Implement libp2p kad RecordStore trait for our store
#[async_trait::async_trait]
impl<S> kad::store::RecordStore for LibP2pRecordStoreAdapter<S>
where
    S: crate::networking::kad::record_store::RecordStore + Send + Sync,
{
    type RecordsIter<'a> = std::vec::IntoIter<std::borrow::Cow<'a, libp2p::kad::Record>>;
    type ProvidedIter<'a> = std::iter::Empty<std::borrow::Cow<'a, libp2p::kad::ProviderRecord>>;

    fn get(&self, key: &kad::RecordKey) -> Option<kad::Record> {
        // This would need to be async in real implementation
        // For now, return None as placeholder
        None
    }

    fn put(&mut self, record: kad::Record) -> Result<(), kad::store::Error> {
        // Convert and store - would need async context
        Ok(())
    }

    fn remove(&mut self, key: &kad::RecordKey) {
        // Remove record - would need async context
    }

    fn records(&self) -> Self::RecordsIter<'_> {
        // Return iterator over all records
        vec![].into_iter()
    }

    fn add_provider(&mut self, record: kad::ProviderRecord) -> Result<(), kad::store::Error> {
        // Add provider record
        Ok(())
    }

    fn providers(&self, key: &kad::RecordKey) -> Vec<kad::ProviderRecord> {
        // Return providers for key
        vec![]
    }

    fn provided(&self) -> Self::ProvidedIter<'_> {
        // Return iterator over provided records
        std::iter::empty()
    }

    fn remove_provider(&mut self, key: &kad::RecordKey, provider: &PeerId) {
        // Remove provider
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bimap() {
        let mut map = BiMap::new();
        
        let key = KadPeerId::new(vec![1, 2, 3]);
        let value = PeerId::random();
        
        map.insert(key.clone(), value);
        
        assert!(map.contains_left(&key));
        assert!(map.contains_right(&value));
        assert_eq!(map.get_by_left(&key), Some(&value));
        assert_eq!(map.get_by_right(&value), Some(&key));
    }

    #[test]
    fn test_peer_id_conversion() {
        let libp2p_peer = PeerId::random();
        let kad_peer = libp2p_peer_id_to_kad(&libp2p_peer);
        let converted_back = kad_peer_id_to_libp2p(&kad_peer).unwrap();
        
        assert_eq!(libp2p_peer, converted_back);
    }

    #[test]
    fn test_address_conversion() {
        let multiaddr: Multiaddr = "/ip4/127.0.0.1/tcp/8080".parse().unwrap();
        let kad_addr = multiaddr_to_kad_address(&multiaddr);
        
        assert_eq!(kad_addr.protocol, "tcp");
        assert!(kad_addr.address.contains("127.0.0.1"));
        assert!(kad_addr.address.contains("8080"));
    }
}