// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Kademlia protocol implementation for message handling and serialization.
//! 
//! This module defines the wire protocol for Kademlia messages, including
//! serialization, versioning, and message validation.

use std::{
    collections::HashMap,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::networking::kad::transport::{
    KadPeerId, KadAddress, KadMessage, KadResponse, RecordKey, Record, PeerInfo,
    ConnectionStatus, KadError,
};

/// Current protocol version
pub const PROTOCOL_VERSION: u32 = 1;

/// Maximum message size (1MB)
pub const MAX_MESSAGE_SIZE: usize = 1024 * 1024;

/// Protocol magic bytes for message identification
pub const PROTOCOL_MAGIC: &[u8] = b"KADP";

/// Message type identifiers
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    FindNode = 0x01,
    FindValue = 0x02,
    PutValue = 0x03,
    AddProvider = 0x04,
    GetProviders = 0x05,
    Ping = 0x06,
    NodesResponse = 0x10,
    ValueResponse = 0x11,
    ProvidersResponse = 0x12,
    AckResponse = 0x13,
    ErrorResponse = 0x14,
}

impl TryFrom<u8> for MessageType {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(MessageType::FindNode),
            0x02 => Ok(MessageType::FindValue),
            0x03 => Ok(MessageType::PutValue),
            0x04 => Ok(MessageType::AddProvider),
            0x05 => Ok(MessageType::GetProviders),
            0x06 => Ok(MessageType::Ping),
            0x10 => Ok(MessageType::NodesResponse),
            0x11 => Ok(MessageType::ValueResponse),
            0x12 => Ok(MessageType::ProvidersResponse),
            0x13 => Ok(MessageType::AckResponse),
            0x14 => Ok(MessageType::ErrorResponse),
            _ => Err(ProtocolError::UnknownMessageType(value)),
        }
    }
}

/// Protocol-specific errors
#[derive(Error, Debug, Clone)]
pub enum ProtocolError {
    #[error("Unknown message type: {0}")]
    UnknownMessageType(u8),
    
    #[error("Invalid protocol version: expected {expected}, got {actual}")]
    InvalidVersion { expected: u32, actual: u32 },
    
    #[error("Invalid magic bytes")]
    InvalidMagic,
    
    #[error("Message too large: {size} bytes (max: {max})")]
    MessageTooLarge { size: usize, max: usize },
    
    #[error("Serialization error: {0}")]
    Serialization(String),
    
    #[error("Deserialization error: {0}")]
    Deserialization(String),
    
    #[error("Invalid message format: {0}")]
    InvalidFormat(String),
    
    #[error("Checksum mismatch")]
    ChecksumMismatch,
    
    #[error("Message expired")]
    MessageExpired,
}

impl From<ProtocolError> for KadError {
    fn from(error: ProtocolError) -> Self {
        KadError::InvalidMessage(error.to_string())
    }
}

/// Wire format message header
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageHeader {
    /// Protocol magic bytes
    pub magic: [u8; 4],
    /// Protocol version
    pub version: u32,
    /// Message type
    pub message_type: u8,
    /// Message flags
    pub flags: u8,
    /// Message ID for request/response correlation
    pub message_id: u64,
    /// Timestamp when message was created
    pub timestamp: u64,
    /// Time-to-live in seconds
    pub ttl: u32,
    /// Payload length
    pub payload_length: u32,
    /// Checksum of the payload
    pub checksum: u32,
}

impl MessageHeader {
    /// Create a new message header
    pub fn new(message_type: MessageType, message_id: u64, payload_length: u32) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            magic: *PROTOCOL_MAGIC,
            version: PROTOCOL_VERSION,
            message_type: message_type as u8,
            flags: 0,
            message_id,
            timestamp: now,
            ttl: 300, // 5 minutes default TTL
            payload_length,
            checksum: 0, // Will be set when encoding
        }
    }

    /// Check if this message has expired
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        now > self.timestamp + self.ttl as u64
    }

    /// Validate the header
    pub fn validate(&self) -> Result<(), ProtocolError> {
        // Check magic bytes
        if self.magic != *PROTOCOL_MAGIC {
            return Err(ProtocolError::InvalidMagic);
        }

        // Check version
        if self.version != PROTOCOL_VERSION {
            return Err(ProtocolError::InvalidVersion {
                expected: PROTOCOL_VERSION,
                actual: self.version,
            });
        }

        // Check message type
        MessageType::try_from(self.message_type)?;

        // Check payload size
        if self.payload_length as usize > MAX_MESSAGE_SIZE {
            return Err(ProtocolError::MessageTooLarge {
                size: self.payload_length as usize,
                max: MAX_MESSAGE_SIZE,
            });
        }

        // Check expiration
        if self.is_expired() {
            return Err(ProtocolError::MessageExpired);
        }

        Ok(())
    }
}

