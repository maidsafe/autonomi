// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::networking::driver::behaviour::upnp;
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

impl From<&upnp::behaviour::Event> for EventType {
    fn from(event: &upnp::behaviour::Event) -> Self {
        match event {
            upnp::behaviour::Event::NewExternalAddr { .. } => EventType::NewExternalAddr,
            upnp::behaviour::Event::ExpiredExternalAddr { .. } => EventType::ExpiredExternalAddr,
            upnp::behaviour::Event::GatewayNotFound => EventType::GatewayNotFound,
            upnp::behaviour::Event::NonRoutableGateway => EventType::NonRoutableGateway,
        }
    }
}

impl super::Recorder<upnp::behaviour::Event> for super::NetworkMetricsRecorder {
    fn record(&self, event: &upnp::behaviour::Event) {
        let _ = self
            .upnp_events
            .get_or_create(&UpnpEventLabels {
                event: event.into(),
            })
            .inc();
    }
}
