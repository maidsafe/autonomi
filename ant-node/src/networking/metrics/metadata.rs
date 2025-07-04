// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::networking::MetricsRegistries;
use libp2p::PeerId;
use prometheus_client::{metrics::info::Info, registry::Registry};

pub(crate) struct MetadataRecorder<'a> {
    pub(crate) metadata_sub_reg: &'a mut Registry,
}

impl<'a> MetadataRecorder<'a> {
    /// Create a new `MetadataRecorder` with a reference to the metadata sub-registry.
    pub(crate) fn new(registries: &'a mut MetricsRegistries) -> Self {
        let metadata_sub_reg = registries
            .metadata
            .sub_registry_with_prefix("ant_networking");
        MetadataRecorder { metadata_sub_reg }
    }

    /// Register peer ID in the metadata registry.
    pub(crate) fn register_peer_id(&mut self, peer_id: &PeerId) {
        self.metadata_sub_reg.register(
            "peer_id",
            "Identifier of a peer of the network",
            Info::new(vec![("peer_id".to_string(), peer_id.to_string())]),
        );
    }

    /// Register the identify protocol version string in the metadata registry.
    pub(crate) fn register_identify_protocol_string(&mut self, identify_protocol_str: String) {
        self.metadata_sub_reg.register(
            "identify_protocol_str",
            "The protocol version string that is used to connect to the correct network",
            Info::new(vec![(
                "identify_protocol_str".to_string(),
                identify_protocol_str,
            )]),
        );
    }
}
