// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.
#![allow(clippy::mutable_key_type)] // for the Bytes in NetworkAddress

use crate::error::{NetworkError, Result};
use crate::record_store::{ClientRecordStore, NodeRecordStore};
use ant_evm::QuotingMetrics;
use ant_protocol::{storage::ValidationType, NetworkAddress};
use libp2p::kad::{
    store::RecordStore, KBucketDistance as Distance, ProviderRecord, Record, RecordKey,
};
use std::{borrow::Cow, collections::HashMap};

pub enum UnifiedRecordStore {
    Client(ClientRecordStore),
    Node(NodeRecordStore),
}

impl RecordStore for UnifiedRecordStore {
    type RecordsIter<'a> = std::vec::IntoIter<Cow<'a, Record>>;
    type ProvidedIter<'a> = std::vec::IntoIter<Cow<'a, ProviderRecord>>;

    fn get(&self, k: &RecordKey) -> Option<std::borrow::Cow<'_, Record>> {
        match self {
            Self::Client(store) => store.get(k),
            Self::Node(store) => store.get(k),
        }
    }

    fn put(&mut self, r: Record) -> libp2p::kad::store::Result<()> {
        match self {
            Self::Client(store) => store.put(r),
            Self::Node(store) => store.put(r),
        }
    }

    fn remove(&mut self, k: &RecordKey) {
        match self {
            Self::Client(store) => store.remove(k),
            Self::Node(store) => store.remove(k),
        }
    }

    fn records(&self) -> Self::RecordsIter<'_> {
        match self {
            Self::Client(store) => store.records(),
            Self::Node(store) => store.records(),
        }
    }

    fn add_provider(&mut self, record: ProviderRecord) -> libp2p::kad::store::Result<()> {
        match self {
            Self::Client(store) => store.add_provider(record),
            Self::Node(store) => store.add_provider(record),
        }
    }

    fn providers(&self, key: &RecordKey) -> Vec<ProviderRecord> {
        match self {
            Self::Client(store) => store.providers(key),
            Self::Node(store) => store.providers(key),
        }
    }

    fn provided(&self) -> Self::ProvidedIter<'_> {
        match self {
            Self::Client(store) => store.provided(),
            Self::Node(store) => store.provided(),
        }
    }

    fn remove_provider(&mut self, k: &RecordKey, p: &libp2p::PeerId) {
        match self {
            Self::Client(store) => store.remove_provider(k, p),
            Self::Node(store) => store.remove_provider(k, p),
        }
    }
}

impl UnifiedRecordStore {
    pub(crate) fn contains(&self, key: &RecordKey) -> Result<bool> {
        match self {
            Self::Client(_) => {
                error!("Calling 'contains' at Client. This should not happen");
                Err(NetworkError::OperationNotAllowedOnClientRecordStore)
            }
            Self::Node(store) => Ok(store.contains(key)),
        }
    }

    pub(crate) fn record_addresses(&self) -> Result<HashMap<NetworkAddress, ValidationType>> {
        match self {
            Self::Client(_) => {
                error!("Calling record_addresses at Client. This should not happen");
                Err(NetworkError::OperationNotAllowedOnClientRecordStore)
            }
            Self::Node(store) => Ok(store.record_addresses()),
        }
    }

    pub(crate) fn record_addresses_ref(
        &self,
    ) -> Result<&HashMap<RecordKey, (NetworkAddress, ValidationType, DataTypes)>> {
        match self {
            Self::Client(_) => {
                error!("Calling record_addresses_ref at Client. This should not happen");
                Err(NetworkError::OperationNotAllowedOnClientRecordStore)
            }
            Self::Node(store) => Ok(store.record_addresses_ref()),
        }
    }

    pub(crate) fn put_verified(
        &mut self,
        r: Record,
        record_type: ValidationType,
    ) -> libp2p::kad::store::Result<()> {
        match self {
            Self::Client(_) => {
                error!("Calling put_verified at Client. This should not happen");
                Ok(())
            }
            Self::Node(store) => store.put_verified(r, record_type),
        }
    }

    /// Return the quoting metrics used to calculate the cost of storing a record
    /// and whether the record is already stored locally
    pub(crate) fn quoting_metrics(
        &self,
        key: &RecordKey,
        data_type: u32,
        data_size: usize,
        network_size: Option<u64>,
    ) -> Result<(QuotingMetrics, bool)> {
        match self {
            Self::Client(_) => {
                error!("Calling quoting_metrics at Client. This should not happen");
                Err(NetworkError::OperationNotAllowedOnClientRecordStore)
            }
            Self::Node(store) => Ok(store.quoting_metrics(key, data_type, data_size, network_size)),
        }
    }

    pub(crate) fn payment_received(&mut self) {
        match self {
            Self::Client(_) => {
                error!("Calling payment_received at Client. This should not happen");
            }
            Self::Node(store) => store.payment_received(),
        }
    }

    pub(crate) fn get_farthest_replication_distance(&self) -> Result<Option<Distance>> {
        match self {
            Self::Client(_) => {
                error!(
                    "Calling get_farthest_replication_distance at Client. This should not happen"
                );
                Err(NetworkError::OperationNotAllowedOnClientRecordStore)
            }
            Self::Node(store) => Ok(store.get_responsible_distance_range()),
        }
    }

    pub(crate) fn set_distance_range(&mut self, distance: Distance) {
        match self {
            Self::Client(_) => {
                error!("Calling set_distance_range at Client. This should not happen");
            }
            Self::Node(store) => store.set_responsible_distance_range(distance),
        }
    }

    pub(crate) fn get_farthest(&self) -> Result<Option<RecordKey>> {
        match self {
            Self::Client(_) => Err(NetworkError::OperationNotAllowedOnClientRecordStore),
            Self::Node(store) => Ok(store.get_farthest()),
        }
    }

    /// Mark the record as stored in the store.
    /// This adds it to records set, so it can now be retrieved
    /// (to be done after writes are finalised)
    pub(crate) fn mark_as_stored(
        &mut self,
        k: RecordKey,
        record_type: ValidationType,
        data_type: DataTypes,
    ) {
        match self {
            Self::Client(_) => {
                error!("Calling mark_as_stored at Client. This should not happen");
            }
            Self::Node(store) => store.mark_as_stored(k, record_type, data_type),
        };
    }

    pub(crate) fn cleanup_irrelevant_records(&mut self) {
        match self {
            Self::Client(_store) => {
                error!("Calling cleanup_irrelevant_records at Client. This should not happen");
            }
            Self::Node(store) => store.cleanup_irrelevant_records(),
        }
    }
}
