// Copyright (C) 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_bootstrap::InitialPeersConfig;
use ant_evm::{CustomNetwork, EvmNetwork, RewardsAddress};
use ant_service_management::{
    NodeRegistryManager, ServiceStatus,
    error::Result as ServiceControlResult,
    fs::{FileSystemActions, NodeInfo},
    metric::{
        MetricsAction, MetricsActionError, NodeMetadataExtended, NodeMetrics,
        ReachabilityStatusValues,
    },
    node::{NODE_SERVICE_DATA_SCHEMA_LATEST, NodeService, NodeServiceData},
};
use async_trait::async_trait;
use color_eyre::eyre::Result;
use libp2p::Multiaddr;
use libp2p_identity::PeerId;
use mockall::{mock, predicate::*};
use service_manager::ServiceInstallCtx;
use std::{
    path::{Path, PathBuf},
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};
use tokio::sync::RwLock;

// Helper function to get consistent test PeerIDs
pub fn get_test_peer_id(index: usize) -> PeerId {
    let hardcoded_peer_ids = [
        "12D3KooWS2tpXGGTmg2AHFiDh57yPQnat49YHnyqoggzXZWpqkCR",
        "12D3KooWRBhwfeGyVBSZZxT3ZfqNMDt6rrHkPFSBNJJ1tF8QWQG5",
        "12D3KooWHFGAWKDEsHi6L67N4KJJjG5UrQzWjgF4gJr3noPwxBhz",
        "12D3KooWPLxNqZV7xN5FJnJBjR7G8QwGBRFBNGz8xGqY4DcLnK8M",
        "12D3KooWQtA5VdYQ3RJfNzCKHkFGTPqjLN4KJJjG5UrQzWjgF4gK",
        "12D3KooWRmZVd7NJJyHFGAWKDEsHi6L67N4KJJjG5UrQzWjgF4h8",
        "12D3KooWSnBvNqZV7xN5FJnJBjR7G8QwGBRFBNGz8xGqY4DcLnP9",
        "12D3KooWTbA5VdYQ3RJfNzCKHkFGTPqjLN4KJJjG5UrQzWjgF4jL",
        "12D3KooWUcZVd7NJJyHFGAWKDEsHi6L67N4KJJjG5UrQzWjgF4k2",
        "12D3KooWVdBvNqZV7xN5FJnJBjR7G8QwGBRFBNGz8xGqY4DcLnQ3",
    ];

    let peer_id = hardcoded_peer_ids
        .get(index)
        .expect("Index out of bounds. Please add more test peer ids");

    PeerId::from_str(peer_id).expect("Failed to parse test PeerId")
}

mock! {
    pub ServiceControl {}
    impl ant_service_management::control::ServiceControl for ServiceControl {
        fn create_service_user(&self, username: &str) -> ServiceControlResult<()>;
        fn get_available_port(&self) -> ServiceControlResult<u16>;
        fn install(&self, install_ctx: ServiceInstallCtx, user_mode: bool) -> ServiceControlResult<()>;
        fn get_process_pid(&self, bin_path: &Path) -> ServiceControlResult<u32>;
        fn start(&self, service_name: &str, user_mode: bool) -> ServiceControlResult<()>;
        fn stop(&self, service_name: &str, user_mode: bool) -> ServiceControlResult<()>;
        fn uninstall(&self, service_name: &str, user_mode: bool) -> ServiceControlResult<()>;
        fn wait(&self, delay: u64);
    }
}

mock! {
    pub MetricsClient {}
    #[async_trait]
    impl MetricsAction for MetricsClient {
        async fn get_node_metrics(&self) -> Result<NodeMetrics, MetricsActionError>;
        async fn get_node_metadata_extended(&self) -> Result<ant_service_management::metric::NodeMetadataExtended, MetricsActionError>;
    }

}

mock! {
    pub FileSystemClient {}

    impl FileSystemActions for FileSystemClient {
        fn node_info(&self, root_dir: &std::path::Path) -> Result<ant_service_management::fs::NodeInfo, ant_service_management::Error>;
    }
}

// Helper function to create a test NodeRegistryManager
pub fn create_test_registry() -> NodeRegistryManager {
    // Create a temporary path for testing
    let temp_path = std::env::temp_dir().join(format!("test_registry_{}.json", std::process::id()));
    NodeRegistryManager::empty(temp_path)
}