/// Wire format for peer information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WirePeerInfo {
    pub peer_id: Vec<u8>,
    pub addresses: Vec<WireAddress>,
    pub connection_status: u8,
    pub last_seen: Option<u64>,
}

impl From<&PeerInfo> for WirePeerInfo {
    fn from(peer: &PeerInfo) -> Self {
        Self {
            peer_id: peer.peer_id.bytes.clone(),
            addresses: peer.addresses.iter().map(WireAddress::from).collect(),
            connection_status: match peer.connection_status {
                ConnectionStatus::Connected => 1,
                ConnectionStatus::Disconnected => 2,
                ConnectionStatus::Unknown => 0,
            },
            last_seen: peer.last_seen.map(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            }),
        }
    }
}

impl TryFrom<&WirePeerInfo> for PeerInfo {
    type Error = ProtocolError;

    fn try_from(wire: &WirePeerInfo) -> Result<Self, Self::Error> {
        let addresses = wire.addresses.iter()
            .map(KadAddress::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        let connection_status = match wire.connection_status {
            1 => ConnectionStatus::Connected,
            2 => ConnectionStatus::Disconnected,
            _ => ConnectionStatus::Unknown,
        };

        let last_seen = wire.last_seen.map(|secs| {
            std::time::UNIX_EPOCH + Duration::from_secs(secs)
        }).and_then(|sys_time| {
            sys_time.elapsed().ok().map(|elapsed| Instant::now() - elapsed)
        });

        Ok(PeerInfo {
            peer_id: KadPeerId::new(wire.peer_id.clone()),
            addresses,
            connection_status,
            last_seen,
        })
    }
}

/// Wire format for addresses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireAddress {
    pub protocol: String,
    pub address: String,
}

impl From<&KadAddress> for WireAddress {
    fn from(addr: &KadAddress) -> Self {
        Self {
            protocol: addr.protocol.clone(),
            address: addr.address.clone(),
        }
    }
}

impl TryFrom<&WireAddress> for KadAddress {
    type Error = ProtocolError;

    fn try_from(wire: &WireAddress) -> Result<Self, Self::Error> {
        Ok(KadAddress::new(wire.protocol.clone(), wire.address.clone()))
    }
}

/// Wire format for records
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireRecord {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub publisher: Option<Vec<u8>>,
    pub expires: Option<u64>,
}

impl From<&Record> for WireRecord {
    fn from(record: &Record) -> Self {
        Self {
            key: record.key.key.clone(),
            value: record.value.clone(),
            publisher: record.publisher.as_ref().map(|p| p.bytes.clone()),
            expires: record.expires.map(|exp| {
                exp.duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            }),
        }
    }
}

impl TryFrom<&WireRecord> for Record {
    type Error = ProtocolError;

    fn try_from(wire: &WireRecord) -> Result<Self, Self::Error> {
        let mut record = Record::new(
            RecordKey::new(wire.key.clone()),
            wire.value.clone(),
        );

        if let Some(publisher_bytes) = &wire.publisher {
            record.publisher = Some(KadPeerId::new(publisher_bytes.clone()));
        }

        if let Some(expires_secs) = wire.expires {
            record.expires = Some(std::time::UNIX_EPOCH + Duration::from_secs(expires_secs));
        }

        Ok(record)
    }
}

/// Main protocol handler for encoding/decoding messages
#[derive(Debug)]
pub struct KadProtocol {
    /// Next message ID to use
    next_message_id: u64,
    /// Cache of recent message IDs to detect duplicates
    recent_messages: HashMap<u64, Instant>,
}

impl KadProtocol {
    /// Create a new protocol handler
    pub fn new() -> Self {
        Self {
            next_message_id: 1,
            recent_messages: HashMap::new(),
        }
    }

    /// Get the next message ID
    fn next_message_id(&mut self) -> u64 {
        let id = self.next_message_id;
        self.next_message_id = self.next_message_id.wrapping_add(1);
        id
    }

    /// Calculate checksum for payload
    fn calculate_checksum(payload: &[u8]) -> u32 {
        // Simple CRC32-like checksum
        let mut checksum = 0u32;
        for byte in payload {
            checksum = checksum.wrapping_add(*byte as u32);
            checksum = checksum.rotate_left(1);
        }
        checksum
    }

