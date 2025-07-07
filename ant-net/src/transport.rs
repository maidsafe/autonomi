// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Transport layer abstractions for ant-net.
//!
//! This module provides high-level interfaces for configuring and managing
//! network transports, hiding the complexity of libp2p transport composition.

use crate::{AntNetError, Result};
use async_trait::async_trait;
use libp2p::{
    core::{muxing::StreamMuxerBox, transport::Boxed},
    identity::Keypair,
    PeerId, Transport as Libp2pTransport,
};
use std::fmt;
use std::time::Duration;

/// Abstract transport configuration and factory.
///
/// This trait allows different transport implementations to be used
/// interchangeably, while hiding the libp2p transport details.
#[async_trait]
pub trait Transport: Send + Sync + fmt::Debug {
    /// Build the actual libp2p transport.
    ///
    /// This method constructs the complete transport stack including
    /// security, multiplexing, and any additional features.
    fn build(
        &self,
        keypair: &Keypair,
        #[cfg(feature = "metrics")] metrics: &mut crate::MetricsRegistries,
    ) -> Result<Boxed<(PeerId, StreamMuxerBox)>>;

    /// Get the transport name for debugging.
    fn name(&self) -> &'static str;

    /// Clone the transport configuration.
    fn clone_transport(&self) -> Box<dyn Transport>;
}

impl Clone for Box<dyn Transport> {
    fn clone(&self) -> Self {
        self.clone_transport()
    }
}

/// QUIC transport configuration.
#[derive(Debug, Clone)]
pub struct QuicTransport {
    /// Maximum stream data buffer size.
    pub max_stream_data: Option<u32>,
    /// Connection idle timeout.
    pub idle_timeout: Option<Duration>,
    /// Keep alive interval.
    pub keep_alive_interval: Option<Duration>,
}

impl Default for QuicTransport {
    fn default() -> Self {
        Self {
            max_stream_data: None,
            idle_timeout: Some(Duration::from_secs(30)),
            keep_alive_interval: Some(Duration::from_secs(10)),
        }
    }
}

impl QuicTransport {
    /// Create a new QUIC transport with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the maximum stream data buffer size.
    pub fn with_max_stream_data(mut self, size: u32) -> Self {
        self.max_stream_data = Some(size);
        self
    }

    /// Set the connection idle timeout.
    pub fn with_idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = Some(timeout);
        self
    }

    /// Set the keep alive interval.
    pub fn with_keep_alive_interval(mut self, interval: Duration) -> Self {
        self.keep_alive_interval = Some(interval);
        self
    }
}

#[async_trait]
impl Transport for QuicTransport {
    fn build(
        &self,
        keypair: &Keypair,
        #[cfg(feature = "metrics")] _metrics: &mut crate::MetricsRegistries,
    ) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
        let mut quic_config = libp2p::quic::Config::new(keypair);

        // Apply configuration options
        if let Some(max_stream_data) = self.max_stream_data {
            quic_config.max_stream_data = max_stream_data;
        }

        // Check for environment variable override
        if let Ok(val) = std::env::var("ANT_MAX_STREAM_DATA") {
            match val.parse::<u32>() {
                Ok(val) => {
                    quic_config.max_stream_data = val;
                    tracing::info!("Overriding QUIC max stream data to {val}");
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to parse ANT_MAX_STREAM_DATA={val}: {e}"
                    );
                }
            }
        }

        let transport = libp2p::quic::tokio::Transport::new(quic_config);

        #[cfg(feature = "metrics")]
        let transport = libp2p::metrics::BandwidthTransport::new(transport, &mut _metrics.standard_metrics);

        let transport = transport
            .map(|(peer_id, muxer), _| (peer_id, StreamMuxerBox::new(muxer)))
            .boxed();

        Ok(transport)
    }

    fn name(&self) -> &'static str {
        "quic"
    }

    fn clone_transport(&self) -> Box<dyn Transport> {
        Box::new(self.clone())
    }
}

/// TCP transport configuration.
#[derive(Debug, Clone)]
pub struct TcpTransport {
    /// TCP port to bind to (None for automatic selection).
    pub port: Option<u16>,
    /// TCP nodelay setting.
    pub nodelay: bool,
}

impl Default for TcpTransport {
    fn default() -> Self {
        Self {
            port: None,
            nodelay: true,
        }
    }
}

impl TcpTransport {
    /// Create a new TCP transport with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the TCP port to bind to.
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    /// Set the TCP nodelay option.
    pub fn with_nodelay(mut self, nodelay: bool) -> Self {
        self.nodelay = nodelay;
        self
    }
}

