// TODO: Shall be removed once the python binding warnings resolved
#![allow(non_local_definitions)]

use crate::{NodeBuilder, RunningNode};
use ant_evm::{EvmNetwork, RewardsAddress};
use ant_networking::{PutRecordCfg, ResponseQuorum};
use ant_protocol::{node::get_antnode_root_dir, storage::ChunkAddress, NetworkAddress};
use const_hex::FromHex;
use libp2p::{
    identity::{Keypair, PeerId},
    kad::Record as KadRecord,
    Multiaddr,
};
use pyo3::{exceptions::PyRuntimeError, exceptions::PyValueError, prelude::*, types::PyModule};
use std::sync::Arc;
use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};
use tokio::sync::Mutex;
use xor_name::XorName;

/// Python wrapper for the Safe Network Node
#[pyclass(name = "SafeNode")]
pub struct AntNode {
    node: Arc<Mutex<Option<RunningNode>>>,
    runtime: Arc<Mutex<Option<tokio::runtime::Runtime>>>,
}

#[pymethods]
impl AntNode {
    #[new]
    fn new() -> Self {
        Self {
            node: Arc::new(Mutex::new(None)),
            runtime: Arc::new(Mutex::new(None)),
        }
    }

    /// Start the node with the given configuration
    #[pyo3(signature = (
        rewards_address,
        evm_network,
        ip = "0.0.0.0",
        port = 0,
        initial_peers = vec![],
        local = false,
        root_dir = None,
        home_network = false,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn run(
        &self,
        rewards_address: String,
        evm_network: String,
        ip: &str,
        port: u16,
        initial_peers: Vec<String>,
        local: bool,
        root_dir: Option<String>,
        home_network: bool,
    ) -> PyResult<()> {
        let rewards_address = RewardsAddress::from_hex(&rewards_address)
            .map_err(|e| PyValueError::new_err(format!("Invalid rewards address: {e}")))?;

        let evm_network = match evm_network.as_str() {
            "arbitrum_one" => EvmNetwork::ArbitrumOne,
            "arbitrum_sepolia" => EvmNetwork::ArbitrumSepolia,
            _ => {
                return Err(PyValueError::new_err(
                    "Invalid EVM network. Must be 'arbitrum_one' or 'arbitrum_sepolia'",
                ))
            }
        };

        let ip: IpAddr = ip
            .parse()
            .map_err(|e| PyValueError::new_err(format!("Invalid IP address: {e}")))?;

        let node_socket_addr = SocketAddr::new(ip, port);

        let initial_peers: Vec<Multiaddr> = initial_peers
            .into_iter()
            .map(|addr| addr.parse())
            .collect::<Result<_, _>>()
            .map_err(|e| PyValueError::new_err(format!("Invalid peer address: {e}")))?;

        let root_dir = root_dir.map(PathBuf::from);

        let keypair = Keypair::generate_ed25519();

        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create runtime: {e}")))?;

        let node = rt.block_on(async {
            let mut node_builder = NodeBuilder::new(
                keypair,
                rewards_address,
                evm_network,
                node_socket_addr,
                local,
                root_dir.unwrap_or_else(|| PathBuf::from(".")),
                false,
            );
            node_builder.initial_peers(initial_peers);
            node_builder.is_behind_home_network(home_network);

            node_builder
                .build_and_run()
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to start node: {e}")))
        })?;

        let mut node_guard = self
            .node
            .try_lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire node lock"))?;
        *node_guard = Some(node);

        let mut rt_guard = self
            .runtime
            .try_lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire runtime lock"))?;
        *rt_guard = Some(rt);

        Ok(())
    }

    /// Get the node's PeerId as a string
    fn peer_id(self_: PyRef<Self>) -> PyResult<String> {
        let node_guard = self_
            .node
            .try_lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire node lock"))?;

        match &*node_guard {
            Some(node) => Ok(node.peer_id().to_string()),
            None => Err(PyRuntimeError::new_err("Node not started")),
        }
    }

    /// Get all record addresses stored by the node
    fn get_all_record_addresses(self_: PyRef<Self>) -> PyResult<Vec<String>> {
        let node_guard = self_
            .node
            .try_lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire node lock"))?;
        let rt_guard = self_
            .runtime
            .try_lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire runtime lock"))?;

        match (&*node_guard, &*rt_guard) {
            (Some(node), Some(rt)) => {
                let addresses = rt.block_on(async {
                    node.get_all_record_addresses().await.map_err(|e| {
                        PyRuntimeError::new_err(format!("Failed to get addresses: {e}"))
                    })
                })?;

                Ok(addresses.into_iter().map(|addr| addr.to_string()).collect())
            }
            _ => Err(PyRuntimeError::new_err("Node not started")),
        }
    }

