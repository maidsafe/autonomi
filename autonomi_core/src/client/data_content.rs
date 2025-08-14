// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Data content types and operations for the Autonomi core client

use ant_evm::ClientProofOfPayment;
use ant_protocol::storage::{
    try_serialize_record, Chunk, DataTypes, GraphEntry, Pointer, RecordKind, Scratchpad,
};
use ant_protocol::NetworkAddress;
use libp2p::kad::Record;
use xor_name::XorName;

use super::Error;
use crate::PutError;

/// Content returned from the network
#[derive(Debug, Clone)]
pub enum DataContent {
    /// Chunk data contains raw Bytes
    /// could be datamap, private, archive, chunked_content, etc.
    Chunk(Chunk),
    /// Graph entry data
    GraphEntry(GraphEntry),
    /// Pointer data
    Pointer(Pointer),
    /// Scratchpad data
    Scratchpad(Scratchpad),
    /// Split copies of GraphEntry
    GraphEntrySplit(Vec<GraphEntry>),
    /// Split copies of Scratchpad
    ScratchpadSplit(Vec<Scratchpad>),
}

impl DataContent {
    /// Returns the corresponding [`DataTypes`] for this content.
    ///
    /// This helper function maps each variant of `DataContent` to its corresponding
    /// `DataTypes` variant, allowing easy identification of the content type.
    ///
    /// # Returns
    ///
    /// The `DataTypes` that corresponds to this content variant.
    ///
    /// # Examples
    ///
    /// ```
    /// use autonomi_core::client::{DataContent, DataTypes};
    /// use ant_protocol::storage::Chunk;
    ///
    /// let chunk_content = DataContent::Chunk(Chunk::new(vec![1, 2, 3]));
    /// assert_eq!(chunk_content.data_types(), DataTypes::Chunk);
    /// ```
    pub(crate) fn data_types(&self) -> DataTypes {
        match self {
            Self::Chunk(_) => DataTypes::Chunk,
            Self::GraphEntry(_) | Self::GraphEntrySplit(_) => DataTypes::GraphEntry,
            Self::Pointer(_) => DataTypes::Pointer,
            Self::Scratchpad(_) | Self::ScratchpadSplit(_) => DataTypes::Scratchpad,
        }
    }

    /// Extract content information for payment and upload
    pub(crate) fn get_content_info(&self) -> (XorName, usize, NetworkAddress) {
        match &self {
            DataContent::Chunk(chunk) => (*chunk.name(), chunk.size(), chunk.network_address()),
            DataContent::GraphEntry(graph_entry) => {
                let addr = graph_entry.address();
                (
                    addr.xorname(),
                    graph_entry.size(),
                    NetworkAddress::from(addr),
                )
            }
            DataContent::Pointer(pointer) => {
                let addr = pointer.address();
                (addr.xorname(), Pointer::size(), NetworkAddress::from(addr))
            }
            DataContent::Scratchpad(scratchpad) => (
                scratchpad.address().xorname(),
                scratchpad.size(),
                scratchpad.network_address(),
            ),
            DataContent::GraphEntrySplit(entries) => {
                let total_size = entries.iter().map(|entry| entry.size()).sum();
                let addr = entries[0].address();
                (
                    entries[0].address().xorname(),
                    total_size,
                    NetworkAddress::from(addr),
                )
            }
            DataContent::ScratchpadSplit(entries) => {
                let total_size = entries.iter().map(|entry| entry.size()).sum();
                (
                    entries[0].address().xorname(),
                    total_size,
                    entries[0].network_address(),
                )
            }
        }
    }