    /// Encode a message to bytes
    pub fn encode_message(&mut self, message: &KadMessage) -> Result<Bytes, ProtocolError> {
        // Determine message type
        let message_type = match message {
            KadMessage::FindNode { .. } => MessageType::FindNode,
            KadMessage::FindValue { .. } => MessageType::FindValue,
            KadMessage::PutValue { .. } => MessageType::PutValue,
            KadMessage::AddProvider { .. } => MessageType::AddProvider,
            KadMessage::GetProviders { .. } => MessageType::GetProviders,
            KadMessage::Ping { .. } => MessageType::Ping,
        };

        // Serialize payload
        let payload = bincode::serialize(message)
            .map_err(|e| ProtocolError::Serialization(e.to_string()))?;

        // Create header
        let message_id = self.next_message_id();
        let mut header = MessageHeader::new(message_type, message_id, payload.len() as u32);
        header.checksum = Self::calculate_checksum(&payload);

        // Serialize header
        let header_bytes = bincode::serialize(&header)
            .map_err(|e| ProtocolError::Serialization(e.to_string()))?;

        // Combine header and payload
        let mut buffer = BytesMut::with_capacity(header_bytes.len() + payload.len());
        buffer.put_slice(&header_bytes);
        buffer.put_slice(&payload);

        Ok(buffer.freeze())
    }

    /// Encode a response to bytes
    pub fn encode_response(&mut self, response: &KadResponse) -> Result<Bytes, ProtocolError> {
        // Determine message type
        let message_type = match response {
            KadResponse::Nodes { .. } => MessageType::NodesResponse,
            KadResponse::Value { .. } => MessageType::ValueResponse,
            KadResponse::Providers { .. } => MessageType::ProvidersResponse,
            KadResponse::Ack { .. } => MessageType::AckResponse,
            KadResponse::Error { .. } => MessageType::ErrorResponse,
        };

        // Serialize payload
        let payload = bincode::serialize(response)
            .map_err(|e| ProtocolError::Serialization(e.to_string()))?;

        // Create header
        let message_id = self.next_message_id();
        let mut header = MessageHeader::new(message_type, message_id, payload.len() as u32);
        header.checksum = Self::calculate_checksum(&payload);

        // Serialize header
        let header_bytes = bincode::serialize(&header)
            .map_err(|e| ProtocolError::Serialization(e.to_string()))?;

        // Combine header and payload
        let mut buffer = BytesMut::with_capacity(header_bytes.len() + payload.len());
        buffer.put_slice(&header_bytes);
        buffer.put_slice(&payload);

        Ok(buffer.freeze())
    }

    /// Decode message from bytes
    pub fn decode_message(&mut self, mut data: Bytes) -> Result<KadMessage, ProtocolError> {
        // First, try to decode header to get payload length
        let header: MessageHeader = bincode::deserialize(&data)
            .map_err(|e| ProtocolError::Deserialization(e.to_string()))?;

        // Validate header
        header.validate()?;

        // Check for duplicate message
        if let Some(last_seen) = self.recent_messages.get(&header.message_id) {
            if last_seen.elapsed() < Duration::from_secs(60) {
                return Err(ProtocolError::InvalidFormat("Duplicate message".to_string()));
            }
        }
        self.recent_messages.insert(header.message_id, Instant::now());

        // Calculate header size by re-serializing it
        let header_size = bincode::serialized_size(&header)
            .map_err(|e| ProtocolError::Serialization(e.to_string()))? as usize;

        // Skip header to get payload
        if data.len() < header_size + header.payload_length as usize {
            return Err(ProtocolError::InvalidFormat("Incomplete message".to_string()));
        }

        data.advance(header_size);
        let payload = data.split_to(header.payload_length as usize);

        // Verify checksum
        let calculated_checksum = Self::calculate_checksum(&payload);
        if calculated_checksum != header.checksum {
            return Err(ProtocolError::ChecksumMismatch);
        }

        // Deserialize payload based on message type
        let message_type = MessageType::try_from(header.message_type)?;
        match message_type {
            MessageType::FindNode |
            MessageType::FindValue |
            MessageType::PutValue |
            MessageType::AddProvider |
            MessageType::GetProviders |
            MessageType::Ping => {
                bincode::deserialize(&payload)
                    .map_err(|e| ProtocolError::Deserialization(e.to_string()))
            }
            _ => Err(ProtocolError::InvalidFormat("Expected request message".to_string())),
        }
    }

