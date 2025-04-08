// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use prometheus_client::encoding::{EncodeLabelSet, EncodeLabelValue};

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
pub(crate) struct UpnpEventLabels {
    event: EventType,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelValue)]
enum EventType {
    NewExternalAddr,
    ExpiredExternalAddr,
    GatewayNotFound,
    NonRoutableGateway,
}

impl From<&libp2p::upnp::Event> for EventType {
    fn from(event: &libp2p::upnp::Event) -> Self {
        match event {
            libp2p::upnp::Event::NewExternalAddr { .. } => EventType::NewExternalAddr,
            libp2p::upnp::Event::ExpiredExternalAddr { .. } => EventType::ExpiredExternalAddr,
            libp2p::upnp::Event::GatewayNotFound => EventType::GatewayNotFound,
            libp2p::upnp::Event::NonRoutableGateway => EventType::NonRoutableGateway,
        }
    }
}

impl super::Recorder<libp2p::upnp::Event> for super::NetworkMetricsRecorder {
    fn record(&self, event: &libp2p::upnp::Event) {
        self.upnp_events
            .get_or_create(&UpnpEventLabels {
                event: event.into(),
            })
            .inc();
    }
}
