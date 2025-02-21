// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use libp2p::core::{transport::PortUse, Endpoint, Multiaddr};
use libp2p::swarm::{
    dummy, ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
    THandlerOutEvent, ToSwarm,
};
use libp2p::PeerId;
use std::{
    convert::Infallible,
    fmt,
    task::{Context, Poll},
    time::{Duration, Instant},
};
use sysinfo::{Pid, ProcessRefreshKind};
use tracing::trace;

pub struct Behaviour {
    max_allowed_cpu_percentage: f32,
    process_cpu_usage_percentage: f32,
    last_refreshed: Instant,
    system: sysinfo::System,
    pid: Pid,
}

const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

impl Behaviour {
    pub fn with_max_percentage(max_allowed_cpu_percentage: f32) -> Self {
        let pid = Pid::from_u32(std::process::id());
        let mut system = sysinfo::System::new();
        system.refresh_cpu();
        system.refresh_process(pid);

        Self {
            max_allowed_cpu_percentage,
            process_cpu_usage_percentage: 0.0,
            last_refreshed: Instant::now(),
            system,
            pid,
        }
    }

    fn check_limit(&mut self) -> Result<(), ConnectionDenied> {
        self.refresh_cpu_stats_if_needed();

        if self.process_cpu_usage_percentage > self.max_allowed_cpu_percentage {
            return Err(ConnectionDenied::new(CpuUsageLimitExceeded {
                process_cpu_usage_percentage: self.process_cpu_usage_percentage,
                max_allowed_cpu_percentage: self.max_allowed_cpu_percentage,
            }));
        }

        Ok(())
    }

    fn refresh_cpu_stats_if_needed(&mut self) {
        let now = Instant::now();

        if self.last_refreshed + REFRESH_INTERVAL > now {
            return;
        }

        self.system
            .refresh_process_specifics(self.pid, ProcessRefreshKind::new().with_cpu());

        let process_cpu_usage_percentage = match self.system.process(self.pid) {
            Some(process) => process.cpu_usage(),
            None => {
                trace!("Failed to retrieve process CPU stats inside connection limit behaviour");
                return;
            }
        };

        self.last_refreshed = now;
        self.process_cpu_usage_percentage = process_cpu_usage_percentage;
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = dummy::ConnectionHandler;
    type ToSwarm = Infallible;

    fn handle_pending_inbound_connection(
        &mut self,
        id: ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        self.check_limit().inspect_err(|_| {
            warn!("Connection limit exceeded, closing pending inbound connection: {id:?} from {remote_addr:?}, local addr: {local_addr:?}");
        })
    }

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        id: ConnectionId,
        maybe_peer: Option<PeerId>,
        addresses: &[Multiaddr],
        effective_role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        self.check_limit().inspect_err(|_| {
            warn!(
                ?maybe_peer,
                ?addresses,
                ?effective_role,
                "Connection limit exceeded, closing pending outbound connection: {id:?}"
            );
        })?;
        Ok(vec![])
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: Endpoint,
        _: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn on_swarm_event(&mut self, _: FromSwarm) {}

    fn on_connection_handler_event(
        &mut self,
        _id: PeerId,
        _: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        // TODO: remove when Rust 1.82 is MSRV
        #[allow(unreachable_patterns)]
        libp2p::core::util::unreachable(event)
    }

    fn poll(&mut self, _: &mut Context<'_>) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        Poll::Pending
    }
}

/// A connection limit has been exceeded.
#[derive(Debug, Clone, Copy)]
pub struct CpuUsageLimitExceeded {
    process_cpu_usage_percentage: f32,
    max_allowed_cpu_percentage: f32,
}

impl std::error::Error for CpuUsageLimitExceeded {}

impl fmt::Display for CpuUsageLimitExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "process cpu usage limit exceeded: process cpu usage percent: {}, max allowed percent: {}",
            self.process_cpu_usage_percentage,
            self.max_allowed_cpu_percentage,
        )
    }
}