    /// Decode response from bytes
    pub fn decode_response(&mut self, mut data: Bytes) -> Result<KadResponse, ProtocolError> {
        // Decode header
        let header: MessageHeader = bincode::deserialize(&data)
            .map_err(|e| ProtocolError::Deserialization(e.to_string()))?;

        // Validate header
        header.validate()?;

        // Calculate header size
        let header_size = bincode::serialized_size(&header)
            .map_err(|e| ProtocolError::Serialization(e.to_string()))? as usize;

        // Get payload
        if data.len() < header_size + header.payload_length as usize {
            return Err(ProtocolError::InvalidFormat("Incomplete message".to_string()));
        }

        data.advance(header_size);
        let payload = data.split_to(header.payload_length as usize);

        // Verify checksum
        let calculated_checksum = Self::calculate_checksum(&payload);
        if calculated_checksum != header.checksum {
            return Err(ProtocolError::ChecksumMismatch);
        }

        // Deserialize payload based on message type
        let message_type = MessageType::try_from(header.message_type)?;
        match message_type {
            MessageType::NodesResponse |
            MessageType::ValueResponse |
            MessageType::ProvidersResponse |
            MessageType::AckResponse |
            MessageType::ErrorResponse => {
                bincode::deserialize(&payload)
                    .map_err(|e| ProtocolError::Deserialization(e.to_string()))
            }
            _ => Err(ProtocolError::InvalidFormat("Expected response message".to_string())),
        }
    }

    /// Clean up old message IDs from cache
    pub fn cleanup_message_cache(&mut self) {
        let cutoff = Instant::now() - Duration::from_secs(300); // 5 minutes
        self.recent_messages.retain(|_, &mut last_seen| last_seen > cutoff);
    }
}

impl Default for KadProtocol {
    fn default() -> Self {
        Self::new()
    }
}

/// Utility for message validation and sanitization
pub struct MessageValidator;

impl MessageValidator {
    /// Validate a KadMessage for security and correctness
    pub fn validate_message(message: &KadMessage) -> Result<(), ProtocolError> {
        match message {
            KadMessage::FindNode { target, requester } => {
                Self::validate_peer_id(target)?;
                Self::validate_peer_id(requester)?;
            }
            
            KadMessage::FindValue { key, requester } => {
                Self::validate_record_key(key)?;
                Self::validate_peer_id(requester)?;
            }
            
            KadMessage::PutValue { record, requester } => {
                Self::validate_record(record)?;
                Self::validate_peer_id(requester)?;
            }
            
            KadMessage::AddProvider { key, provider, provider_addresses, requester } => {
                Self::validate_record_key(key)?;
                Self::validate_peer_id(provider)?;
                Self::validate_peer_id(requester)?;
                
                for addr in provider_addresses {
                    Self::validate_address(addr)?;
                }
            }
            
            KadMessage::GetProviders { key, requester } => {
                Self::validate_record_key(key)?;
                Self::validate_peer_id(requester)?;
            }
            
            KadMessage::Ping { requester } => {
                Self::validate_peer_id(requester)?;
            }
        }
        
        Ok(())
    }

    /// Validate a KadResponse
    pub fn validate_response(response: &KadResponse) -> Result<(), ProtocolError> {
        match response {
            KadResponse::Nodes { closer_peers, requester } => {
                Self::validate_peer_id(requester)?;
                
                for peer in closer_peers {
                    Self::validate_peer_info(peer)?;
                }
            }
            
            KadResponse::Value { record, closer_peers, requester } => {
                Self::validate_peer_id(requester)?;
                
                if let Some(record) = record {
                    Self::validate_record(record)?;
                }
                
                for peer in closer_peers {
                    Self::validate_peer_info(peer)?;
                }
            }
            
            KadResponse::Providers { key, providers, closer_peers, requester } => {
                Self::validate_record_key(key)?;
                Self::validate_peer_id(requester)?;
                
                for peer in providers {
                    Self::validate_peer_info(peer)?;
                }
                
                for peer in closer_peers {
                    Self::validate_peer_info(peer)?;
                }
            }
            
            KadResponse::Ack { requester } => {
                Self::validate_peer_id(requester)?;
            }
            
            KadResponse::Error { error: _, requester } => {
                Self::validate_peer_id(requester)?;
            }
        }
        
        Ok(())
    }

    fn validate_peer_id(peer_id: &KadPeerId) -> Result<(), ProtocolError> {
        if peer_id.bytes.is_empty() || peer_id.bytes.len() > 256 {
            return Err(ProtocolError::InvalidFormat("Invalid peer ID length".to_string()));
        }
        Ok(())
    }

    fn validate_record_key(key: &RecordKey) -> Result<(), ProtocolError> {
        if key.key.is_empty() || key.key.len() > 256 {
            return Err(ProtocolError::InvalidFormat("Invalid record key length".to_string()));
        }
        Ok(())
    }

