//! Helpers for constructing `NodeServiceData` fixtures used in UI tests.

use ant_bootstrap::InitialPeersConfig;
use ant_evm::{EvmNetwork, RewardsAddress};
use ant_service_management::{NodeServiceData, ReachabilityProgress, ServiceStatus};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    env,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    str::FromStr,
};

static NODE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn allocate_pid(index: u64) -> u32 {
    1000 + index as u32
}

fn node_binary_name() -> String {
    format!("antnode{}", env::consts::EXE_SUFFIX)
}

fn base_path(unique_id: u64) -> PathBuf {
    env::temp_dir().join(format!("node_launchpad_test_{unique_id}"))
}

/// Create a deterministic `NodeServiceData` entry for tests.
///
/// Each invocation uses a unique temporary directory base to avoid clashes between tests that run
/// concurrently. Only the fields that are relevant for UI rendering are populated; the rest are
/// filled with sensible defaults that mirror what the real registry would provide.
pub fn make_node_service_data(index: u64, status: ServiceStatus) -> NodeServiceData {
    let service_name = format!("antnode-{}", index + 1);
    make_named_node_service_data(&service_name, index, status)
}

/// Same as [`make_node_service_data`] but lets callers override the service name while keeping
/// deterministic numbering for ports and identifiers.
pub fn make_named_node_service_data(
    service_name: &str,
    index: u64,
    status: ServiceStatus,
) -> NodeServiceData {
    let unique_id = NODE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let temp_dir = base_path(unique_id);

    let pid = if status == ServiceStatus::Running {
        Some(allocate_pid(index))
    } else {
        None
    };

    NodeServiceData {
        schema_version: 3,
        service_name: service_name.to_string(),
        version: "0.1.0".to_string(),
        status,
        antnode_path: temp_dir.join(node_binary_name()),
        data_dir_path: temp_dir.join("data"),
        log_dir_path: temp_dir.join("logs"),
        number: (index + 1) as u16,
        metrics_port: (25_000 + index) as u16,
        connected_peers: 5,
        alpha: false,
        auto_restart: false,
        evm_network: EvmNetwork::ArbitrumOne,
        initial_peers_config: InitialPeersConfig {
            first: false,
            local: false,
            addrs: vec![],
            network_contacts_url: vec![],
            ignore_cache: false,
            bootstrap_cache_dir: None,
        },
        listen_addr: None,
        log_format: None,
        max_archived_log_files: Some(10),
        max_log_files: Some(5),
        network_id: Some(1),
        node_ip: Some(Ipv4Addr::new(127, 0, 0, 1)),
        node_port: Some((15_000 + index) as u16),
        no_upnp: false,
        peer_id: None,
        pid,
        rewards_address: RewardsAddress::from_str("0x1234567890123456789012345678901234567890")
            .unwrap_or_default(),
        reachability_progress: ReachabilityProgress::NotRun,
        last_critical_failure: None,
        rpc_socket_addr: Some(SocketAddr::from(([127, 0, 0, 1], (35_000 + index) as u16))),
        skip_reachability_check: false,
        user: None,
        user_mode: false,
        write_older_cache_files: false,
    }
}
