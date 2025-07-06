// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use clap::Parser;
use color_eyre::eyre::Result;
use futures::StreamExt;
use libp2p::autonat::NatStatus;
use libp2p::core::{multiaddr::Protocol, Multiaddr};
use libp2p::swarm::SwarmEvent;
use libp2p::{noise, tcp, yamux};
use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::time::Duration;
use tracing::{debug, info, warn};
use tracing_log::AsTrace;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use behaviour::{Behaviour, BehaviourEvent};

mod behaviour;

const CONFIDENCE_MAX: usize = 2;
const RETRY_INTERVAL: Duration = Duration::from_secs(10);

/// A tool to detect NAT status of the machine. It can be run in server mode or client mode.
/// The program returns with the following exit codes based on the network status:
/// - 10: Public NAT
/// - 11: Public under UPnP
/// - 12: Private or Unknown NAT
#[derive(Debug, Parser)]
#[clap(disable_version_flag = true)]
struct Opt {
    /// Port to listen on.
    ///
    /// `0` causes the OS to assign a random available port.
    #[clap(long, short, default_value_t = 0)]
    port: u16,

    /// Servers to send dial-back requests to as a client, in a 'multiaddr' format.
    ///
    /// If no servers are provided, the program will run in server mode.
    ///
    /// A multiaddr looks like `/ip4/1.2.3.4/tcp/1200/tcp` where `1.2.3.4` is the IP and `1200` is the port.
    /// Alternatively, the address can be written as `1.2.3.4:1200`.
    ///
    /// This argument can be provided multiple times to connect to multiple peers.
    #[clap(name = "SERVER", value_name = "multiaddr", value_delimiter = ',', value_parser = parse_peer_addr)]
    server_addr: Vec<Multiaddr>,

    /// Disable use of UPnP to open a port on the router, before detecting NAT status.
    #[clap(long, short, default_value_t = false)]
    no_upnp: bool,

    #[command(flatten)]
    verbose: clap_verbosity_flag::Verbosity,

    /// Print the crate version
    #[clap(long)]
    crate_version: bool,

    /// Print the package version
    #[clap(long)]
    #[cfg(not(feature = "nightly"))]
    package_version: bool,

    /// Print version information.
    #[clap(long)]
    version: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let opt = Opt::parse();

    if opt.version {
        println!(
            "{}",
            ant_build_info::version_string(
                "Autonomi NAT Detection",
                env!("CARGO_PKG_VERSION"),
                None
            )
        );
        return Ok(());
    }

