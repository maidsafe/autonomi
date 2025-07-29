// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use prometheus_client::encoding::{EncodeLabelSet, EncodeLabelValue};

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
pub(crate) struct RelayClientEventLabels {
    event: EventType,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelValue)]
enum EventType {
    ReservationReqAccepted,
    OutboundCircuitEstablished,
    InboundCircuitEstablished,
}

impl From<&libp2p::relay::client::Event> for EventType {
    fn from(event: &libp2p::relay::client::Event) -> Self {
        match event {
            libp2p::relay::client::Event::ReservationReqAccepted { .. } => {
                EventType::ReservationReqAccepted
            }
            libp2p::relay::client::Event::OutboundCircuitEstablished { .. } => {
                EventType::OutboundCircuitEstablished
            }
            libp2p::relay::client::Event::InboundCircuitEstablished { .. } => {
                EventType::InboundCircuitEstablished
            }
        }
    }
}

impl super::Recorder<libp2p::relay::client::Event> for super::NetworkMetricsRecorder {
    fn record(&self, event: &libp2p::relay::client::Event) {
        let _ = self
            .relay_client_events
            .get_or_create(&RelayClientEventLabels {
                event: event.into(),
            })
            .inc();
    }
}