// Helper function to create test service data
pub fn create_test_service_data(number: u16) -> NodeServiceData {
    NodeServiceData {
        alpha: false,
        auto_restart: false,
        connected_peers: 10,
        data_dir_path: PathBuf::from(format!("/var/antctl/services/antnode{number}")),
        evm_network: EvmNetwork::Custom(CustomNetwork {
            rpc_url_http: "http://localhost:8545".parse().unwrap(),
            payment_token_address: RewardsAddress::from_str(
                "0x5FbDB2315678afecb367f032d93F642f64180aa3",
            )
            .unwrap(),
            data_payments_address: RewardsAddress::from_str(
                "0x8464135c8F25Da09e49BC8782676a84730C318bC",
            )
            .unwrap(),
        }),
        initial_peers_config: InitialPeersConfig::default(),
        listen_addr: None,
        log_dir_path: PathBuf::from(format!("/var/log/antnode/antnode{number}")),
        log_format: None,
        max_archived_log_files: None,
        max_log_files: None,
        metrics_port: 600 + number,
        network_id: None,
        node_ip: None,
        node_port: None,
        number,
        peer_id: None,
        pid: None,
        reachability_check_progress: None,
        relay: false,
        rewards_address: RewardsAddress::from_str("0x03B770D9cD32077cC0bF330c13C114a87643B124")
            .unwrap(),
        rpc_socket_addr: None,
        antnode_path: PathBuf::from(format!("/var/antctl/services/antnode{number}/antnode")),
        schema_version: NODE_SERVICE_DATA_SCHEMA_LATEST,
        service_name: format!("antnode{number}"),
        skip_reachability_check: false,
        status: ServiceStatus::Added,
        no_upnp: false,
        user: Some("ant".to_string()),
        user_mode: false,
        version: "0.98.1".to_string(),
        write_older_cache_files: false,
    }
}

// Helper function to create test services with mocks
pub fn create_test_services_with_mocks(count: usize) -> Result<Vec<NodeService>> {
    let peer_ids = (1..=count)
        .map(|i| get_test_peer_id(i - 1))
        .collect::<Vec<_>>();

    let mut services = Vec::new();

    for i in 1..=count {
        let peer_ids_clone = peer_ids.clone();
        let mut mock_fs_client = MockFileSystemClient::new();
        let mut mock_metrics_client = MockMetricsClient::new();

        // Set up mock expectations
        mock_metrics_client
            .expect_get_node_metrics()
            .times(2)
            .returning(move || {
                Ok(NodeMetrics {
                    reachability_status: ReachabilityStatusValues {
                        progress_percent: 100,
                        upnp: false,
                        public: true,
                        private: false,
                    },
                    connected_peers: 10,
                })
            });

        mock_metrics_client
            .expect_get_node_metadata_extended()
            .times(1)
            .returning(move || {
                Ok(NodeMetadataExtended {
                    pid: 1000 + i as u32,
                    peer_id: peer_ids_clone[i - 1],
                    root_dir: PathBuf::from(format!("/var/antctl/services/antnode{i}")),
                    log_dir: PathBuf::from(format!("/var/log/antnode/antnode{i}")),
                })
            });

        mock_fs_client
            .expect_node_info()
            .times(1)
            .returning(move |_root_dir| {
                Ok(NodeInfo {
                    listeners: vec![
                        Multiaddr::from_str(&format!("/ip4/127.0.0.1/udp/600{i}")).unwrap(),
                    ],
                })
            });

        let service_data = create_test_service_data(i as u16);
        let service_data = Arc::new(RwLock::new(service_data));

        let service = NodeService::new(
            service_data,
            Box::new(mock_fs_client),
            Box::new(mock_metrics_client),
        );

        services.push(service);
    }

    Ok(services)
}

// Helper function to create test services without RPC mocks (for simple tests)
pub fn create_test_services_simple(count: usize) -> Vec<NodeService> {
    (1..=count)
        .map(|i| {
            let service_data = create_test_service_data(i as u16);
            let service_data = Arc::new(RwLock::new(service_data));

            let mock_fs_client = MockFileSystemClient::new();
            let mock_metrics_client = MockMetricsClient::new();

            NodeService::new(
                service_data,
                Box::new(mock_fs_client),
                Box::new(mock_metrics_client),
            )
        })
        .collect()
}

/// Represents different progress scenarios for testing
#[derive(Debug, Clone)]
pub enum MockMetricsProgressScenario {
    /// Immediate completion (100% from start)
    Immediate,
    /// Incremental progress: starts at 0%, increases by increment each call
    Incremental { start: u8, increment: u8, max: u8 },
    /// Multiple stages with different progress values
    Staged(Vec<u8>),
    /// Never called - for services that fail to start and never reach progress monitoring
    NeverCalled,
    /// Gets stuck at a specific progress value
    StuckAt(u8),
}

