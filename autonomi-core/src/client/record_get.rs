// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Generic record retrieval implementation for the Autonomi core client
//!
//! This module provides a unified interface for retrieving different types of records
//! from the network, with appropriate conflict resolution for each data type.

use super::{Client, DataContent, Error, GetError};
use crate::client::{
    data_types::{
        graph::deserialize_graph_entry_from_record, pointer::resolve_pointer_split,
        scratchpad::resolve_scratchpad_split,
    },
    utils::determine_data_type_from_address,
};
use crate::networking::{NetworkError, Strategy};

use ant_protocol::NetworkAddress;
use ant_protocol::storage::{
    Chunk, DataTypes, Pointer, RecordHeader, RecordKind, Scratchpad, try_deserialize_record,
};
use libp2p::kad::Record;
use tracing::{debug, error, warn};

impl Client {
    /// Retrieve a record from the network using its address.
    /// Note: as `NetworkAddress` indicates the data type, the handling workflow can use that to deploy correspondent `conflict handling` scheme during the fetch.
    pub async fn record_get(&self, address: &NetworkAddress) -> Result<DataContent, Error> {
        // Determine the data type from the address
        let data_type = determine_data_type_from_address(address)?;
        let strategy = self.get_strategy(data_type);

        debug!("Fetching {:?} from network at: {:?}", data_type, address);

        match data_type {
            DataTypes::Chunk => {
                let record = self
                    .network
                    .get_record_with_retries(address.clone(), strategy)
                    .await
                    .map_err(|err| Error::GetError(GetError::Network(err)))?
                    .ok_or(Error::GetError(GetError::RecordNotFound))?;

                let chunk = deserialize_chunk_from_record(&record)?;
                Ok(DataContent::Chunk(chunk))
            }
            DataTypes::GraphEntry => {
                let record = self
                    .network
                    .get_record_with_retries(address.clone(), strategy)
                    .await
                    .map_err(|err| Error::GetError(GetError::Network(err)))?
                    .ok_or(Error::GetError(GetError::RecordNotFound))?;

                Ok(DataContent::GraphEntry(
                    deserialize_graph_entry_from_record(&record)?,
                ))
            }
            DataTypes::Pointer => {
                let pointer = match self
                    .network
                    .get_record_with_retries(address.clone(), strategy)
                    .await
                {
                    Ok(Some(record)) => {
                        try_deserialize_record::<Pointer>(&record).map_err(GetError::Protocol)?
                    }
                    Ok(None) => return Err(Error::GetError(GetError::RecordNotFound)),
                    Err(NetworkError::SplitRecord(result_map)) => {
                        resolve_pointer_split(result_map, address.clone())?
                    }
                    Err(err) => {
                        error!("Error fetching pointer: {err:?}");
                        return Err(Error::GetError(GetError::Network(err)));
                    }
                };

                verify_pointer(&pointer)?;
                Ok(DataContent::Pointer(pointer))
            }
            DataTypes::Scratchpad => {
                match self
                    .network
                    .get_record_with_retries(address.clone(), strategy)
                    .await
                {
                    Ok(Some(record)) => {
                        let scratchpad = try_deserialize_record::<Scratchpad>(&record)
                            .map_err(GetError::Protocol)?;
                        verify_scratchpad(&scratchpad)?;
                        Ok(DataContent::Scratchpad(scratchpad))
                    }
                    Ok(None) => Err(Error::GetError(GetError::RecordNotFound)),
                    Err(NetworkError::SplitRecord(result_map)) => Ok(DataContent::Scratchpad(
                        resolve_scratchpad_split(result_map, address.clone())?,
                    )),
                    Err(e) => {
                        warn!("Failed to fetch scratchpad {address:?} from network: {e}");
                        Err(Error::GetError(GetError::Network(e)))
                    }
                }
            }
        }
    }

    pub(crate) fn get_strategy(&self, data_type: DataTypes) -> &Strategy {
        match data_type {
            DataTypes::Chunk => &self.config.chunks,
            DataTypes::GraphEntry => &self.config.graph_entry,
            DataTypes::Pointer => &self.config.pointer,
            DataTypes::Scratchpad => &self.config.scratchpad,
        }
    }
}

// Helper methods for record deserialization

pub(crate) fn deserialize_chunk_from_record(record: &Record) -> Result<Chunk, Error> {
    let header =
        RecordHeader::from_record(record).map_err(|e| Error::GetError(GetError::Protocol(e)))?;

    if matches!(
        header.kind,
        RecordKind::DataOnly(DataTypes::Chunk) | RecordKind::DataWithPayment(DataTypes::Chunk)
    ) {
        let chunk: Chunk =
            try_deserialize_record(record).map_err(|e| Error::GetError(GetError::Protocol(e)))?;
        Ok(chunk)
    } else {
        error!(
            "Record kind mismatch: expected Chunk, got {:?}",
            header.kind
        );
        Err(Error::GetError(GetError::RecordKindMismatch(
            RecordKind::DataOnly(DataTypes::Chunk),
        )))
    }
}

fn verify_pointer(pointer: &ant_protocol::storage::Pointer) -> Result<(), Error> {
    if !pointer.verify_signature() {
        return Err(Error::GetError(GetError::Configuration(
            "Pointer signature is invalid".to_string(),
        )));
    }
    Ok(())
}

fn verify_scratchpad(scratchpad: &ant_protocol::storage::Scratchpad) -> Result<(), Error> {
    if !scratchpad.verify_signature() {
        return Err(Error::GetError(GetError::Configuration(
            "Scratchpad signature is invalid".to_string(),
        )));
    }
    if scratchpad.is_too_big() {
        return Err(Error::GetError(GetError::Configuration(format!(
            "Scratchpad size is too big: {} > {}",
            scratchpad.size(),
            ant_protocol::storage::Scratchpad::MAX_SIZE
        ))));
    }
    Ok(())
}
