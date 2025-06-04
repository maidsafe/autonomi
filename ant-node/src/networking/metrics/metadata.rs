// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{networking::MetricsRegistries, ReachabilityStatus};
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

    /// Register the reachability status of the node in the metadata registry.
    /// Setting `status` to `None` indicates that the reachability check has not been performed.
    pub(crate) fn register_reachability_status(&mut self, status: Option<ReachabilityStatus>) {
        let mut upnp_result = false;
        let mut relay = false;
        let mut reachable = false;
        let mut not_routable = false;
        let mut not_performed = false;

        match status {
            Some(ReachabilityStatus::NotRoutable { upnp }) => {
                not_routable = true;
                upnp_result = upnp;
            }
            Some(ReachabilityStatus::Relay { upnp }) => {
                relay = true;
                upnp_result = upnp;
            }
            Some(ReachabilityStatus::Reachable { upnp, .. }) => {
                reachable = true;
                upnp_result = upnp;
            }
            None => {
                not_performed = true;
            }
        }

        self.metadata_sub_reg.register(
            "reachability_status",
            "The reachability status of the node",
            self.construct_reachability_info(
                upnp_result,
                relay,
                reachable,
                not_routable,
                not_performed,
                false,
            ),
        );
    }

    /// Register that a reachability check is ongoing in the metadata registry.
    /// This sets the other reachability status fields to `false`.
    pub(crate) fn register_reachability_check_is_ongoing(&mut self) {
        self.metadata_sub_reg.register(
            "is_ongoing_reachability_check",
            "Indicates if a reachability check is currently ongoing",
            self.construct_reachability_info(false, false, false, false, false, true),
        );
    }

    fn construct_reachability_info(
        &self,
        upnp: bool,
        relay: bool,
        reachable: bool,
        not_routable: bool,
        not_performed: bool,
        is_ongoing: bool,
    ) -> Info<[(String, String); 6]> {
        Info::new([
            ("not_routable".to_string(), not_routable.to_string()),
            ("relay".to_string(), relay.to_string()),
            ("reachable".to_string(), reachable.to_string()),
            ("check_not_performed".to_string(), not_performed.to_string()),
            ("upnp".to_string(), upnp.to_string()),
            ("is_ongoing".to_string(), is_ongoing.to_string()),
        ])
    }
}