/// Helper function to create test services with progressive metrics mocking
pub fn create_test_services_with_progressive_mocks(
    count: usize,
    progress_scenarios: Vec<MockMetricsProgressScenario>,
) -> Result<Vec<NodeService>> {
    let peer_ids = (1..=count)
        .map(|i| get_test_peer_id(i - 1))
        .collect::<Vec<_>>();

    let mut services = Vec::new();

    for i in 1..=count {
        let peer_ids_clone = peer_ids.clone();
        let mut mock_fs_client = MockFileSystemClient::new();
        let mut mock_metrics_client = MockMetricsClient::new();

        let scenario = progress_scenarios
            .get(i - 1)
            .unwrap_or(&MockMetricsProgressScenario::Immediate)
            .clone();

        // Set up file system mock
        mock_fs_client
            .expect_node_info()
            .times(0..=1)
            .returning(move |_root_dir| {
                Ok(NodeInfo {
                    listeners: vec![
                        Multiaddr::from_str(&format!("/ip4/127.0.0.1/udp/600{i}")).unwrap(),
                    ],
                })
            });

        match &scenario {
            MockMetricsProgressScenario::StuckAt(_) => {
                mock_metrics_client
                    .expect_get_node_metadata_extended()
                    .times(0);
            }
            MockMetricsProgressScenario::NeverCalled => {
                mock_metrics_client
                    .expect_get_node_metadata_extended()
                    .times(0);
            }
            _ => {
                mock_metrics_client
                    .expect_get_node_metadata_extended()
                    .times(0..=1)
                    .returning(move || {
                        Ok(NodeMetadataExtended {
                            pid: 1000 + i as u32,
                            peer_id: peer_ids_clone[i - 1],
                            root_dir: PathBuf::from(format!("/var/antctl/services/antnode{i}")),
                            log_dir: PathBuf::from(format!("/var/log/antnode/antnode{i}")),
                        })
                    });
            }
        }

        // Set up metrics mock based on scenario
        setup_metrics_mock_for_scenario(&mut mock_metrics_client, scenario);

        let service_data = create_test_service_data(i as u16);
        let service_data = Arc::new(RwLock::new(service_data));

        let service = NodeService::new(
            service_data,
            Box::new(mock_fs_client),
            Box::new(mock_metrics_client),
        );

        services.push(service);
    }

    Ok(services)
}

/// Sets up metrics mock expectations based on the progress scenario
fn setup_metrics_mock_for_scenario(
    mock_metrics_client: &mut MockMetricsClient,
    scenario: MockMetricsProgressScenario,
) {
    match scenario {
        MockMetricsProgressScenario::Immediate => {
            mock_metrics_client
                .expect_get_node_metrics()
                .times(1..)
                .returning(|| {
                    Ok(NodeMetrics {
                        reachability_status: ReachabilityStatusValues {
                            progress_percent: 100,
                            upnp: false,
                            public: true,
                            private: false,
                        },
                        connected_peers: 10,
                    })
                });
        }
        MockMetricsProgressScenario::Incremental {
            start,
            increment,
            max,
        } => {
            let progress = Arc::new(AtomicU8::new(start));
            mock_metrics_client
                .expect_get_node_metrics()
                .times(1..)
                .returning(move || {
                    let current = progress.load(Ordering::Relaxed);
                    let next = if current + increment >= max {
                        max
                    } else {
                        current + increment
                    };
                    progress.store(next, Ordering::Relaxed);

                    Ok(NodeMetrics {
                        reachability_status: ReachabilityStatusValues {
                            progress_percent: current,
                            upnp: false,
                            public: true,
                            private: false,
                        },
                        connected_peers: 10,
                    })
                });
        }
        MockMetricsProgressScenario::Staged(stages) => {
            let call_count = Arc::new(AtomicU8::new(0));
            let stages = Arc::new(stages);
            mock_metrics_client
                .expect_get_node_metrics()
                .times(1..)
                .returning(move || {
                    let count = call_count.fetch_add(1, Ordering::Relaxed) as usize;
                    let progress = stages
                        .get(count)
                        .copied()
                        .unwrap_or(*stages.last().unwrap());

                    Ok(NodeMetrics {
                        reachability_status: ReachabilityStatusValues {
                            progress_percent: progress,
                            upnp: false,
                            public: true,
                            private: false,
                        },
                        connected_peers: 10,
                    })
                });
        }
        MockMetricsProgressScenario::NeverCalled => {
            // Don't set up any expectations - this mock should never be called
        }

        MockMetricsProgressScenario::StuckAt(progress) => {
            mock_metrics_client
                .expect_get_node_metrics()
                .times(1..)
                .returning(move || {
                    Ok(NodeMetrics {
                        reachability_status: ReachabilityStatusValues {
                            progress_percent: progress,
                            upnp: false,
                            public: true,
                            private: false,
                        },
                        connected_peers: 10,
                    })
                });
        }
    }
}
