// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#[cfg(feature = "open-metrics")]
use crate::networking::MetricsRegistries;
use libp2p::{
    PeerId, Transport as _,
    core::{muxing::StreamMuxerBox, transport},
    identity::Keypair,
};

const MAX_STREAM_DATA_ENV_STR: &str = "ANT_MAX_STREAM_DATA";

pub(crate) fn build_transport(
    keypair: &Keypair,
    #[cfg(feature = "open-metrics")] registries: &mut MetricsRegistries,
) -> transport::Boxed<(PeerId, StreamMuxerBox)> {
    let trans = generate_quic_transport(keypair);
    #[cfg(feature = "open-metrics")]
    let trans = libp2p::metrics::BandwidthTransport::new(trans, &mut registries.standard_metrics);

    let trans = trans.map(|(peer_id, muxer), _| (peer_id, StreamMuxerBox::new(muxer)));

    trans.boxed()
}

fn generate_quic_transport(
    keypair: &Keypair,
) -> libp2p::quic::GenTransport<libp2p::quic::tokio::Provider> {
    let mut quic_config = libp2p::quic::Config::new(keypair);
    if let Ok(val) = std::env::var(MAX_STREAM_DATA_ENV_STR) {
        match val.parse::<u32>() {
            Ok(val) => {
                quic_config.max_stream_data = val;
                tracing::info!("Overriding QUIC connection receive window value to {val}");
            }
            Err(e) => {
                tracing::warn!(
                    "QUIC connection receive window value override failed. Could not parse `{MAX_STREAM_DATA_ENV_STR}={val}` as integer: {e}"
                )
            }
        }
    }

    libp2p::quic::tokio::Transport::new(quic_config)
}