    /// Prepare a record for storage on the network
    pub(crate) fn prepare_record(
        &self,
        receipt: Option<ClientProofOfPayment>,
    ) -> Result<Record, Error> {
        let (_xor_name, _size, network_address) = self.get_content_info();
        let data_type = self.data_types();
        let record_value = if let Some(receipt) = receipt {
            // Create record with payment proof
            let record_kind = RecordKind::DataWithPayment(data_type);
            match &self {
                DataContent::Chunk(chunk) => {
                    try_serialize_record(&(receipt.to_proof_of_payment(), chunk), record_kind)
                }
                DataContent::GraphEntry(graph_entry) => {
                    try_serialize_record(&(receipt.to_proof_of_payment(), graph_entry), record_kind)
                }
                DataContent::Pointer(pointer) => {
                    try_serialize_record(&(receipt.to_proof_of_payment(), pointer), record_kind)
                }
                DataContent::Scratchpad(scratchpad) => {
                    try_serialize_record(&(receipt.to_proof_of_payment(), scratchpad), record_kind)
                }
                DataContent::GraphEntrySplit(_entries) => {
                    return Err(Error::PutError(PutError::Serialization(
                        "Get split GraphEntry: {_entries:?}".to_string(),
                    )));
                }
                DataContent::ScratchpadSplit(_entries) => {
                    return Err(Error::PutError(PutError::Serialization(
                        "Get split Scratchpad: {_entries:?}".to_string(),
                    )));
                }
            }
            .map_err(|e| {
                Error::PutError(PutError::Serialization(format!(
                    "Failed to serialize {data_type:?} with payment: {e:?}"
                )))
            })?
        } else {
            // Create record without payment proof
            let record_kind = RecordKind::DataOnly(data_type);
            match &self {
                DataContent::Chunk(chunk) => try_serialize_record(&chunk, record_kind),
                DataContent::GraphEntry(graph_entry) => {
                    try_serialize_record(&graph_entry, record_kind)
                }
                DataContent::Pointer(pointer) => try_serialize_record(&pointer, record_kind),
                DataContent::Scratchpad(scratchpad) => {
                    try_serialize_record(&scratchpad, record_kind)
                }
                DataContent::GraphEntrySplit(_entries) => {
                    return Err(Error::PutError(PutError::Serialization(
                        "Get split GraphEntry: {_entries:?}".to_string(),
                    )));
                }
                DataContent::ScratchpadSplit(_entries) => {
                    return Err(Error::PutError(PutError::Serialization(
                        "Get split Scratchpad: {_entries:?}".to_string(),
                    )));
                }
            }
            .map_err(|e| {
                Error::PutError(PutError::Serialization(format!(
                    "Failed to serialize {data_type:?} : {e:?}"
                )))
            })?
        };

        Ok(Record {
            key: network_address.to_record_key(),
            value: record_value.to_vec(),
            publisher: None,
            expires: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::DataContent;
    use ant_protocol::storage::{Chunk, ChunkAddress, DataTypes, PointerTarget};
    use ant_protocol::Bytes;
    use bls::SecretKey;

    #[test]
    fn test_data_content_chunk_type() -> Result<(), Box<dyn std::error::Error>> {
        let chunk = Chunk::new(Bytes::from(vec![1, 2, 3, 4]));
        let content = DataContent::Chunk(chunk);

        assert_eq!(content.data_types(), DataTypes::Chunk);
        Ok(())
    }

    #[test]
    fn test_data_content_graph_entry_type() -> Result<(), Box<dyn std::error::Error>> {
        let secret_key = SecretKey::random();
        let content = [0u8; 32];
        let parents = vec![];
        let outputs = vec![];

        let graph_entry =
            ant_protocol::storage::GraphEntry::new(&secret_key, parents, content, outputs);
        let content = DataContent::GraphEntry(graph_entry);

        assert_eq!(content.data_types(), DataTypes::GraphEntry);
        Ok(())
    }

    #[test]
    fn test_data_content_pointer_type() -> Result<(), Box<dyn std::error::Error>> {
        let secret_key = SecretKey::random();
        let target = PointerTarget::ChunkAddress(ChunkAddress::new(xor_name::XorName::random(
            &mut rand::thread_rng(),
        )));

        let pointer = ant_protocol::storage::Pointer::new(&secret_key, 0u64, target);
        let content = DataContent::Pointer(pointer);

        assert_eq!(content.data_types(), DataTypes::Pointer);
        Ok(())
    }

    #[test]
    fn test_data_content_scratchpad_type() -> Result<(), Box<dyn std::error::Error>> {
        let secret_key = SecretKey::random();
        let content_type = 1u64;
        let data = Bytes::from(vec![1, 2, 3, 4]);

        let scratchpad =
            ant_protocol::storage::Scratchpad::new(&secret_key, content_type, &data, 0u64);
        let content = DataContent::Scratchpad(scratchpad);

        assert_eq!(content.data_types(), DataTypes::Scratchpad);
        Ok(())
    }

    #[test]
    fn test_all_data_content_types() -> Result<(), Box<dyn std::error::Error>> {
        // Test that all DataTypes variants are covered
        let chunk = Chunk::new(Bytes::from(vec![1, 2, 3]));
        let chunk_content = DataContent::Chunk(chunk);
        assert_eq!(chunk_content.data_types(), DataTypes::Chunk);

        let secret_key = SecretKey::random();
        let content = [0u8; 32];
        let graph_entry =
            ant_protocol::storage::GraphEntry::new(&secret_key, vec![], content, vec![]);
        let graph_content = DataContent::GraphEntry(graph_entry);
        assert_eq!(graph_content.data_types(), DataTypes::GraphEntry);

        let target = PointerTarget::ChunkAddress(ChunkAddress::new(xor_name::XorName::random(
            &mut rand::thread_rng(),
        )));
        let pointer = ant_protocol::storage::Pointer::new(&secret_key, 0u64, target);
        let pointer_content = DataContent::Pointer(pointer);
        assert_eq!(pointer_content.data_types(), DataTypes::Pointer);

        let scratchpad = ant_protocol::storage::Scratchpad::new(
            &secret_key,
            1u64,
            &Bytes::from(vec![1, 2, 3]),
            0u64,
        );
        let scratchpad_content = DataContent::Scratchpad(scratchpad);
        assert_eq!(scratchpad_content.data_types(), DataTypes::Scratchpad);

        Ok(())
    }
}