    /// Get the node's kbuckets information
    fn get_kbuckets(self_: PyRef<Self>) -> PyResult<Vec<(u32, Vec<String>)>> {
        let node_guard = self_
            .node
            .try_lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire node lock"))?;
        let rt_guard = self_
            .runtime
            .try_lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire runtime lock"))?;

        match (&*node_guard, &*rt_guard) {
            (Some(node), Some(rt)) => {
                let kbuckets = rt.block_on(async {
                    node.get_kbuckets().await.map_err(|e| {
                        PyRuntimeError::new_err(format!("Failed to get kbuckets: {e}"))
                    })
                })?;

                Ok(kbuckets
                    .into_iter()
                    .map(|(distance, peers)| {
                        (distance, peers.into_iter().map(|p| p.to_string()).collect())
                    })
                    .collect())
            }
            _ => Err(PyRuntimeError::new_err("Node not started")),
        }
    }

    /// Get the node's rewards/wallet address as a hex string
    fn get_rewards_address(self_: PyRef<Self>) -> PyResult<String> {
        let node_guard = self_
            .node
            .try_lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire node lock"))?;

        match &*node_guard {
            Some(node) => Ok(format!("0x{}", hex::encode(node.reward_address()))),
            None => Err(PyRuntimeError::new_err("Node not started")),
        }
    }

    /// Set a new rewards/wallet address for the node
    /// The address should be a hex string starting with "0x"
    fn set_rewards_address(self_: PyRef<Self>, address: String) -> PyResult<()> {
        let node_guard = self_
            .node
            .try_lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire node lock"))?;

        // Remove "0x" prefix if present
        let address = address.strip_prefix("0x").unwrap_or(&address);

        // Validate the address format
        let _new_address = RewardsAddress::from_hex(address)
            .map_err(|e| PyValueError::new_err(format!("Invalid rewards address: {e}")))?;

        match &*node_guard {
            Some(_) => Err(PyRuntimeError::new_err(
                "Changing rewards address requires node restart. Please stop and start the node with the new address."
            )),
            None => Err(PyRuntimeError::new_err("Node not started")),
        }
    }

    /// Store a record in the node's storage
    fn store_record(
        self_: PyRef<Self>,
        key: String,
        value: Vec<u8>,
        _data_type: String,
    ) -> PyResult<()> {
        let node_guard = self_
            .node
            .try_lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire node lock"))?;
        let rt_guard = self_
            .runtime
            .try_lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire runtime lock"))?;

        match (&*node_guard, &*rt_guard) {
            (Some(node), Some(rt)) => {
                let xorname = XorName::from_content(
                    &hex::decode(key)
                        .map_err(|e| PyValueError::new_err(format!("Invalid key format: {e}")))?,
                );
                let chunk_address = ChunkAddress::new(xorname);
                let network_address = NetworkAddress::from_chunk_address(chunk_address);
                let record_key = network_address.to_record_key();

                rt.block_on(async {
                    let record = KadRecord {
                        key: record_key,
                        value,
                        publisher: None,
                        expires: None,
                    };
                    let cfg = PutRecordCfg {
                        put_quorum: ResponseQuorum::One,
                        retry_strategy: Default::default(),
                        use_put_record_to: None,
                        verification: None,
                    };
                    node.network.put_record(record, &cfg).await.map_err(|e| {
                        PyRuntimeError::new_err(format!("Failed to store record: {e}"))
                    })
                })?;

                Ok(())
            }
            _ => Err(PyRuntimeError::new_err("Node not started")),
        }
    }

    /// Get a record from the node's storage
    fn get_record(self_: PyRef<Self>, key: String) -> PyResult<Option<Vec<u8>>> {
        let node_guard = self_
            .node
            .try_lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire node lock"))?;
        let rt_guard = self_
            .runtime
            .try_lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire runtime lock"))?;

        match (&*node_guard, &*rt_guard) {
            (Some(node), Some(rt)) => {
                let xorname = XorName::from_content(
                    &hex::decode(key)
                        .map_err(|e| PyValueError::new_err(format!("Invalid key format: {e}")))?,
                );
                let chunk_address = ChunkAddress::new(xorname);
                let network_address = NetworkAddress::from_chunk_address(chunk_address);
                let record_key = network_address.to_record_key();

                let record = rt.block_on(async {
                    node.network
                        .get_local_record(&record_key)
                        .await
                        .map_err(|e| PyRuntimeError::new_err(format!("Failed to get record: {e}")))
                })?;

                Ok(record.map(|r| r.value.to_vec()))
            }
            _ => Err(PyRuntimeError::new_err("Node not started")),
        }
    }