    if opt.crate_version {
        println!("Crate version: {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    #[cfg(not(feature = "nightly"))]
    if opt.package_version {
        println!("Package version: {}", ant_build_info::package_version());
        return Ok(());
    }

    let registry = tracing_subscriber::registry().with(tracing_subscriber::fmt::layer());
    // Use `RUST_LOG` if set, else use the verbosity flag (where `-vvvv` is trace level).
    let _ = if std::env::var_os("RUST_LOG").is_some() {
        registry.with(EnvFilter::from_env("RUST_LOG")).try_init()
    } else {
        let filter = tracing_subscriber::filter::Targets::new().with_target(
            env!("CARGO_BIN_NAME").replace('-', "_"),
            opt.verbose.log_level_filter().as_trace(),
        );
        registry.with(filter).try_init()
    };

    let mut builder = AppBuilder::new()
        .servers(opt.server_addr)
        .upnp(false)
        .port(opt.port);

    // Run the program twice, to first detect NAT status without UPnP,
    // and then with UPnP enabled. (Unless `--no-upnp` was given.)
    let mut running_with_upnp = false;
    loop {
        let status = builder
            .build()?
            // The main loop will exit once it has gained enough confidence in the NAT status.
            .run()
            .await;

        match status {
            NatStatus::Public(addr) => {
                info!(%addr, "NAT is public{}", if running_with_upnp { " (with UPnP)" } else { "" });
                if running_with_upnp {
                    // The error codes are used by other programs, caution when changing them.
                    std::process::exit(11);
                } else {
                    std::process::exit(10);
                }
            }
            NatStatus::Private => {
                // Unless `--no-upnp` is set, rerun the program with UPnP enabled.
                if !opt.no_upnp && !running_with_upnp {
                    warn!("NAT is private, rerunning program with UPnP enabled in 2 seconds...");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    builder = builder.upnp(true);
                    running_with_upnp = true;
                } else {
                    info!("NAT is private");
                    std::process::exit(12);
                }
            }
            NatStatus::Unknown => {
                info!("NAT status is unknown");
                std::process::exit(12);
            }
        }
    }
}

enum State {
    // This is where we start dialing the servers.
    Init(Vec<Multiaddr>),
    // When we're dialing, we'll move on once we've connected to a server.
    Dialing,
    // We start probing until we have enough confidence (`usize`).
    // Keep track of confidence to report on changes.
    Probing(usize),
    // With enough confidence reached, we should report back the status.
    Done(NatStatus),
}

struct App {
    swarm: libp2p::Swarm<Behaviour>,
    // Interval with which to check the state of the program. (State is also checked on events.)
    interval: tokio::time::Interval,
    // When we are a client, we progress through different states.
    client_state: Option<State>,
    // Keep track of candidate addresses to avoid logging duplicates.
    candidate_addrs: HashSet<Multiaddr>,
}

impl App {
    fn new(swarm: libp2p::Swarm<Behaviour>, servers: Vec<Multiaddr>) -> Self {
        Self {
            swarm,
            interval: tokio::time::interval(Duration::from_secs(5)),
            client_state: if servers.is_empty() {
                None
            } else {
                Some(State::Init(servers))
            },
            candidate_addrs: HashSet::new(),
        }
    }

    /// Run the event loop until we have gained enough confidence in the NAT status.
    async fn run(mut self) -> NatStatus {
        loop {
            // Process both events and check the state per the interval.
            tokio::select! {
                event = self.swarm.select_next_some() => self.on_event(event),
                _ = self.interval.tick() => self.check_state(),
            }

            // If we reached `Done` status, return the status.
            if let Some(State::Done(status)) = self.client_state {
                break status;
            }
        }
    }

    // Called regularly to check the state of the program.
    fn check_state(&mut self) {
        let state = if let Some(state) = self.client_state.take() {
            state
        } else {
            return;
        };

        match state {
            State::Init(servers) => {
                self.client_state = Some(State::Dialing);

                for addr in servers {
                    // `SwarmEvent::Dialing` is only triggered when peer ID is included, so
                    // we log here too to make sure we log that we're dialing a server.
                    if let Err(e) = self.swarm.dial(addr.clone()) {
                        warn!(%addr, ?e, "Failed to dial server");
                    } else {
                        info!(%addr, "Dialing server");
                    }
                }
            }
            State::Dialing => {
                let info = self.swarm.network_info();
                if info.num_peers() > 0 {
                    self.client_state = Some(State::Probing(0));
                } else {
                    self.client_state = Some(State::Dialing);
                }
            }
            State::Probing(old_confidence) => {
                let confidence = self.swarm.behaviour().autonat.confidence();
                let status = self.swarm.behaviour().autonat.nat_status();

                if confidence >= CONFIDENCE_MAX {
                    debug!(confidence, ?status, "probing complete");
                    self.client_state = Some(State::Done(status));
                } else {
                    if confidence != old_confidence {
                        info!(
                            ?status,
                            %confidence,
                            "Confidence in NAT status {}",
                            if confidence > old_confidence {
                                "increased"
                            } else {
                                "decreased"
                            }
                        );
                    }
                    self.client_state = Some(State::Probing(confidence));
                }
            }
            State::Done(status) => {
                // Nothing more to do
                self.client_state = Some(State::Done(status));
            }
        }
    }

    fn on_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        match event {
            // We delegate the specific behaviour events to their respective methods.
            SwarmEvent::Behaviour(event) => match event {
                BehaviourEvent::Identify(event) => self.on_event_identify(event),
                BehaviourEvent::Autonat(event) => self.on_event_autonat(event),
                BehaviourEvent::Upnp(event) => self.on_event_upnp(event),
            },
            SwarmEvent::NewListenAddr { address, .. } => {
                debug!(%address, "Listening on new address");
            }
            SwarmEvent::NewExternalAddrCandidate { address } => {
                // Only report on newly discovered addresses.
                if self.candidate_addrs.insert(address.clone()) {
                    info!(%address, "New external address candidate");
                }
            }
            SwarmEvent::ExternalAddrConfirmed { address } => {
                info!(%address, "External address confirmed");
                self.check_state();
            }
            SwarmEvent::ExternalAddrExpired { address } => {
                warn!(%address, "External address expired")
            }
            SwarmEvent::ConnectionEstablished {
                peer_id,
                num_established,
                connection_id,
                ..
            } => {
                debug!(
                    conn_id=%connection_id,
                    %peer_id,
                    count=num_established,
                    "Connected to peer{}",
                    if num_established.get() > 1 {
                        " (again)"
                    } else {
                        ""
                    }
                );
                self.check_state();
            }
            SwarmEvent::ConnectionClosed {
                peer_id,
                num_established,
                connection_id,
                cause,
                ..
            } => {
                debug!(conn_id=%connection_id, %peer_id, count=num_established, ?cause, "Closed connection to peer");
            }
            SwarmEvent::IncomingConnection {
                local_addr,
                send_back_addr,
                connection_id,
                ..
            } => {
                debug!(conn_id=%connection_id, %local_addr, %send_back_addr, "Incoming connection");
            }
            SwarmEvent::IncomingConnectionError {
                connection_id,
                local_addr,
                send_back_addr,
                error,
                ..
            } => {
                warn!(conn_id=%connection_id, %local_addr, %send_back_addr, ?error, "Incoming connection error");
            }
            SwarmEvent::OutgoingConnectionError {
                peer_id,
                connection_id,
                error,
                ..
            } => {
                warn!(conn_id=%connection_id, ?peer_id, ?error, "Connection error");
            }
            SwarmEvent::ExpiredListenAddr { .. } => { /* ignore */ }
            SwarmEvent::ListenerClosed { .. } => { /* ignore */ }
            SwarmEvent::ListenerError { .. } => { /* ignore */ }
            SwarmEvent::Dialing {
                peer_id,
                connection_id,
            } => {
                info!(?peer_id, conn_id=%connection_id, "Dialing peer");
            }
            SwarmEvent::NewExternalAddrOfPeer { .. } => { /* ignore */ }
            event => warn!(?event, "Unknown SwarmEvent"),
        }
    }
}

/// Parse strings like `1.2.3.4:1234` and `/ip4/1.2.3.4/tcp/1234` into a multiaddr.
fn parse_peer_addr(addr: &str) -> Result<Multiaddr, &'static str> {
    // Parse valid IPv4 socket address, e.g. `1.2.3.4:1234`.
    if let Ok(addr) = addr.parse::<std::net::SocketAddrV4>() {
        let multiaddr = Multiaddr::from(*addr.ip()).with(Protocol::Tcp(addr.port()));

        return Ok(multiaddr);
    }