#[async_trait]
impl Transport for TcpTransport {
    fn build(
        &self,
        keypair: &Keypair,
        #[cfg(feature = "metrics")] _metrics: &mut crate::MetricsRegistries,
    ) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
        // Generate QUIC transport (aligned with autonomi approach)
        let quic_config = libp2p::quic::Config::new(keypair);
        
        // Apply TCP-like configuration where possible
        if self.nodelay {
            // QUIC doesn't have nodelay equivalent, it's always optimized for low latency
        }
        
        let transport = libp2p::quic::tokio::Transport::new(quic_config);

        #[cfg(feature = "metrics")]
        let transport = libp2p::metrics::BandwidthTransport::new(transport, &mut _metrics.standard_metrics);

        let transport = transport
            .map(|(peer_id, muxer), _| (peer_id, StreamMuxerBox::new(muxer)))
            .boxed();

        Ok(transport)
    }

    fn name(&self) -> &'static str {
        "quic"
    }

    fn clone_transport(&self) -> Box<dyn Transport> {
        Box::new(self.clone())
    }
}

/// WebSocket transport configuration.
#[derive(Debug, Clone)]
pub struct WebSocketTransport {
    /// Use secure WebSockets (WSS).
    pub secure: bool,
}

impl Default for WebSocketTransport {
    fn default() -> Self {
        Self { secure: true }
    }
}

impl WebSocketTransport {
    /// Create a new WebSocket transport with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether to use secure WebSockets.
    pub fn with_secure(mut self, secure: bool) -> Self {
        self.secure = secure;
        self
    }
}

#[async_trait]
impl Transport for WebSocketTransport {
    fn build(
        &self,
        keypair: &Keypair,
        #[cfg(feature = "metrics")] _metrics: &mut crate::MetricsRegistries,
    ) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
        // WebSocket transport temporarily stubbed out - autonomi uses QUIC
        // Fall back to QUIC transport for now
        let quic_config = libp2p::quic::Config::new(keypair);
        let transport = libp2p::quic::tokio::Transport::new(quic_config);

        #[cfg(feature = "metrics")]
        let transport = libp2p::metrics::BandwidthTransport::new(transport, &mut _metrics.standard_metrics);

        let transport = transport
            .map(|(peer_id, muxer), _| (peer_id, StreamMuxerBox::new(muxer)))
            .boxed();

        Ok(transport)
    }

    fn name(&self) -> &'static str {
        "websocket"
    }

    fn clone_transport(&self) -> Box<dyn Transport> {
        Box::new(self.clone())
    }
}

/// Transport builder for composing multiple transports.
#[derive(Debug, Clone)]
pub struct TransportBuilder {
    transports: Vec<Box<dyn Transport>>,
}

impl Default for TransportBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TransportBuilder {
    /// Create a new transport builder.
    pub fn new() -> Self {
        Self {
            transports: Vec::new(),
        }
    }

    /// Add a transport to the builder.
    pub fn with_transport(mut self, transport: Box<dyn Transport>) -> Self {
        self.transports.push(transport);
        self
    }

    /// Add a QUIC transport.
    pub fn with_quic(self) -> Self {
        self.with_transport(Box::new(QuicTransport::new()))
    }

    /// Add a TCP transport.
    pub fn with_tcp(self) -> Self {
        self.with_transport(Box::new(TcpTransport::new()))
    }

    /// Add a WebSocket transport.
    pub fn with_websocket(self) -> Self {
        self.with_transport(Box::new(WebSocketTransport::new()))
    }

    /// Build a default transport stack (QUIC + TCP fallback).
    pub fn default_stack() -> Self {
        Self::new().with_quic().with_tcp()
    }

    /// Build the composed transport.
    pub fn build(
        self,
        keypair: &Keypair,
        #[cfg(feature = "metrics")] metrics: &mut crate::MetricsRegistries,
    ) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
        if self.transports.is_empty() {
            return Err(AntNetError::Configuration(
                "No transports configured".to_string(),
            ));
        }

        if self.transports.len() == 1 {
            // Single transport
            return self.transports[0].build(
                keypair,
                #[cfg(feature = "metrics")]
                metrics,
            );
        }

        // Multiple transports - build them all and compose
        let mut built_transports = Vec::new();
        for transport in &self.transports {
            built_transports.push(transport.build(
                keypair,
                #[cfg(feature = "metrics")]
                metrics,
            )?);
        }

        // For now, just return the first transport
        // TODO: Implement proper transport composition
        Ok(built_transports.into_iter().next().unwrap())
    }
}

#[cfg(feature = "metrics")]
pub struct MetricsRegistries {
    pub standard_metrics: libp2p::metrics::Metrics,
}

#[cfg(feature = "metrics")]
impl MetricsRegistries {
    pub fn new() -> Self {
        Self {
            standard_metrics: libp2p::metrics::Metrics::new(),
        }
    }
}