    fn validate_record(record: &Record) -> Result<(), ProtocolError> {
        Self::validate_record_key(&record.key)?;
        
        if record.value.len() > MAX_MESSAGE_SIZE {
            return Err(ProtocolError::MessageTooLarge {
                size: record.value.len(),
                max: MAX_MESSAGE_SIZE,
            });
        }
        
        if let Some(publisher) = &record.publisher {
            Self::validate_peer_id(publisher)?;
        }
        
        Ok(())
    }

    fn validate_peer_info(peer: &PeerInfo) -> Result<(), ProtocolError> {
        Self::validate_peer_id(&peer.peer_id)?;
        
        if peer.addresses.len() > 10 {
            return Err(ProtocolError::InvalidFormat("Too many addresses".to_string()));
        }
        
        for addr in &peer.addresses {
            Self::validate_address(addr)?;
        }
        
        Ok(())
    }

    fn validate_address(addr: &KadAddress) -> Result<(), ProtocolError> {
        if addr.protocol.is_empty() || addr.address.is_empty() {
            return Err(ProtocolError::InvalidFormat("Empty address field".to_string()));
        }
        
        if addr.protocol.len() > 32 || addr.address.len() > 256 {
            return Err(ProtocolError::InvalidFormat("Address field too long".to_string()));
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_type_conversion() {
        assert_eq!(MessageType::try_from(0x01).unwrap(), MessageType::FindNode);
        assert_eq!(MessageType::try_from(0x10).unwrap(), MessageType::NodesResponse);
        assert!(MessageType::try_from(0xFF).is_err());
    }

    #[test]
    fn test_message_header() {
        let header = MessageHeader::new(MessageType::FindNode, 123, 456);
        
        assert_eq!(header.magic, *PROTOCOL_MAGIC);
        assert_eq!(header.version, PROTOCOL_VERSION);
        assert_eq!(header.message_type, MessageType::FindNode as u8);
        assert_eq!(header.message_id, 123);
        assert_eq!(header.payload_length, 456);
        
        assert!(header.validate().is_ok());
    }

    #[test]
    fn test_checksum_calculation() {
        let data = b"hello world";
        let checksum1 = KadProtocol::calculate_checksum(data);
        let checksum2 = KadProtocol::calculate_checksum(data);
        assert_eq!(checksum1, checksum2);

        let other_data = b"hello worlD";
        let checksum3 = KadProtocol::calculate_checksum(other_data);
        assert_ne!(checksum1, checksum3);
    }

    #[tokio::test]
    async fn test_message_encoding_decoding() {
        let mut protocol = KadProtocol::new();
        
        let original_message = KadMessage::FindNode {
            target: KadPeerId::new(vec![1, 2, 3, 4]),
            requester: KadPeerId::new(vec![5, 6, 7, 8]),
        };

        // Encode
        let encoded = protocol.encode_message(&original_message).unwrap();
        
        // Decode
        let decoded_message = protocol.decode_message(encoded).unwrap();
        
        // Compare
        match (&original_message, &decoded_message) {
            (
                KadMessage::FindNode { target: t1, requester: r1 },
                KadMessage::FindNode { target: t2, requester: r2 }
            ) => {
                assert_eq!(t1, t2);
                assert_eq!(r1, r2);
            }
            _ => panic!("Message type mismatch"),
        }
    }

    #[test]
    fn test_message_validation() {
        let valid_message = KadMessage::FindNode {
            target: KadPeerId::new(vec![1, 2, 3, 4]),
            requester: KadPeerId::new(vec![5, 6, 7, 8]),
        };
        
        assert!(MessageValidator::validate_message(&valid_message).is_ok());

        let invalid_message = KadMessage::FindNode {
            target: KadPeerId::new(vec![]), // Empty peer ID
            requester: KadPeerId::new(vec![5, 6, 7, 8]),
        };
        
        assert!(MessageValidator::validate_message(&invalid_message).is_err());
    }

    #[test]
    fn test_wire_format_conversion() {
        let original_peer = PeerInfo {
            peer_id: KadPeerId::new(vec![1, 2, 3, 4]),
            addresses: vec![
                KadAddress::new("tcp".to_string(), "127.0.0.1:8080".to_string()),
            ],
            connection_status: ConnectionStatus::Connected,
            last_seen: Some(Instant::now()),
        };

        let wire_peer = WirePeerInfo::from(&original_peer);
        let converted_peer = PeerInfo::try_from(&wire_peer).unwrap();

        assert_eq!(original_peer.peer_id, converted_peer.peer_id);
        assert_eq!(original_peer.addresses.len(), converted_peer.addresses.len());
        assert_eq!(original_peer.connection_status, converted_peer.connection_status);
    }
}