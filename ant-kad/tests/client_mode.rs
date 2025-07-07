use libp2p_identify as identify;
use libp2p_identity as identity;
use libp2p_core::{transport::MemoryTransport, upgrade, Transport};
use libp2p_tcp as tcp;
use libp2p_noise as noise;
use libp2p_yamux as yamux;
use libp2p_swarm::{self as swarm, Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt;
use ant_kad::{store::MemoryStore, Behaviour, Config, Mode};
use ant_kad::Event::*;
use serial_test::serial;
use tracing_subscriber::EnvFilter;
use MyBehaviourEvent::*;

fn create_swarm() -> Swarm<MyBehaviour> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_id = local_key.public().to_peer_id();
    
    // Create a transport that supports both TCP and memory
    let tcp_transport = tcp::tokio::Transport::default();
    let memory_transport = MemoryTransport::default();
    let transport = tcp_transport
        .or_transport(memory_transport)
        .upgrade(upgrade::Version::V1)
        .authenticate(noise::Config::new(&local_key).unwrap())
        .multiplex(yamux::Config::default())
        .boxed();

    let behaviour = MyBehaviour::new(local_key);
    Swarm::new(
        transport,
        behaviour,
        local_id,
        swarm::Config::without_executor(),
    )
}

#[tokio::test]
#[serial]
async fn server_gets_added_to_routing_table_by_client() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let mut client = create_swarm();
    let mut server = create_swarm();

    server.listen().with_memory_addr_external().await;
    client.connect(&mut server).await;

    let server_peer_id = *server.local_peer_id();
    tokio::spawn(server.loop_on_next());

    let external_event_peer = client
        .wait(|e| match e {
            SwarmEvent::NewExternalAddrOfPeer { peer_id, .. } => Some(peer_id),
            _ => None,
        })
        .await;
    let routing_updated_peer = client
        .wait(|e| match e {
            SwarmEvent::Behaviour(Kad(RoutingUpdated { peer, .. })) => Some(peer),
            _ => None,
        })
        .await;

    assert_eq!(external_event_peer, server_peer_id);
    assert_eq!(routing_updated_peer, server_peer_id);
}

#[tokio::test]
#[serial]
async fn two_servers_add_each_other_to_routing_table() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let mut server1 = create_swarm();
    let mut server2 = create_swarm();

    server2.listen().with_memory_addr_external().await;
    server1.connect(&mut server2).await;

    let server1_peer_id = *server1.local_peer_id();
    let server2_peer_id = *server2.local_peer_id();

    match libp2p_swarm_test::drive(&mut server1, &mut server2).await {
        (
            [Identify(_), Identify(_), Kad(RoutingUpdated { peer: peer1, .. })]
            | [Identify(_), Kad(RoutingUpdated { peer: peer1, .. }), Identify(_)],
            [Identify(_), Identify(_)],
        ) => {
            assert_eq!(peer1, server2_peer_id);
        }
        other => panic!("Unexpected events: {other:?}"),
    }

    server1.listen().with_memory_addr_external().await;
    server2.connect(&mut server1).await;

    tokio::spawn(server1.loop_on_next());

    let peer = server2
        .wait(|e| match e {
            SwarmEvent::Behaviour(Kad(RoutingUpdated { peer, .. })) => Some(peer),
            _ => None,
        })
        .await;

    assert_eq!(peer, server1_peer_id);
}

#[tokio::test]
#[serial]
async fn adding_an_external_addresses_activates_server_mode_on_existing_connections() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let mut client = create_swarm();
    let mut server = create_swarm();
    let server_peer_id = *server.local_peer_id();

    let (memory_addr, _) = server.listen().await;

    client.dial(memory_addr.clone()).unwrap();

    // Do the usual identify send/receive dance.
    match libp2p_swarm_test::drive(&mut client, &mut server).await {
        ([Identify(_), Identify(_)], [Identify(_), Identify(_)]) => {}
        other => panic!("Unexpected events: {other:?}"),
    }

    // Server learns its external address (this could be through AutoNAT or some other mechanism).
    server.add_external_address(memory_addr);

    // The server reconfigured its connection to the client to be in server mode,
    // pushes that information to client which as a result updates its routing
    // table and triggers a mode change to Mode::Server.
    match libp2p_swarm_test::drive(&mut client, &mut server).await {
        (
            [Identify(identify::Event::Received { .. }), Kad(RoutingUpdated { peer: peer1, .. })],
            [Kad(ModeChanged { new_mode }), Identify(identify::Event::Pushed { .. })],
        ) => {
            assert_eq!(new_mode, Mode::Server);
            assert_eq!(peer1, server_peer_id);
        }
        other => panic!("Unexpected events: {other:?}"),
    }
}

#[tokio::test]
#[serial]
async fn set_client_to_server_mode() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let mut client = create_swarm();
    client.behaviour_mut().kad.set_mode(Some(Mode::Client));

    let mut server = create_swarm();

    server.listen().with_memory_addr_external().await;
    client.connect(&mut server).await;

    let server_peer_id = *server.local_peer_id();

    let peer_id = client
        .wait(|e| match e {
            SwarmEvent::NewExternalAddrOfPeer { peer_id, .. } => Some(peer_id),
            _ => None,
        })
        .await;
    let client_event = client.wait(|e| match e {
        SwarmEvent::Behaviour(Kad(RoutingUpdated { peer, .. })) => Some(peer),
        _ => None,
    });
    let server_event = server.wait(|e| match e {
        SwarmEvent::Behaviour(Identify(identify::Event::Received { info, .. })) => Some(info),
        _ => None,
    });

    let (peer, info) = futures::future::join(client_event, server_event).await;

    assert_eq!(peer, server_peer_id);
    assert_eq!(peer_id, server_peer_id);
    assert!(info
        .protocols
        .iter()
        .all(|proto| ant_kad::PROTOCOL_NAME.ne(proto)));

    client.behaviour_mut().kad.set_mode(Some(Mode::Server));

    tokio::spawn(client.loop_on_next());

    let info = server
        .wait(|e| match e {
            SwarmEvent::Behaviour(Identify(identify::Event::Received { info, .. })) => Some(info),
            _ => None,
        })
        .await;

    assert!(info
        .protocols
        .iter()
        .any(|proto| ant_kad::PROTOCOL_NAME.eq(proto)));
}

#[derive(libp2p_swarm::NetworkBehaviour)]
#[behaviour(prelude = "libp2p_swarm::derive_prelude")]
struct MyBehaviour {
    identify: identify::Behaviour,
    kad: Behaviour<MemoryStore>,
}

impl MyBehaviour {
    fn new(k: identity::Keypair) -> Self {
        let local_peer_id = k.public().to_peer_id();

        Self {
            identify: identify::Behaviour::new(identify::Config::new(
                "/test/1.0.0".to_owned(),
                k.public(),
            )),
            kad: Behaviour::with_config(
                local_peer_id,
                MemoryStore::new(local_peer_id),
                Config::new(ant_kad::PROTOCOL_NAME),
            ),
        }
    }
}
