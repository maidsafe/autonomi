// Copyright 2023 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::record_store::{ClientRecordStore, NodeRecordStore};
use libp2p::kad::{
    store::{RecordStore, Result},
    KBucketDistance as Distance, ProviderRecord, Record, RecordKey,
};
use sn_protocol::{storage::RecordType, NetworkAddress};
use sn_transfers::NanoTokens;
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

    fn put(&mut self, r: Record) -> Result<()> {
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

    fn add_provider(&mut self, record: ProviderRecord) -> Result<()> {
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
    pub(crate) fn contains(&self, key: &RecordKey) -> Option<&RecordType> {
        match self {
            Self::Client(store) => store.contains(key),
            Self::Node(store) => store.contains(key),
        }
    }

    pub(crate) fn record_addresses(&self) -> HashMap<NetworkAddress, RecordType> {
        match self {
            Self::Client(store) => store.record_addresses(),
            Self::Node(store) => store.record_addresses(),
        }
    }

    #[allow(clippy::mutable_key_type)]
    pub(crate) fn record_addresses_ref(&self) -> &HashMap<RecordKey, RecordType> {
        match self {
            Self::Client(store) => store.record_addresses_ref(),
            Self::Node(store) => store.record_addresses_ref(),
        }
    }

    pub(crate) fn put_verified(&mut self, r: Record, record_type: RecordType) -> Result<()> {
        match self {
            Self::Client(store) => store.put_verified(r, record_type),
            Self::Node(store) => store.put_verified(r, record_type),
        }
    }

    pub(crate) fn store_cost(&self) -> NanoTokens {
        match self {
            Self::Client(_) => {
                warn!("Calling store cost calculation at Client. This should not happen");
                NanoTokens::zero()
            }
            Self::Node(store) => store.store_cost(),
        }
    }

    pub(crate) fn set_distance_range(&mut self, distance_range: Distance) {
        match self {
            Self::Client(store) => store.set_distance_range(distance_range),
            Self::Node(store) => store.set_distance_range(distance_range),
        }
    }
}