    // Parse any valid multiaddr string
    if let Ok(addr) = addr.parse::<Multiaddr>() {
        return Ok(addr);
    }

    Err("Could not parse address")
}

struct AppBuilder {
    port: u16,
    servers: Vec<Multiaddr>,
    upnp: bool,
}

impl AppBuilder {
    fn new() -> Self {
        Self {
            port: 0,
            upnp: false,
            servers: vec![],
        }
    }

    fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    fn upnp(mut self, upnp: bool) -> Self {
        self.upnp = upnp;
        self
    }

    fn servers(mut self, servers: Vec<Multiaddr>) -> Self {
        self.servers = servers;
        self
    }

    fn build(&self) -> Result<App> {
        // If no servers are provided, we are in server mode. Conversely, with servers
        // provided, we are in client mode.
        let client_mode = !self.servers.is_empty();

        let mut swarm = libp2p::SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )?
            .with_behaviour(|key| Behaviour::new(key.public(), client_mode, self.upnp))?
            // Make it so that we retry just before idling out, to prevent quickly disconnecting/connecting
            // to the same server.
            .with_swarm_config(|c| {
                c.with_idle_connection_timeout(RETRY_INTERVAL + Duration::from_secs(2))
            })
            .build();

        swarm.listen_on(
            Multiaddr::empty()
                .with(Protocol::Ip4(Ipv4Addr::UNSPECIFIED))
                .with(Protocol::Tcp(self.port)),
        )?;

        info!(
            peer_id=%swarm.local_peer_id(),
            "Starting in {} mode",
            if client_mode { "client" } else { "server" }
        );

        let app = App::new(swarm, self.servers.clone());

        Ok(app)
    }
}