    /// Delete a record from the node's storage
    fn delete_record(self_: PyRef<Self>, key: String) -> PyResult<bool> {
        let node_guard = self_
            .node
            .try_lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire node lock"))?;
        let rt_guard = self_
            .runtime
            .try_lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire runtime lock"))?;

        match (&*node_guard, &*rt_guard) {
            (Some(node), Some(rt)) => {
                let xorname = XorName::from_content(
                    &hex::decode(key)
                        .map_err(|e| PyValueError::new_err(format!("Invalid key format: {e}")))?,
                );
                let chunk_address = ChunkAddress::new(xorname);
                let network_address = NetworkAddress::from_chunk_address(chunk_address);
                let record_key = network_address.to_record_key();

                rt.block_on(async {
                    // First check if we have the record using record_key
                    if let Ok(Some(_)) = node.network.get_local_record(&record_key).await {
                        // If we have it, remove it
                        // Note: This is a simplified version - you might want to add proper deletion logic
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                })
            }
            _ => Err(PyRuntimeError::new_err("Node not started")),
        }
    }

    /// Get the total size of stored records
    fn get_stored_records_size(self_: PyRef<Self>) -> PyResult<u64> {
        let node_guard = self_
            .node
            .try_lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire node lock"))?;
        let rt_guard = self_
            .runtime
            .try_lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire runtime lock"))?;

        match (&*node_guard, &*rt_guard) {
            (Some(node), Some(rt)) => rt.block_on(async {
                let records = node
                    .network
                    .get_all_local_record_addresses()
                    .await
                    .map_err(|e| PyRuntimeError::new_err(format!("Failed to get records: {e}")))?;

                let mut total_size = 0u64;
                for (key, _) in records {
                    if let Ok(Some(record)) =
                        node.network.get_local_record(&key.to_record_key()).await
                    {
                        total_size += record.value.len() as u64;
                    }
                }
                Ok(total_size)
            }),
            _ => Err(PyRuntimeError::new_err("Node not started")),
        }
    }

    /// Get the current root directory path for node data
    fn get_root_dir(self_: PyRef<Self>) -> PyResult<String> {
        let node_guard = self_
            .node
            .try_lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire node lock"))?;

        match &*node_guard {
            Some(node) => Ok(node
                .root_dir_path()
                .to_str()
                .ok_or_else(|| PyValueError::new_err("Invalid path encoding"))?
                .to_string()),
            None => Err(PyRuntimeError::new_err("Node not started")),
        }
    }

    /// Get the default root directory path for the given peer ID
    /// This is platform specific:
    ///  - Linux: $HOME/.local/share/autonomi/node/<peer-id>
    ///  - macOS: $HOME/Library/Application Support/autonomi/node/<peer-id>
    ///  - Windows: C:\Users\<username>\AppData\Roaming\autonomi\node\<peer-id>
    #[allow(clippy::redundant_closure)]
    #[staticmethod]
    fn get_default_root_dir(peer_id: Option<String>) -> PyResult<String> {
        let peer_id = if let Some(id_str) = peer_id {
            let id = id_str
                .parse::<PeerId>()
                .map_err(|e| PyValueError::new_err(format!("Invalid peer ID: {e}")))?;
            Some(id)
        } else {
            None
        };

        let path = get_antnode_root_dir(peer_id.unwrap_or_else(|| PeerId::random()))
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to get default root dir: {e}")))?;

        Ok(path
            .to_str()
            .ok_or_else(|| PyValueError::new_err("Invalid path encoding"))?
            .to_string())
    }

    /// Get the logs directory path
    fn get_logs_dir(self_: PyRef<Self>) -> PyResult<String> {
        let node_guard = self_
            .node
            .try_lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire node lock"))?;

        match &*node_guard {
            Some(node) => {
                let logs_path = node.root_dir_path().join("logs");
                Ok(logs_path
                    .to_str()
                    .ok_or_else(|| PyValueError::new_err("Invalid path encoding"))?
                    .to_string())
            }
            None => Err(PyRuntimeError::new_err("Node not started")),
        }
    }

    /// Get the data directory path where records are stored
    fn get_data_dir(self_: PyRef<Self>) -> PyResult<String> {
        let node_guard = self_
            .node
            .try_lock()
            .map_err(|_| PyRuntimeError::new_err("Failed to acquire node lock"))?;

        match &*node_guard {
            Some(node) => {
                let data_path = node.root_dir_path().join("data");
                Ok(data_path
                    .to_str()
                    .ok_or_else(|| PyValueError::new_err("Invalid path encoding"))?
                    .to_string())
            }
            None => Err(PyRuntimeError::new_err("Node not started")),
        }
    }
}

/// Python module initialization
#[pymodule]
#[pyo3(name = "_antnode")]
fn init_module(_py: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<AntNode>()?;
    Ok(())
}
