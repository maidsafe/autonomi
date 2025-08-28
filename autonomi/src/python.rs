// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// Standard library imports
use std::{
    path::PathBuf,
    str::FromStr,
    sync::Arc,
    time::{Duration, SystemTime},
};

// External dependencies
use ant_bootstrap::BootstrapCacheConfig;
use ant_evm::{PaymentQuote, QuotingMetrics, RewardsAddress};
use ant_protocol::storage::DataTypes;
use bls::{PK_SIZE, PublicKey, SecretKey};
use bytes::Bytes;
use exponential_backoff::Backoff;
use libp2p::Multiaddr;
use pyo3::{
    basic::CompareOp,
    exceptions::{PyConnectionError, PyRuntimeError, PyValueError},
    prelude::*,
};
use pyo3_async_runtimes::tokio::future_into_py;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use xor_name::{XOR_NAME_LEN, XorName};

// Internal imports
use crate::{
    Amount, AttoTokens, Chunk, ChunkAddress, Client, ClientConfig, ClientOperatingStrategy,
    GraphEntry, GraphEntryAddress, InitialPeersConfig, MaxFeePerGas, Network as EVMNetwork,
    Pointer, PointerAddress, Scratchpad, ScratchpadAddress, Signature, TransactionConfig, Wallet,
    client::{
        ClientEvent, UploadSummary,
        chunk::DataMapChunk,
        data::DataAddress,
        files::{archive_private::PrivateArchiveDataMap, archive_public::ArchiveAddress},
        key_derivation::{
            DerivationIndex, DerivedPubkey, DerivedSecretKey, MainPubkey, MainSecretKey,
        },
        payment::{PaymentOption, Receipt},
        pointer::PointerTarget,
        quote::{QuoteForAddress, StoreQuote},
        vault::{UserData, VaultSecretKey},
    },
    files::{Metadata, PrivateArchive, PublicArchive},
    networking::{PeerId, Quorum, RetryStrategy, Strategy},
    register::{RegisterAddress, RegisterHistory},
};

/// Helper function to convert ScratchpadError to appropriate Python exception
fn scratchpad_error_to_py_err(
    error: crate::client::data_types::scratchpad::ScratchpadError,
) -> PyErr {
    // Use the original Rust error message directly
    PyRuntimeError::new_err(format!("{error}"))
}

/// Enhanced helper that can decrypt conflicting data when owner key is available
fn scratchpad_error_to_py_err_with_owner(
    error: crate::client::data_types::scratchpad::ScratchpadError,
    owner_key: Option<&crate::client::data_types::scratchpad::SecretKey>,
) -> PyErr {
    use crate::client::data_types::scratchpad::ScratchpadError;

    match error {
        ScratchpadError::Fork(conflicting_scratchpads) => {
            let mut message = format!("{}", ScratchpadError::Fork(conflicting_scratchpads.clone()));

            // If we have the owner key, decrypt and show the actual conflicting data
            if let Some(owner) = owner_key {
                message.push_str("\n\nConflicting data content:");

                for (i, scratchpad) in conflicting_scratchpads.iter().enumerate() {
                    match scratchpad.decrypt_data(owner) {
                        Ok(decrypted_bytes) => match String::from_utf8(decrypted_bytes.to_vec()) {
                            Ok(decrypted_text) => {
                                message.push_str(&format!(
                                    "\n  Conflict {}: \"{}\" (Counter: {}, Hash: {})",
                                    i + 1,
                                    decrypted_text,
                                    scratchpad.counter(),
                                    hex::encode(scratchpad.encrypted_data_hash())[..16].to_string()
                                        + "..."
                                ));
                            }
                            Err(_) => {
                                message.push_str(&format!(
                                        "\n  Conflict {}: <binary data {} bytes> (Counter: {}, Hash: {})",
                                        i + 1,
                                        decrypted_bytes.len(),
                                        scratchpad.counter(),
                                        hex::encode(scratchpad.encrypted_data_hash())[..16].to_string() + "..."
                                    ));
                            }
                        },
                        Err(_) => {
                            message.push_str(&format!(
                                "\n  Conflict {}: <decryption failed> (Counter: {}, Hash: {})",
                                i + 1,
                                scratchpad.counter(),
                                hex::encode(scratchpad.encrypted_data_hash())[..16].to_string()
                                    + "..."
                            ));
                        }
                    }
                }

                let max_counter = conflicting_scratchpads
                    .iter()
                    .map(|s| s.counter())
                    .max()
                    .unwrap_or(0);

                message.push_str(&format!(
                    "\n\nChoose which data to keep and update with counter: {}",
                    max_counter + 1
                ));
            }

            PyRuntimeError::new_err(message)
        }
        _ => PyRuntimeError::new_err(format!("{error}")),
    }
}

#[pyclass(name = "AttoTokens")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PyAttoTokens {
    inner: AttoTokens,
}

#[pymethods]
impl PyAttoTokens {
    /// Creates a new instance with zero tokens
    #[staticmethod]
    fn zero() -> Self {
        Self {
            inner: AttoTokens::zero(),
        }
    }

    /// Returns whether this represents zero tokens
    fn is_zero(&self) -> bool {
        self.inner.is_zero()
    }

    /// Creates a new instance from atto amount
    #[staticmethod]
    fn from_atto(value: String) -> PyResult<Self> {
        let amount = Amount::from_str(&value)
            .map_err(|e| PyValueError::new_err(format!("Invalid amount: {e}")))?;
        Ok(Self {
            inner: AttoTokens::from_atto(amount),
        })
    }

    /// Creates a new instance from a u64 number of atto tokens
    #[staticmethod]
    fn from_u64(value: u64) -> Self {
        Self {
            inner: AttoTokens::from_u64(value),
        }
    }

    /// Creates a new instance from a u128 number of atto tokens
    #[staticmethod]
    fn from_u128(value: u128) -> Self {
        Self {
            inner: AttoTokens::from_u128(value),
        }
    }

    /// Gets the amount as an atto value string
    fn as_atto(&self) -> String {
        self.inner.as_atto().to_string()
    }

    /// Adds another AttoTokens value, returning None if overflow occurred
    fn checked_add(&self, rhs: &PyAttoTokens) -> Option<PyAttoTokens> {
        self.inner
            .checked_add(rhs.inner)
            .map(|inner| PyAttoTokens { inner })
    }

    /// Subtracts another AttoTokens value, returning None if overflow occurred
    fn checked_sub(&self, rhs: &PyAttoTokens) -> Option<PyAttoTokens> {
        self.inner
            .checked_sub(rhs.inner)
            .map(|inner| PyAttoTokens { inner })
    }

    /// Converts the value as/to bytes
    fn as_bytes(&self) -> Vec<u8> {
        self.inner.to_bytes()
    }

    /// Creates a new instance from a string representation
    #[staticmethod]
    fn from_str(value_str: &str) -> PyResult<Self> {
        AttoTokens::from_str(value_str)
            .map(|inner| Self { inner })
            .map_err(|e| PyValueError::new_err(format!("Failed to parse AttoTokens: {e}")))
    }

    /// Returns the string representation of the value
    fn __str__(&self) -> String {
        self.inner.to_string()
    }

    /// Returns the representation of the value for debugging
    fn __repr__(&self) -> String {
        format!("AttoTokens('{}')", self.inner)
    }
}

#[pyclass(name = "DataTypes", eq, eq_int)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PyDataTypes {
    Chunk = 0,
    GraphEntry = 1,
    Pointer = 2,
    Scratchpad = 3,
}

impl From<PyDataTypes> for DataTypes {
    fn from(py_data_type: PyDataTypes) -> Self {
        match py_data_type {
            PyDataTypes::Chunk => DataTypes::Chunk,
            PyDataTypes::GraphEntry => DataTypes::GraphEntry,
            PyDataTypes::Pointer => DataTypes::Pointer,
            PyDataTypes::Scratchpad => DataTypes::Scratchpad,
        }
    }
}

#[pyclass(name = "Client")]
pub(crate) struct PyClient {
    inner: Client,
}

#[pymethods]
impl PyClient {
    /// Initialize the client with default configuration.
    #[staticmethod]
    fn init(py: Python) -> PyResult<Bound<PyAny>> {
        future_into_py(py, async {
            let inner = Client::init()
                .await
                .map_err(|e| PyConnectionError::new_err(format!("Failed to connect: {e}")))?;
            Ok(PyClient { inner })
        })
    }

    /// Initialize a client that is configured to be local.
    #[staticmethod]
    fn init_local(py: Python) -> PyResult<Bound<PyAny>> {
        future_into_py(py, async {
            let inner = Client::init_local()
                .await
                .map_err(|e| PyConnectionError::new_err(format!("Failed to connect: {e}")))?;
            Ok(PyClient { inner })
        })
    }

    /// Initialize a client that is configured to be connected to the alpha network.
    #[staticmethod]
    fn init_alpha(py: Python) -> PyResult<Bound<PyAny>> {
        future_into_py(py, async {
            let inner = Client::init_alpha()
                .await
                .map_err(|e| PyConnectionError::new_err(format!("Failed to connect: {e}")))?;
            Ok(PyClient { inner })
        })
    }

    /// Initialize a client that bootstraps from a list of peers.
    ///
    /// If any of the provided peers is a global address, the client will not be local.
    #[staticmethod]
    fn init_with_peers(py: Python, peers: Vec<String>) -> PyResult<Bound<PyAny>> {
        let peers: Vec<Multiaddr> = peers
            .iter()
            .map(|p| Multiaddr::from_str(p))
            .collect::<Result<_, _>>()
            .map_err(|e| PyValueError::new_err(format!("Failed to parse peers: {e}")))?;

        future_into_py(py, async {
            let inner = Client::init_with_peers(peers)
                .await
                .map_err(|e| PyConnectionError::new_err(format!("Failed to connect: {e}")))?;
            Ok(PyClient { inner })
        })
    }

    /// Initialize the client with the given configuration.
    #[staticmethod]
    fn init_with_config(py: Python, config: PyClientConfig) -> PyResult<Bound<PyAny>> {
        future_into_py(py, async {
            let inner = Client::init_with_config(config.inner)
                .await
                .map_err(|e| PyConnectionError::new_err(format!("Failed to connect: {e}")))?;
            Ok(PyClient { inner })
        })
    }

    fn enable_client_events(&mut self) -> PyClientEventReceiver {
        let receiver = self.inner.enable_client_events();
        PyClientEventReceiver {
            inner: Arc::new(futures::lock::Mutex::new(receiver)),
        }
    }

    /// Returns the EVM network used by this client.
    fn evm_network(&self) -> PyEVMNetwork {
        PyEVMNetwork {
            inner: self.inner.evm_network().clone(),
        }
    }

    /// Get the cost of storing a chunk on the network
    fn chunk_cost<'a>(&self, py: Python<'a>, addr: PyChunkAddress) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let cost = client
                .chunk_cost(&addr.inner)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to get chunk cost: {e}")))?;
            Ok(cost.to_string())
        })
    }

    /// Get a chunk from the network.
    fn chunk_get<'a>(&self, py: Python<'a>, addr: &PyChunkAddress) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();
        let addr = addr.inner;

        future_into_py(py, async move {
            let chunk = client
                .chunk_get(&addr)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to get chunk: {e}")))?;
            Ok(chunk.value.to_vec())
        })
    }

    /// Manually upload a chunk to the network. It is recommended to use the `data_put` method instead to upload data.
    fn chunk_put<'a>(
        &self,
        py: Python<'a>,
        data: Vec<u8>,
        payment: &PyPaymentOption,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();
        let payment = payment.inner.clone();
        let chunk = Chunk::new(Bytes::from(data));

        future_into_py(py, async move {
            let (cost, addr) = client
                .chunk_put(&chunk, payment)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to put chunk: {e}")))?;
            Ok((cost.to_string(), PyChunkAddress { inner: addr }))
        })
    }

    /// Fetches a GraphEntry from the network.
    fn graph_entry_get<'a>(
        &self,
        py: Python<'a>,
        addr: PyGraphEntryAddress,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let entry = client
                .graph_entry_get(&addr.inner)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to get graph entry: {e}")))?;
            Ok(PyGraphEntry { inner: entry })
        })
    }

    /// Check if a graph_entry exists on the network
    fn graph_entry_check_existance<'a>(
        &self,
        py: Python<'a>,
        addr: PyGraphEntryAddress,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let exists = client
                .graph_entry_check_existence(&addr.inner)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to get graph entry: {e}")))?;
            Ok(exists)
        })
    }

    /// Manually puts a GraphEntry to the network.
    fn graph_entry_put<'a>(
        &self,
        py: Python<'a>,
        entry: PyGraphEntry,
        payment_option: &PyPaymentOption,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();
        let payment = payment_option.inner.clone();

        future_into_py(py, async move {
            let (cost, addr) = client
                .graph_entry_put(entry.inner, payment)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to get graph entry: {e}")))?;

            Ok((cost.to_string(), PyGraphEntryAddress { inner: addr }))
        })
    }

    /// Get the cost to create a GraphEntry
    fn graph_entry_cost<'a>(&self, py: Python<'a>, key: PyPublicKey) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let cost = client.graph_entry_cost(&key.inner).await.map_err(|e| {
                PyRuntimeError::new_err(format!("Failed to get graph entry cost: {e}"))
            })?;

            Ok(cost.to_string())
        })
    }

    /// Get Scratchpad from the Network.
    /// A Scratchpad is stored at the owner's public key so we can derive the address from it.
    fn scratchpad_get_from_public_key<'a>(
        &self,
        py: Python<'a>,
        public_key: PyPublicKey,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let scratchpad = client
                .scratchpad_get_from_public_key(&public_key.inner)
                .await
                .map_err(scratchpad_error_to_py_err)?;

            Ok(PyScratchpad { inner: scratchpad })
        })
    }

    /// Get Scratchpad from the Network using the scratpad address.
    fn scratchpad_get<'a>(
        &self,
        py: Python<'a>,
        addr: PyScratchpadAddress,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let scratchpad = client
                .scratchpad_get(&addr.inner)
                .await
                .map_err(scratchpad_error_to_py_err)?;

            Ok(PyScratchpad { inner: scratchpad })
        })
    }

    /// Check if a scratchpad exists on the network
    fn scratchpad_check_existance<'a>(
        &self,
        py: Python<'a>,
        addr: PyScratchpadAddress,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let exists = client
                .scratchpad_check_existence(&addr.inner)
                .await
                .map_err(scratchpad_error_to_py_err)?;

            Ok(exists)
        })
    }

    /// Manually store a scratchpad on the network
    fn scratchpad_put<'a>(
        &self,
        py: Python<'a>,
        scratchpad: PyScratchpad,
        payment_option: &PyPaymentOption,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();
        let payment = payment_option.inner.clone();

        future_into_py(py, async move {
            let (cost, addr) = client
                .scratchpad_put(scratchpad.inner, payment)
                .await
                .map_err(scratchpad_error_to_py_err)?;

            Ok((cost.to_string(), PyScratchpadAddress { inner: addr }))
        })
    }

    /// Create a new scratchpad to the network.
    ///
    /// Make sure that the owner key is not already used for another scratchpad as each key is associated with one scratchpad.
    /// The data will be encrypted with the owner key before being stored on the network.
    /// The content type is used to identify the type of data stored in the scratchpad, the choice is up to the caller.
    ///
    /// Returns the cost and the address of the scratchpad.
    fn scratchpad_create<'a>(
        &self,
        py: Python<'a>,
        owner: PySecretKey,
        content_type: u64,
        initial_data: Vec<u8>,
        payment_option: &PyPaymentOption,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();
        let payment = payment_option.inner.clone();

        future_into_py(py, async move {
            let (cost, addr) = client
                .scratchpad_create(
                    &owner.inner,
                    content_type,
                    &Bytes::from(initial_data),
                    payment,
                )
                .await
                .map_err(|e| scratchpad_error_to_py_err_with_owner(e, Some(&owner.inner)))?;

            Ok((cost.to_string(), PyScratchpadAddress { inner: addr }))
        })
    }

    /// Update an existing scratchpad to the network.
    /// The scratchpad needs to be created first with `scratchpad_create`.
    /// This operation is free as the scratchpad was already paid for at creation.
    /// Only the latest version of the scratchpad is kept on the Network, previous versions will be overwritten and unrecoverable.
    fn scratchpad_update<'a>(
        &self,
        py: Python<'a>,
        owner: PySecretKey,
        content_type: u64,
        data: Vec<u8>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            client
                .scratchpad_update(&owner.inner, content_type, &Bytes::from(data))
                .await
                .map_err(|e| scratchpad_error_to_py_err_with_owner(e, Some(&owner.inner)))?;

            Ok(())
        })
    }

    /// Update an existing scratchpad from a specific scratchpad to the network.
    ///
    /// This will increment the counter of the scratchpad and update the content.
    /// This function is used internally by `scratchpad_update` after the scratchpad has been retrieved from the network.
    /// To skip the retrieval step if you already have the scratchpad, use this function directly.
    /// This function will return the new scratchpad after it has been updated.
    fn scratchpad_update_from<'a>(
        &self,
        py: Python<'a>,
        current: PyScratchpad,
        owner: PySecretKey,
        content_type: u64,
        data: Vec<u8>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let new_scratchpad = client
                .scratchpad_update_from(
                    &current.inner,
                    &owner.inner,
                    content_type,
                    &Bytes::from(data),
                )
                .await
                .map_err(|e| scratchpad_error_to_py_err_with_owner(e, Some(&owner.inner)))?;

            Ok(PyScratchpad {
                inner: new_scratchpad,
            })
        })
    }

    /// Get the cost of creating a new Scratchpad
    fn scratchpad_cost<'a>(
        &self,
        py: Python<'a>,
        public_key: PyPublicKey,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let cost = client
                .scratchpad_cost(&public_key.inner)
                .await
                .map_err(|e| {
                    PyRuntimeError::new_err(format!("Failed to get scratchpad cost: {e}"))
                })?;

            Ok(cost.to_string())
        })
    }

    /// Verify a scratchpad
    #[staticmethod]
    fn scratchpad_verify(scratchpad: &PyScratchpad) -> PyResult<()> {
        Client::scratchpad_verify(&scratchpad.inner).map_err(scratchpad_error_to_py_err)
    }

    /// Get the cost of storing an archive on the network
    fn archive_cost<'a>(
        &self,
        py: Python<'a>,
        archive: PyPublicArchive,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let cost = client
                .archive_cost(&archive.inner)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to get archive cost: {e}")))?;
            Ok(cost.to_string())
        })
    }

    /// Fetch a private archive from the network using its datamap
    fn archive_get<'a>(
        &self,
        py: Python<'a>,
        data_map: &PyDataMapChunk,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();
        let data_map = data_map.inner.clone();

        future_into_py(py, async move {
            let archive = client
                .archive_get(&data_map)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to get archive: {e}")))?;

            Ok(PyPrivateArchive { inner: archive })
        })
    }

    /// Upload a private archive to the network
    fn archive_put<'a>(
        &self,
        py: Python<'a>,
        archive: PyPrivateArchive,
        payment: PyPaymentOption,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let (cost, data_map) = client
                .archive_put(&archive.inner, payment.inner)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to put archive: {e}")))?;

            Ok((cost.to_string(), PyDataMapChunk { inner: data_map }))
        })
    }

    /// Upload a public archive to the network
    fn archive_put_public<'a>(
        &self,
        py: Python<'a>,
        archive: PyPublicArchive,
        payment: PyPaymentOption,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let (cost, addr) = client
                .archive_put_public(&archive.inner, payment.inner)
                .await
                .map_err(|e| {
                    PyRuntimeError::new_err(format!("Failed to put public archive: {e}"))
                })?;

            Ok((cost.to_string(), PyArchiveAddress { inner: addr }))
        })
    }

    /// Get the cost to upload a file/dir to the network.
    fn file_cost<'a>(&self, py: Python<'a>, path: PathBuf) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let cost = client
                .file_cost(&path)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to get file cost: {e}")))?;

            Ok(cost.to_string())
        })
    }

    /// Download a private file from network to local file system.
    fn file_download<'a>(
        &self,
        py: Python<'a>,
        data_map: PyDataMapChunk,
        path: PathBuf,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            client
                .file_download(&data_map.inner, path)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to download file: {e}")))?;

            Ok(())
        })
    }

    /// Download a private directory from network to local file system
    fn dir_download<'a>(
        &self,
        py: Python<'a>,
        data_map: PyPrivateArchiveDataMap,
        dir_path: PathBuf,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            client
                .dir_download(&data_map.inner, dir_path)
                .await
                .map_err(|e| {
                    PyRuntimeError::new_err(format!("Failed to download directory: {e}"))
                })?;
            Ok(())
        })
    }

    /// Upload a directory to the network. The directory is recursively walked and each file is uploaded to the network.
    /// The datamaps of these (private) files are not uploaded but returned within the PrivateArchive return type.
    fn dir_content_upload<'a>(
        &self,
        py: Python<'a>,
        dir_path: PathBuf,
        payment: PyPaymentOption,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let (cost, archive) = client
                .dir_content_upload(dir_path, payment.inner)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to upload directory: {e}")))?;
            Ok((cost.to_string(), PyPrivateArchive { inner: archive }))
        })
    }

    /// Download file from network to local file system.
    fn file_download_public<'a>(
        &self,
        py: Python<'a>,
        addr: &PyDataAddress,
        path: PathBuf,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        let addr = addr.inner;
        future_into_py(py, async move {
            client
                .file_download_public(&addr, path)
                .await
                .map_err(|e| {
                    PyRuntimeError::new_err(format!("Failed to download public file: {e}"))
                })?;

            Ok(())
        })
    }

    /// Same as `dir_upload` but also uploads the archive (privately) to the network.
    ///
    /// Returns the datamap allowing the private archive to be downloaded from the network.
    fn dir_upload<'a>(
        &self,
        py: Python<'a>,
        dir_path: PathBuf,
        payment: PyPaymentOption,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let (cost, data_map) = client
                .dir_upload(dir_path, payment.inner)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to upload directory: {e}")))?;
            Ok((
                cost.to_string(),
                PyPrivateArchiveDataMap { inner: data_map },
            ))
        })
    }

    /// Upload the content of a private file to the network.
    /// Reads file, splits into chunks, uploads chunks, returns [`DataMapChunk`]
    fn file_content_upload<'a>(
        &self,
        py: Python<'a>,
        path: PathBuf,
        payment: PyPaymentOption,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let (cost, data_map) = client
                .file_content_upload(path, payment.inner)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to upload file: {e}")))?;
            Ok((cost.to_string(), PyDataMapChunk { inner: data_map }))
        })
    }

    /// Upload the content of a public file to the network.
    /// Reads file, splits into chunks, uploads chunks, uploads datamap, returns DataAddr (pointing to the datamap)
    fn file_content_upload_public<'a>(
        &self,
        py: Python<'a>,
        path: PathBuf,
        payment: PyPaymentOption,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let (cost, data_addr) = client
                .file_content_upload_public(path, payment.inner)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to upload file: {e}")))?;
            Ok((cost.to_string(), PyDataAddress { inner: data_addr }))
        })
    }

    /// Upload a piece of private data to the network. This data will be self-encrypted.
    /// The [`DataMapChunk`] is not uploaded to the network, keeping the data private.
    ///
    /// Returns the [`DataMapChunk`] containing the map to the encrypted chunks.
    fn data_put<'a>(
        &self,
        py: Python<'a>,
        data: Vec<u8>,
        payment: &PyPaymentOption,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();
        let payment = payment.inner.clone();

        future_into_py(py, async move {
            let (cost, data_map) = client
                .data_put(Bytes::from(data), payment)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to put data: {e}")))?;
            Ok((cost.to_string(), PyDataMapChunk { inner: data_map }))
        })
    }

    /// Fetch a blob of (private) data from the network
    fn data_get<'a>(&self, py: Python<'a>, access: &PyDataMapChunk) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();
        let access = access.inner.clone();

        future_into_py(py, async move {
            let data = client
                .data_get(&access)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to get data: {e}")))?;
            Ok(data.to_vec())
        })
    }

    /// Get the estimated cost of storing a piece of data.
    fn data_cost<'a>(&self, py: Python<'a>, data: Vec<u8>) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let cost = client
                .data_cost(Bytes::from(data))
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to get data cost: {e}")))?;
            Ok(cost.to_string())
        })
    }

    /// Upload a piece of data to the network. This data is publicly accessible.
    ///
    /// Returns the Data Address at which the data was stored.
    fn data_put_public<'a>(
        &self,
        py: Python<'a>,
        data: Vec<u8>,
        payment: &PyPaymentOption,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();
        let payment = payment.inner.clone();

        future_into_py(py, async move {
            let (cost, addr) = client
                .data_put_public(bytes::Bytes::from(data), payment)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to put data: {e}")))?;

            Ok((cost.to_string(), PyDataAddress { inner: addr }))
        })
    }

    /// Fetch a blob of data from the network
    fn data_get_public<'a>(
        &self,
        py: Python<'a>,
        addr: &PyDataAddress,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        let addr = addr.inner;
        future_into_py(py, async move {
            let data = client
                .data_get_public(&addr)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to get data: {e}")))?;
            Ok(data.to_vec())
        })
    }

    /// Upload a directory as a public archive to the network.
    /// Returns the network address where the archive is stored.
    fn dir_upload_public<'a>(
        &self,
        py: Python<'a>,
        dir_path: PathBuf,
        payment: &PyPaymentOption,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();
        let payment = payment.inner.clone();

        future_into_py(py, async move {
            let (cost, addr) = client
                .dir_upload_public(dir_path, payment)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to upload directory: {e}")))?;
            Ok((cost.to_string(), PyArchiveAddress { inner: addr }))
        })
    }

    /// Download a public archive from the network to a local directory.
    fn dir_download_public<'a>(
        &self,
        py: Python<'a>,
        addr: &PyArchiveAddress,
        dir_path: PathBuf,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        let addr = addr.inner;
        future_into_py(py, async move {
            client
                .dir_download_public(&addr, dir_path)
                .await
                .map_err(|e| {
                    PyRuntimeError::new_err(format!("Failed to download directory: {e}"))
                })?;
            Ok(())
        })
    }

    /// Upload a directory to the network. The directory is recursively walked and each file is uploaded to the network.
    ///
    /// The datamaps of these files are uploaded on the network, making the individual files publicly available.
    ///
    /// This returns, but does not upload (!),the `PublicArchive` containing the datamaps of the uploaded files.
    fn dir_content_upload_public<'a>(
        &self,
        py: Python<'a>,
        dir_path: PathBuf,
        payment: PyPaymentOption,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let (cost, archive) = client
                .dir_content_upload_public(dir_path, payment.inner)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to upload directory: {e}")))?;
            Ok((cost.to_string(), PyPublicArchive { inner: archive }))
        })
    }

    /// Get a public archive from the network.
    fn archive_get_public<'a>(
        &self,
        py: Python<'a>,
        addr: &PyArchiveAddress,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        let addr = addr.inner;
        future_into_py(py, async move {
            let archive = client
                .archive_get_public(&addr)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to get archive: {e}")))?;

            Ok(PyPublicArchive { inner: archive })
        })
    }

    /// Get the cost of creating a new vault.
    fn vault_cost<'a>(
        &self,
        py: Python<'a>,
        key: &PyVaultSecretKey,
        max_expected_size: u64,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();
        let key = key.inner.clone();

        future_into_py(py, async move {
            let cost = client
                .vault_cost(&key, max_expected_size)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to get vault cost: {e}")))?;
            Ok(cost.to_string())
        })
    }

    /// Put data into the client's VaultPacket
    ///
    /// Dynamically expand the vault capacity by paying for more space (Scratchpad) when needed.
    ///
    /// It is recommended to use the hash of the app name or unique identifier as the content type.
    fn vault_put<'a>(
        &self,
        py: Python<'a>,
        data: Vec<u8>,
        payment: &PyPaymentOption,
        key: &PyVaultSecretKey,
        content_type: u64,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();
        let payment = payment.inner.clone();
        let key = key.inner.clone();

        future_into_py(py, async move {
            match client
                .vault_put(bytes::Bytes::from(data), payment, &key, content_type)
                .await
            {
                Ok(cost) => Ok(cost.to_string()),
                Err(e) => Err(PyRuntimeError::new_err(format!(
                    "Failed to write to vault: {e}"
                ))),
            }
        })
    }

    /// Get the register history, starting from the root to the latest entry.
    ///
    /// This returns a [`RegisterHistory`] that can be use to get the register values from the history.
    ///
    /// [`RegisterHistory::next`] can be used to get the values one by one, from the first to the latest entry.
    /// [`RegisterHistory::collect`] can be used to get all the register values from the history from the first to the latest entry.
    fn register_history(&self, addr: String) -> PyResult<PyRegisterHistory> {
        let client = self.inner.clone();
        let addr = RegisterAddress::from_hex(&addr)
            .map_err(|e| PyValueError::new_err(format!("Failed to parse address: {e}")))?;

        let history = client.register_history(&addr);
        Ok(PyRegisterHistory::new(history))
    }

    /// Create a new register key from a SecretKey and a name.
    ///
    /// This derives a new `SecretKey` from the owner's `SecretKey` using the name.
    /// Note that you will need to keep track of the names you used to create the register key.
    #[staticmethod]
    fn register_key_from_name(owner: PySecretKey, name: &str) -> PyResult<PySecretKey> {
        let key = Client::register_key_from_name(&owner.inner, name);
        Ok(PySecretKey { inner: key })
    }

    /// Create a new RegisterValue from bytes, make sure the bytes are not longer than `REGISTER_VALUE_SIZE`
    #[staticmethod]
    fn register_value_from_bytes(bytes: &[u8]) -> PyResult<[u8; 32]> {
        let value = Client::register_value_from_bytes(bytes)
            .map_err(|e| PyValueError::new_err(format!("`bytes` has invalid length: {e}")))?;
        Ok(value)
    }

    /// Create a new register with an initial value.
    ///
    /// Note that two payments are required, one for the underlying `GraphEntry` and one for the `Pointer`.
    fn register_create<'a>(
        &self,
        py: Python<'a>,
        owner: PySecretKey,
        value: [u8; 32],
        payment: PyPaymentOption,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let (cost, addr) = client
                .register_create(&owner.inner, value, payment.inner)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to create register: {e}")))?;

            Ok((cost.to_string(), addr.to_hex()))
        })
    }

    /// Update the value of a register.
    ///
    /// The register needs to be created first with `register_create`.
    fn register_update<'a>(
        &self,
        py: Python<'a>,
        owner: PySecretKey,
        value: [u8; 32],
        payment: PyPaymentOption,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let cost = client
                .register_update(&owner.inner, value, payment.inner)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to update register: {e}")))?;

            Ok(cost.to_string())
        })
    }

    /// Get the current value of the register
    fn register_get<'a>(&self, py: Python<'a>, addr: String) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();
        let addr = RegisterAddress::from_hex(&addr)
            .map_err(|e| PyValueError::new_err(format!("Failed to parse address: {e}")))?;

        future_into_py(py, async move {
            let data = client
                .register_get(&addr)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to get register: {e}")))?;

            Ok(data)
        })
    }

    /// Get the current value of the register
    fn register_cost<'a>(&self, py: Python<'a>, owner: PyPublicKey) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let cost = client.register_cost(&owner.inner).await.map_err(|e| {
                PyRuntimeError::new_err(format!("Failed to get register cost: {e}"))
            })?;

            Ok(cost.to_string())
        })
    }

    /// Retrieves and returns a decrypted vault if one exists.
    ///
    /// Returns the content type of the bytes in the vault.
    fn fetch_and_decrypt_vault<'a>(
        &self,
        py: Python<'a>,
        key: &PyVaultSecretKey,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();
        let key = key.inner.clone();

        future_into_py(py, async move {
            match client.vault_get(&key).await {
                Ok((data, content_type)) => Ok((data.to_vec(), content_type)),
                Err(e) => Err(PyRuntimeError::new_err(format!(
                    "Failed to fetch vault: {e}"
                ))),
            }
        })
    }

    /// Get the user data from the vault
    fn vault_get_user_data<'a>(
        &self,
        py: Python<'a>,
        key: &PyVaultSecretKey,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();
        let key = key.inner.clone();

        future_into_py(py, async move {
            match client.vault_get_user_data(&key).await {
                Ok(user_data) => Ok(PyUserData { inner: user_data }),
                Err(e) => Err(PyRuntimeError::new_err(format!(
                    "Failed to get user data from vault: {e}"
                ))),
            }
        })
    }

    /// Put the user data to the vault.
    ///
    /// Returns the total cost of the put operation.
    fn vault_put_user_data<'a>(
        &self,
        py: Python<'a>,
        key: &PyVaultSecretKey,
        payment: &PyPaymentOption,
        user_data: &PyUserData,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();
        let key = key.inner.clone();
        let payment = payment.inner.clone();
        let user_data = user_data.inner.clone();

        future_into_py(py, async move {
            match client.vault_put_user_data(&key, payment, user_data).await {
                Ok(cost) => Ok(cost.to_string()),
                Err(e) => Err(PyRuntimeError::new_err(format!(
                    "Failed to put user data: {e}"
                ))),
            }
        })
    }

    /// @deprecated Use `vault_put` instead. This function will be removed in a future version.
    fn write_bytes_to_vault<'a>(
        &self,
        py: Python<'a>,
        data: Vec<u8>,
        payment: &PyPaymentOption,
        key: &PyVaultSecretKey,
        content_type: u64,
    ) -> PyResult<Bound<'a, PyAny>> {
        self.vault_put(py, data, payment, key, content_type)
    }

    /// @deprecated Use `vault_get_user_data` instead. This function will be removed in a future version.
    fn get_user_data_from_vault<'a>(
        &self,
        py: Python<'a>,
        key: &PyVaultSecretKey,
    ) -> PyResult<Bound<'a, PyAny>> {
        self.vault_get_user_data(py, key)
    }

    /// @deprecated Use `vault_put_user_data` instead. This function will be removed in a future version.
    fn put_user_data_to_vault<'a>(
        &self,
        py: Python<'a>,
        key: &PyVaultSecretKey,
        payment: &PyPaymentOption,
        user_data: &PyUserData,
    ) -> PyResult<Bound<'a, PyAny>> {
        self.vault_put_user_data(py, key, payment, user_data)
    }

    /// Get a pointer from the network
    fn pointer_get<'a>(
        &self,
        py: Python<'a>,
        addr: PyPointerAddress,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            match client.pointer_get(&addr.inner).await {
                Ok(pointer) => Ok(PyPointer { inner: pointer }),
                Err(e) => Err(PyRuntimeError::new_err(format!(
                    "Failed to get pointer: {e}"
                ))),
            }
        })
    }

    /// Check if a pointer exists on the network
    fn pointer_check_existance<'a>(
        &self,
        py: Python<'a>,
        addr: PyPointerAddress,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let exists = client
                .pointer_check_existence(&addr.inner)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to get pointer: {e}")))?;

            Ok(exists)
        })
    }

    /// Manually store a pointer on the network
    fn pointer_put<'a>(
        &self,
        py: Python<'a>,
        pointer: &PyPointer,
        payment_option: &PyPaymentOption,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();
        let pointer = pointer.inner.clone();
        let payment = payment_option.inner.clone();

        future_into_py(py, async move {
            let (_cost, addr) = client
                .pointer_put(pointer, payment)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to put pointer: {e}")))?;

            Ok(PyPointerAddress { inner: addr })
        })
    }

    /// Create a new pointer on the network.
    ///
    /// Make sure that the owner key is not already used for another pointer as each key is associated with one pointer
    fn pointer_create<'a>(
        &self,
        py: Python<'a>,
        owner: PySecretKey,
        target: PyPointerTarget,
        payment_option: &PyPaymentOption,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();
        let payment = payment_option.inner.clone();

        future_into_py(py, async move {
            let (cost, addr) = client
                .pointer_create(&owner.inner, target.inner, payment)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to create pointer: {e}")))?;

            Ok((cost.to_string(), PyPointerAddress { inner: addr }))
        })
    }

    /// Update an existing pointer to point to a new target on the network.
    ///
    /// The pointer needs to be created first with `pointer_put`.
    /// This operation is free as the pointer was already paid for at creation.
    /// Only the latest version of the pointer is kept on the Network, previous versions will be overwritten and unrecoverable.
    fn pointer_update<'a>(
        &self,
        py: Python<'a>,
        owner: PySecretKey,
        target: PyPointerTarget,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            client
                .pointer_update(&owner.inner, target.inner)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to update pointer: {e}")))?;

            Ok(())
        })
    }

    /// Update an existing pointer from a specific pointer to point to a new target on the network.
    ///
    /// This will increment the counter of the pointer and update the target.
    /// This function is used internally by `pointer_update` after the pointer has been retrieved from the network.
    /// To skip the retrieval step if you already have the pointer, use this function directly.
    /// This function will return the new pointer after it has been updated.
    fn pointer_update_from<'a>(
        &self,
        py: Python<'a>,
        current: PyPointer,
        owner: PySecretKey,
        target: PyPointerTarget,
    ) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();

        future_into_py(py, async move {
            let new_pointer = client
                .pointer_update_from(&current.inner, &owner.inner, target.inner)
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to update pointer: {e}")))?;

            Ok(PyPointer { inner: new_pointer })
        })
    }

    /// Calculate the cost of storing a pointer
    fn pointer_cost<'a>(&self, py: Python<'a>, key: &PyPublicKey) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();
        let key = key.inner;

        future_into_py(py, async move {
            match client.pointer_cost(&key).await {
                Ok(cost) => Ok(cost.to_string()),
                Err(e) => Err(PyRuntimeError::new_err(format!(
                    "Failed to get pointer cost: {e}"
                ))),
            }
        })
    }

    /// Verify a pointer
    #[staticmethod]
    fn pointer_verify(pointer: &PyPointer) -> PyResult<()> {
        Client::pointer_verify(&pointer.inner)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to verify pointer: {e}")))
    }

    fn get_raw_quotes<'a>(
        &self,
        py: Python<'a>,
        data_type: PyDataTypes,
        content_addrs: Vec<(PyXorName, usize)>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let data_type: DataTypes = data_type.into();
        let client = self.inner.clone();
        let content_addrs_iter = content_addrs
            .into_iter()
            .map(|(xor_name, size)| (xor_name.inner, size));

        future_into_py(py, async move {
            let results = client.get_raw_quotes(data_type, content_addrs_iter).await;

            let py_results: Vec<_> = results
                .into_iter()
                .map(|result| match result {
                    Ok((xor_name, quotes)) => {
                        let py_xor_name = PyXorName { inner: xor_name };

                        let py_quotes: Vec<_> = quotes
                            .into_iter()
                            .map(|(peer_id, addresses, quote)| {
                                let peer_id_str = peer_id.to_string();

                                let addresses_str = addresses
                                    .0
                                    .iter()
                                    .map(|addr| addr.to_string())
                                    .collect::<Vec<_>>()
                                    .join(",");

                                let py_payment_quote = PyPaymentQuote { inner: quote };
                                (peer_id_str, addresses_str, py_payment_quote)
                            })
                            .collect();

                        (py_xor_name, py_quotes)
                    }
                    Err(err) => {
                        error!("Error in get_raw_quotes: {}", err);

                        let empty_xor = PyXorName {
                            inner: XorName::default(),
                        };
                        let empty_quotes: Vec<(String, String, PyPaymentQuote)> = Vec::new();

                        (empty_xor, empty_quotes)
                    }
                })
                .collect();

            Ok(py_results)
        })
    }

    fn get_store_quotes<'a>(
        &self,
        py: Python<'a>,
        data_type: PyDataTypes,
        content_addrs: Vec<(PyXorName, usize)>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let data_type: DataTypes = data_type.into();

        let client = self.inner.clone();

        let content_addrs_iter = content_addrs
            .into_iter()
            .map(|(xor_name, size)| (xor_name.inner, size));

        future_into_py(py, async move {
            match client.get_store_quotes(data_type, content_addrs_iter).await {
                Ok(quotes) => {
                    let py_store_quote = PyStoreQuote { inner: quotes };
                    Ok(py_store_quote)
                }
                Err(err) => Err(PyErr::new::<PyValueError, _>(format!("{err:?}"))),
            }
        })
    }
}

#[pyclass(name = "ClientEvent")]
#[derive(Debug, Clone)]
pub struct PyClientEvent {
    inner: ClientEvent,
}

#[pymethods]
impl PyClientEvent {
    #[getter]
    fn event_type(&self) -> &'static str {
        match self.inner {
            ClientEvent::UploadComplete(_) => "UploadComplete",
        }
    }

    #[getter]
    fn upload_summary(&self) -> Option<PyUploadSummary> {
        match &self.inner {
            ClientEvent::UploadComplete(summary) => Some(PyUploadSummary {
                inner: summary.clone(),
            }),
        }
    }

    fn __str__(&self) -> PyResult<String> {
        Ok(format!("{:?}", self.inner))
    }
}

#[pyclass(name = "ClientEventReceiver")]
pub struct PyClientEventReceiver {
    inner: Arc<futures::lock::Mutex<mpsc::Receiver<ClientEvent>>>,
}

#[pymethods]
impl PyClientEventReceiver {
    /// Receive the next client event, returning None if the channel is closed
    fn recv<'a>(&self, py: Python<'a>) -> PyResult<Bound<'a, PyAny>> {
        let inner = Arc::clone(&self.inner);

        future_into_py(py, async move {
            let mut receiver = inner.lock().await;

            let result = receiver
                .recv()
                .await
                .map(|event| PyClientEvent { inner: event });

            Ok(result)
        })
    }
}

#[pyclass(name = "ClientOperatingStrategy")]
#[derive(Debug, Clone)]
pub struct PyClientOperatingStrategy {
    inner: ClientOperatingStrategy,
}

#[pymethods]
impl PyClientOperatingStrategy {
    /// Create a new ClientOperatingStrategy with default values
    #[new]
    fn new() -> Self {
        Self {
            inner: ClientOperatingStrategy::new(),
        }
    }

    /// Get the strategy for chunk operations
    #[getter]
    fn get_chunks(&self) -> PyStrategy {
        PyStrategy {
            inner: self.inner.chunks.clone(),
        }
    }

    /// Get the strategy for graph entry operations
    #[getter]
    fn get_graph_entry(&self) -> PyStrategy {
        PyStrategy {
            inner: self.inner.graph_entry.clone(),
        }
    }

    /// Get the strategy for pointer operations
    #[getter]
    fn get_pointer(&self) -> PyStrategy {
        PyStrategy {
            inner: self.inner.pointer.clone(),
        }
    }

    /// Get the strategy for scratchpad operations
    #[getter]
    fn get_scratchpad(&self) -> PyStrategy {
        PyStrategy {
            inner: self.inner.scratchpad.clone(),
        }
    }

    /// Return a string representation of the strategy
    fn __str__(&self) -> String {
        format!("{:?}", self.inner)
    }

    /// Get a representation for debugging
    fn __repr__(&self) -> String {
        self.__str__()
    }
}

/// Address of a Pointer, is derived from the owner's unique public key.
#[pyclass(name = "PointerAddress", eq, ord)]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub struct PyPointerAddress {
    inner: PointerAddress,
}

#[pymethods]
impl PyPointerAddress {
    /// Construct a new PointerAddress given an owner.
    #[new]
    fn new(public_key: PyPublicKey) -> PyResult<Self> {
        Ok(Self {
            inner: PointerAddress::new(public_key.inner),
        })
    }

    /// Return the owner public key.
    pub fn owner(&self) -> PyPublicKey {
        PyPublicKey {
            inner: *self.inner.owner(),
        }
    }

    /// Returns the hex string representation of the pointer address.
    #[getter]
    fn hex(&self) -> String {
        self.inner.to_hex()
    }

    /// Create a Pointer address from a hex string.
    #[staticmethod]
    fn from_hex(hex: &str) -> PyResult<Self> {
        Ok(Self {
            inner: PointerAddress::from_hex(hex)
                .map_err(|e| PyValueError::new_err(e.to_string()))?,
        })
    }

    fn __str__(&self) -> PyResult<String> {
        Ok(self.hex())
    }

    fn __repr__(&self) -> PyResult<String> {
        Ok(format!("PointerAddress('{}')", self.hex()))
    }
}

/// Pointer, a mutable address pointing to other data on the Network.
/// It is stored at the owner's public key and can only be updated by the owner.
#[pyclass(name = "Pointer")]
#[derive(Debug, Clone)]
pub struct PyPointer {
    inner: Pointer,
}

#[pymethods]
impl PyPointer {
    /// Create a new pointer, signing it with the provided secret key.
    /// This pointer would be stored on the network at the provided key's public key.
    /// There can only be one pointer at a time at the same address (one per key).
    #[new]
    pub fn new(key: &PySecretKey, counter: u64, target: &PyPointerTarget) -> PyResult<Self> {
        Ok(Self {
            inner: Pointer::new(&key.inner, counter, target.inner.clone()),
        })
    }

    /// Returns the network address where this pointer is stored.
    pub fn address(&self) -> PyPointerAddress {
        PyPointerAddress {
            inner: self.inner.address(),
        }
    }

    /// Returns the target that this pointer points to.
    #[getter]
    fn target(&self) -> PyPointerTarget {
        PyPointerTarget {
            inner: self.inner.target().clone(),
        }
    }

    fn __str__(&self) -> PyResult<String> {
        Ok(format!("Pointer('{}')", self.inner.address().to_hex()))
    }
}

/// The target that a pointer points to on the network.
#[pyclass(name = "PointerTarget")]
#[derive(Debug, Clone)]
pub struct PyPointerTarget {
    inner: PointerTarget,
}

#[pymethods]
impl PyPointerTarget {
    /// Initialize a pointer targeting a chunk.
    #[staticmethod]
    fn new_chunk(addr: PyChunkAddress) -> PyResult<Self> {
        Ok(Self {
            inner: PointerTarget::ChunkAddress(addr.inner),
        })
    }

    /// Initialize a pointer targeting a graph entry.
    #[staticmethod]
    fn new_graph_entry(addr: PyGraphEntryAddress) -> PyResult<Self> {
        Ok(Self {
            inner: PointerTarget::GraphEntryAddress(addr.inner),
        })
    }

    /// Initialize a pointer targeting another pointer.
    #[staticmethod]
    fn new_pointer(addr: PyPointerAddress) -> PyResult<Self> {
        Ok(Self {
            inner: PointerTarget::PointerAddress(addr.inner),
        })
    }

    /// Initialize a pointer targeting a scratchpad.
    #[staticmethod]
    fn new_scratchpad(addr: PyScratchpadAddress) -> PyResult<Self> {
        Ok(Self {
            inner: PointerTarget::ScratchpadAddress(addr.inner),
        })
    }

    #[getter]
    fn target(&self) -> PyPointerTarget {
        PyPointerTarget {
            inner: PointerTarget::ChunkAddress(ChunkAddress::new(self.inner.xorname())),
        }
    }

    /// Returns the hex string representation of the target
    #[getter]
    fn hex(&self) -> String {
        self.inner.to_hex()
    }

    fn __str__(&self) -> PyResult<String> {
        Ok(format!("PointerTarget('{}')", self.hex()))
    }
}

#[pyclass(name = "Chunk", eq, ord)]
#[derive(Hash, Eq, PartialEq, PartialOrd, Ord, Clone, Debug)]
pub struct PyChunk {
    inner: Chunk,
}

#[pymethods]
impl PyChunk {
    /// Creates a new instance of `Chunk`.
    #[new]
    fn new(value: Vec<u8>) -> Self {
        Self {
            inner: Chunk::new(Bytes::from(value)),
        }
    }

    /// Returns the value of the chunk.
    #[getter]
    fn value(&self) -> Vec<u8> {
        self.inner.value().to_vec()
    }

    /// Returns the address of the chunk.
    #[getter]
    fn address(&self) -> PyChunkAddress {
        PyChunkAddress {
            inner: *self.inner.address(),
        }
    }

    /// Returns the network address.
    fn network_address(&self) -> String {
        self.inner.network_address().to_string()
    }

    /// Returns the name of the chunk.
    fn name(&self) -> PyXorName {
        PyXorName {
            inner: *self.inner.name(),
        }
    }

    /// Returns size of this chunk after serialisation.
    fn size(&self) -> usize {
        self.inner.size()
    }

    /// Returns true if the chunk is too big
    fn is_too_big(&self) -> bool {
        self.inner.is_too_big()
    }

    /// The maximum size of an unencrypted/raw chunk (4MB).
    #[classattr]
    fn max_raw_size() -> usize {
        Chunk::MAX_RAW_SIZE
    }

    /// The maximum size of an encrypted chunk (4MB + 32 bytes).
    #[classattr]
    fn max_size() -> usize {
        Chunk::MAX_SIZE
    }

    fn __str__(&self) -> PyResult<String> {
        Ok(format!("Chunk(size={})", self.inner.size()))
    }

    fn __repr__(&self) -> PyResult<String> {
        Ok(format!(
            "Chunk(address='{}', size={})",
            self.inner.address().to_hex(),
            self.inner.size()
        ))
    }
}

/// An address of a chunk of data on the network. Used to locate and retrieve data chunks.
#[pyclass(name = "ChunkAddress", eq, ord)]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub struct PyChunkAddress {
    inner: ChunkAddress,
}

#[pymethods]
impl PyChunkAddress {
    /// Creates a new chunk address from a hex string.
    #[new]
    fn new(addr: PyXorName) -> PyResult<Self> {
        Ok(Self {
            inner: ChunkAddress::new(addr.inner),
        })
    }

    /// Returns the XorName
    pub fn xorname(&self) -> PyXorName {
        PyXorName {
            inner: *self.inner.xorname(),
        }
    }

    /// Generate a chunk address for the given content (for content-addressable-storage).
    #[staticmethod]
    fn from_content(data: Vec<u8>) -> PyResult<Self> {
        Ok(Self {
            inner: ChunkAddress::new(XorName::from_content(&data[..])),
        })
    }

    /// Generate a random chunk address.
    #[staticmethod]
    fn random() -> PyResult<Self> {
        Ok(Self {
            inner: ChunkAddress::new(XorName::random(&mut rand::thread_rng())),
        })
    }

    #[getter]
    fn hex(&self) -> String {
        self.inner.to_hex()
    }

    /// Create a ChunkAddress from a hex string.
    #[staticmethod]
    fn from_hex(hex: &str) -> PyResult<Self> {
        Ok(Self {
            inner: ChunkAddress::from_hex(hex).map_err(|e| PyValueError::new_err(e.to_string()))?,
        })
    }

    fn __str__(&self) -> PyResult<String> {
        Ok(self.hex())
    }

    fn __repr__(&self) -> PyResult<String> {
        Ok(format!("ChunkAddress('{}')", self.hex()))
    }
}

/// Address of a GraphEntry, is derived from the owner's unique public key.
#[pyclass(name = "GraphEntryAddress", eq, ord)]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub struct PyGraphEntryAddress {
    inner: GraphEntryAddress,
}

#[pymethods]
impl PyGraphEntryAddress {
    /// Create graph entry address
    #[staticmethod]
    fn new(public_key: PyPublicKey) -> PyResult<Self> {
        Ok(Self {
            inner: GraphEntryAddress::new(public_key.inner),
        })
    }

    #[getter]
    fn hex(&self) -> String {
        self.inner.to_hex()
    }

    /// Create a graph entry address from a hex string.
    #[staticmethod]
    fn from_hex(hex: &str) -> PyResult<Self> {
        Ok(Self {
            inner: GraphEntryAddress::from_hex(hex)
                .map_err(|e| PyValueError::new_err(e.to_string()))?,
        })
    }

    fn __str__(&self) -> PyResult<String> {
        Ok(self.hex())
    }

    fn __repr__(&self) -> PyResult<String> {
        Ok(format!("GraphEntryAddress('{}')", self.hex()))
    }
}

/// Configuration for the bootstrap cache
#[pyclass(name = "BootstrapCacheConfig")]
#[derive(Debug, Clone)]
pub struct PyBootstrapCacheConfig {
    pub(crate) inner: BootstrapCacheConfig,
}

#[pymethods]
impl PyBootstrapCacheConfig {
    /// Creates a new BootstrapCacheConfig with default settings
    /// When `local` is set to true, a different cache file name is used.
    /// I.e. the file name will include `_local_` in the name.
    #[new]
    fn new(local: bool) -> PyResult<Self> {
        let config = BootstrapCacheConfig::new(local).map_err(|e| {
            PyRuntimeError::new_err(format!("Failed to create default config: {e}"))
        })?;
        Ok(Self { inner: config })
    }

    /// Creates a new BootstrapCacheConfig with empty settings
    #[staticmethod]
    fn empty() -> Self {
        Self {
            inner: BootstrapCacheConfig::empty(),
        }
    }

    /// Set a new addr expiry duration in seconds
    fn with_addr_expiry_duration(&self, seconds: u64) -> Self {
        Self {
            inner: self
                .inner
                .clone()
                .with_addr_expiry_duration(Duration::from_secs(seconds)),
        }
    }

    /// Update the config with a custom cache directory
    fn with_cache_dir(&self, path: PathBuf) -> Self {
        Self {
            inner: self.inner.clone().with_cache_dir(path),
        }
    }

    /// Sets the maximum number of peers
    fn with_max_peers(&self, max_peers: usize) -> Self {
        Self {
            inner: self.inner.clone().with_max_peers(max_peers),
        }
    }

    /// Sets the maximum number of addresses for a single peer
    fn with_addrs_per_peer(&self, max_addrs: usize) -> Self {
        Self {
            inner: self.inner.clone().with_addrs_per_peer(max_addrs),
        }
    }

    /// Sets the flag to disable writing to the cache file
    fn with_disable_cache_writing(&self, disable: bool) -> Self {
        Self {
            inner: self.inner.clone().with_disable_cache_writing(disable),
        }
    }

    /// Get the address expiry duration in seconds
    #[getter]
    fn addr_expiry_duration(&self) -> u64 {
        self.inner.addr_expiry_duration.as_secs()
    }

    /// Get the maximum number of peers
    #[getter]
    fn max_peers(&self) -> usize {
        self.inner.max_peers
    }

    /// Get the maximum number of addresses per peer
    #[getter]
    fn max_addrs_per_peer(&self) -> usize {
        self.inner.max_addrs_per_peer
    }

    /// Get the cache directory
    #[getter]
    fn cache_dir(&self) -> PathBuf {
        self.inner.cache_dir.clone()
    }

    /// Get whether cache writing is disabled
    #[getter]
    fn disable_cache_writing(&self) -> bool {
        self.inner.disable_cache_writing
    }

    /// Get the minimum cache save duration in seconds
    #[getter]
    fn min_cache_save_duration(&self) -> u64 {
        self.inner.min_cache_save_duration.as_secs()
    }

    /// Get the maximum cache save duration in seconds
    #[getter]
    fn max_cache_save_duration(&self) -> u64 {
        self.inner.max_cache_save_duration.as_secs()
    }

    /// Get the cache save scaling factor
    #[getter]
    fn cache_save_scaling_factor(&self) -> u32 {
        self.inner.cache_save_scaling_factor
    }

    /// Return a string representation
    fn __str__(&self) -> String {
        format!("{:?}", self.inner)
    }

    /// Return a debug representation
    fn __repr__(&self) -> String {
        self.__str__()
    }
}

#[pyclass(name = "InitialPeersConfig")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PyInitialPeersConfig {
    inner: InitialPeersConfig,
}

#[pymethods]
impl PyInitialPeersConfig {
    /// Create a new InitialPeersConfig with default values
    #[new]
    fn new() -> Self {
        Self {
            inner: InitialPeersConfig::default(),
        }
    }

    #[getter]
    fn get_first(&self) -> bool {
        self.inner.first
    }

    #[setter]
    fn set_first(&mut self, value: bool) {
        self.inner.first = value;
    }

    /// Addresses to use for bootstrap, in multiaddr format
    #[getter]
    fn get_addrs(&self) -> Vec<String> {
        self.inner
            .addrs
            .iter()
            .map(|addr| addr.to_string())
            .collect()
    }

    #[setter]
    fn set_addrs(&mut self, addrs: Vec<String>) -> PyResult<()> {
        self.inner.addrs = addrs
            .iter()
            .filter_map(|addr| Multiaddr::from_str(addr).ok())
            .collect();
        Ok(())
    }

    /// URLs to fetch network contacts from
    #[getter]
    fn get_network_contacts_url(&self) -> Vec<String> {
        self.inner.network_contacts_url.clone()
    }

    #[setter]
    fn set_network_contacts_url(&mut self, urls: Vec<String>) {
        self.inner.network_contacts_url = urls;
    }

    /// Whether this is a local network
    #[getter]
    fn get_local(&self) -> bool {
        self.inner.local
    }

    #[setter]
    fn set_local(&mut self, value: bool) {
        self.inner.local = value;
    }

    /// Whether to ignore the bootstrap cache
    #[getter]
    fn get_ignore_cache(&self) -> bool {
        self.inner.ignore_cache
    }

    #[setter]
    fn set_ignore_cache(&mut self, value: bool) {
        self.inner.ignore_cache = value;
    }

    /// Directory for bootstrap cache files
    #[getter]
    fn get_bootstrap_cache_dir(&self) -> Option<PathBuf> {
        self.inner.bootstrap_cache_dir.clone()
    }

    #[setter]
    fn set_bootstrap_cache_dir(&mut self, dir: Option<PathBuf>) {
        self.inner.bootstrap_cache_dir = dir;
    }

    /// Get bootstrap addresses
    #[pyo3(signature = (count=None))]
    fn get_bootstrap_addr<'a>(
        &self,
        py: Python<'a>,
        count: Option<usize>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let inner_config = self.inner.clone();

        future_into_py(py, async move {
            match inner_config.get_bootstrap_addr(count).await {
                Ok(addrs) => Ok(addrs
                    .into_iter()
                    .map(|addr| addr.to_string())
                    .collect::<Vec<String>>()),
                Err(e) => Err(PyRuntimeError::new_err(format!(
                    "Failed to get bootstrap addresses: {e}"
                ))),
            }
        })
    }

    /// Read bootstrap addresses from the ANT_PEERS environment variable
    #[staticmethod]
    fn read_bootstrap_addr_from_env() -> Vec<String> {
        InitialPeersConfig::read_bootstrap_addr_from_env()
            .into_iter()
            .map(|addr| addr.to_string())
            .collect()
    }

    /// Return a string representation
    fn __str__(&self) -> String {
        format!("{:?}", self.inner)
    }

    /// Return a representation for debugging
    fn __repr__(&self) -> String {
        self.__str__()
    }
}

#[pyclass(name = "MainPubkey")]
#[derive(Copy, PartialEq, Eq, Ord, PartialOrd, Clone, Serialize, Deserialize, Hash)]
pub struct PyMainPubkey {
    inner: MainPubkey,
}

#[pymethods]
impl PyMainPubkey {
    /// Create a new MainPubkey from a PublicKey
    #[new]
    fn new(public_key: PyPublicKey) -> Self {
        Self {
            inner: MainPubkey::new(public_key.inner),
        }
    }

    /// Verify that the signature is valid for the message
    fn verify(&self, sig: &PySignature, msg: &[u8]) -> bool {
        self.inner.verify(&sig.inner, msg)
    }

    /// Generate a new DerivedPubkey from provided DerivationIndex
    fn derive_key(&self, index: &PyDerivationIndex) -> PyDerivedPubkey {
        PyDerivedPubkey {
            inner: self.inner.derive_key(&index.inner),
        }
    }

    /// Return the inner pubkey's bytes representation
    fn as_bytes(&self) -> [u8; PK_SIZE] {
        self.inner.to_bytes()
    }

    /// Return a hex representation of the MainPubkey
    fn as_hex(&self) -> String {
        self.inner.to_hex()
    }

    /// Create a new MainPubkey from a hex string
    #[staticmethod]
    fn from_hex(hex_str: &str) -> PyResult<Self> {
        MainPubkey::from_hex(hex_str)
            .map(|inner| Self { inner })
            .map_err(|e| PyValueError::new_err(format!("Failed to parse hex: {e}")))
    }

    /// Return string representation (hex format)
    fn __str__(&self) -> String {
        self.inner.to_hex()
    }

    /// Return representation for debugging
    fn __repr__(&self) -> String {
        format!("MainPubkey('{}')", self.inner.to_hex())
    }
}

#[pyclass(name = "MainSecretKey")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PyMainSecretKey {
    inner: MainSecretKey,
}

#[pymethods]
impl PyMainSecretKey {
    /// Create a new MainSecretKey from a SecretKey
    #[new]
    fn new(secret_key: PySecretKey) -> Self {
        Self {
            inner: MainSecretKey::new(secret_key.inner),
        }
    }

    /// Return the matching MainPubkey
    fn public_key(&self) -> PyMainPubkey {
        PyMainPubkey {
            inner: self.inner.public_key(),
        }
    }

    /// Signs the given message
    fn sign(&self, msg: &[u8]) -> PySignature {
        PySignature {
            inner: self.inner.sign(msg),
        }
    }

    /// Derive a DerivedSecretKey from a DerivationIndex
    fn derive_key(&self, index: &PyDerivationIndex) -> PyDerivedSecretKey {
        PyDerivedSecretKey {
            inner: self.inner.derive_key(&index.inner),
        }
    }

    /// Return the inner secret key's bytes representation
    fn to_bytes(&self) -> Vec<u8> {
        self.inner.to_bytes()
    }

    /// Generate a new random MainSecretKey
    #[staticmethod]
    fn random() -> Self {
        Self {
            inner: MainSecretKey::random(),
        }
    }

    /// Generate a new random DerivedSecretKey from the MainSecretKey
    fn random_derived_key(&self) -> PyDerivedSecretKey {
        PyDerivedSecretKey {
            inner: self.inner.random_derived_key(&mut rand::thread_rng()),
        }
    }

    /// Return string representation for debugging
    fn __repr__(&self) -> String {
        format!("MainSecretKey(public_key={})", self.public_key().as_hex())
    }
}
/// Address of a Scratchpad, is derived from the owner's unique public key.
#[pyclass(name = "ScratchpadAddress", eq, ord)]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub struct PyScratchpadAddress {
    inner: ScratchpadAddress,
}

#[pymethods]
impl PyScratchpadAddress {
    /// Construct a new ScratchpadAddress given an owner.
    #[new]
    fn new(public_key: PyPublicKey) -> PyResult<Self> {
        Ok(Self {
            inner: ScratchpadAddress::new(public_key.inner),
        })
    }

    /// Return the owner public key.
    pub fn owner(&self) -> PyPublicKey {
        PyPublicKey {
            inner: *self.inner.owner(),
        }
    }

    /// Returns the hex string representation of the scratchpad address.
    #[getter]
    fn hex(&self) -> String {
        self.inner.to_hex()
    }

    /// Create a scratchpad address from a hex string.
    #[staticmethod]
    fn from_hex(hex: &str) -> PyResult<Self> {
        Ok(Self {
            inner: ScratchpadAddress::from_hex(hex)
                .map_err(|e| PyValueError::new_err(e.to_string()))?,
        })
    }

    fn __str__(&self) -> PyResult<String> {
        Ok(self.hex())
    }

    fn __repr__(&self) -> PyResult<String> {
        Ok(format!("ScratchpadAddress('{}')", self.hex()))
    }
}

/// Address of Data on the Network.
#[pyclass(name = "DataAddress", eq, ord)]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub struct PyDataAddress {
    inner: DataAddress,
}

#[pymethods]
impl PyDataAddress {
    /// Construct a new DataAddress
    #[new]
    fn new(xorname: PyXorName) -> PyResult<Self> {
        Ok(Self {
            inner: DataAddress::new(xorname.inner),
        })
    }

    /// Returns the hex string representation of the data address.
    #[getter]
    fn hex(&self) -> String {
        self.inner.to_hex()
    }

    /// Create a Data address from a hex string.
    #[staticmethod]
    fn from_hex(hex: &str) -> PyResult<Self> {
        Ok(Self {
            inner: DataAddress::from_hex(hex).map_err(|e| PyValueError::new_err(e.to_string()))?,
        })
    }

    fn __str__(&self) -> PyResult<String> {
        Ok(self.hex())
    }

    fn __repr__(&self) -> PyResult<String> {
        Ok(format!("DataAddress('{}')", self.hex()))
    }
}

/// Address of Data on the Network.
#[pyclass(name = "ArchiveAddress", eq, ord)]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub struct PyArchiveAddress {
    inner: ArchiveAddress,
}

#[pymethods]
impl PyArchiveAddress {
    /// Construct a new ArchiveAddress, the address of a public archive on the network.
    #[new]
    fn new(xorname: PyXorName) -> PyResult<Self> {
        Ok(Self {
            inner: ArchiveAddress::new(xorname.inner),
        })
    }

    /// Returns the hex string representation of this archive address.
    #[getter]
    fn hex(&self) -> String {
        self.inner.to_hex()
    }

    /// Create an ArchiveAddress from a hex string.
    #[staticmethod]
    fn from_hex(hex: &str) -> PyResult<Self> {
        Ok(Self {
            inner: ArchiveAddress::from_hex(hex)
                .map_err(|e| PyValueError::new_err(e.to_string()))?,
        })
    }

    fn __str__(&self) -> PyResult<String> {
        Ok(self.hex())
    }

    fn __repr__(&self) -> PyResult<String> {
        Ok(format!("ArchiveAddress('{}')", self.hex()))
    }
}

/// Address of Data on the Network.
#[pyclass(name = "PrivateArchiveDataMap", eq, ord)]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct PyPrivateArchiveDataMap {
    inner: PrivateArchiveDataMap,
}

#[pymethods]
impl PyPrivateArchiveDataMap {
    /// Returns the hex string representation of this private archive datamap.
    #[getter]
    fn hex(&self) -> String {
        self.inner.to_hex()
    }

    /// Create a PrivateArchiveDataMap from a hex string.
    #[staticmethod]
    fn from_hex(hex: &str) -> PyResult<Self> {
        Ok(Self {
            inner: PrivateArchiveDataMap::from_hex(hex)
                .map_err(|e| PyValueError::new_err(e.to_string()))?,
        })
    }

    fn __str__(&self) -> PyResult<String> {
        Ok(self.hex())
    }

    fn __repr__(&self) -> PyResult<String> {
        Ok(format!("PrivateArchiveDataMap('{}')", self.hex()))
    }
}

/// A wallet for interacting with the network's payment system.
/// Handles token transfers, balance checks, and payments for network operations.
#[pyclass(name = "Wallet")]
#[derive(Clone)]
pub struct PyWallet {
    pub(crate) inner: Wallet,
}

#[pymethods]
impl PyWallet {
    /// Creates a new wallet from a private key string.
    /// The wallet will be configured to use the ArbitrumOne network.
    #[new]
    fn new(private_key: String) -> PyResult<Self> {
        let wallet = Wallet::new_from_private_key(
            EVMNetwork::ArbitrumOne, // TODO: Make this configurable
            &private_key,
        )
        .map_err(|e| PyValueError::new_err(format!("`private_key` invalid: {e}")))?;

        Ok(Self { inner: wallet })
    }

    /// Convenience function that creates a new Wallet with a random EthereumWallet.
    #[staticmethod]
    fn new_with_random_wallet(network: PyEVMNetwork) -> Self {
        Self {
            inner: Wallet::new_with_random_wallet(network.inner),
        }
    }

    /// Creates a new wallet from a private key string with a specified network.
    #[staticmethod]
    fn new_from_private_key(network: PyEVMNetwork, private_key: &str) -> PyResult<Self> {
        let inner = Wallet::new_from_private_key(network.inner, private_key)
            .map_err(|e| PyValueError::new_err(format!("`private_key` invalid: {e}")))?;

        Ok(Self { inner })
    }

    /// Returns a string representation of the wallet's address.
    fn address(&self) -> String {
        self.inner.address().to_string()
    }

    /// Returns the `Network` of this wallet.
    fn network(&self) -> PyEVMNetwork {
        PyEVMNetwork {
            inner: self.inner.network().clone(),
        }
    }

    /// Returns the raw balance of payment tokens in the wallet.
    fn balance<'a>(&self, py: Python<'a>) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            match client.balance_of_tokens().await {
                Ok(balance) => Ok(balance.to_string()),
                Err(e) => Err(PyRuntimeError::new_err(format!(
                    "Failed to get balance: {e}"
                ))),
            }
        })
    }

    /// Returns the current balance of gas tokens in the wallet.
    fn balance_of_gas<'a>(&self, py: Python<'a>) -> PyResult<Bound<'a, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            match client.balance_of_gas_tokens().await {
                Ok(balance) => Ok(balance.to_string()),
                Err(e) => Err(PyRuntimeError::new_err(format!(
                    "Failed to get balance: {e}"
                ))),
            }
        })
    }

    /// Returns a random private key string.
    #[staticmethod]
    pub fn random_private_key() -> String {
        Wallet::random_private_key()
    }

    /// Sets the transaction configuration for the wallet.
    fn set_transaction_config(&mut self, config: PyTransactionConfig) -> PyResult<()> {
        self.inner.set_transaction_config(config.into());
        Ok(())
    }
}

#[pyclass(name = "TransactionConfig")]
#[derive(Clone, Debug)]
pub struct PyTransactionConfig {
    pub max_fee_per_gas: PyMaxFeePerGas,
}

#[pymethods]
impl PyTransactionConfig {
    /// Create a TransactionConfig with a specific MaxFeePerGas setting
    #[new]
    fn new(max_fee_per_gas: PyMaxFeePerGas) -> Self {
        Self { max_fee_per_gas }
    }

    /// Get the current MaxFeePerGas setting
    #[getter]
    fn max_fee_per_gas(&self) -> PyMaxFeePerGas {
        self.max_fee_per_gas.clone()
    }

    fn __str__(&self) -> String {
        format!(
            "{:?}",
            std::convert::Into::<TransactionConfig>::into(self.clone())
        )
    }
}

#[allow(clippy::from_over_into)]
impl Into<TransactionConfig> for PyTransactionConfig {
    fn into(self) -> TransactionConfig {
        let max_fee_per_gas = match self.max_fee_per_gas {
            PyMaxFeePerGas::Auto() => MaxFeePerGas::Auto,
            PyMaxFeePerGas::LimitedAuto(limit) => MaxFeePerGas::LimitedAuto(limit),
            PyMaxFeePerGas::Unlimited() => MaxFeePerGas::Unlimited,
            PyMaxFeePerGas::Custom(limit) => MaxFeePerGas::Custom(limit),
        };

        TransactionConfig { max_fee_per_gas }
    }
}

#[pyclass(name = "MaxFeePerGas")]
#[derive(Clone, Debug)]
pub enum PyMaxFeePerGas {
    Auto(),
    LimitedAuto(u128),
    Unlimited(),
    Custom(u128),
}

#[pymethods]
impl PyMaxFeePerGas {
    /// Use the current market price for fee per gas. WARNING: This can result in unexpected high gas fees!
    #[staticmethod]
    pub fn auto() -> Self {
        Self::Auto()
    }

    /// Use the current market price for fee per gas, but with an upper limit.
    #[staticmethod]
    pub fn limited_auto(value: u128) -> Self {
        Self::LimitedAuto(value)
    }

    /// Use no max fee per gas. WARNING: This can result in unexpected high gas fees!
    #[staticmethod]
    pub fn unlimited() -> Self {
        Self::Unlimited()
    }

    /// Use a custom max fee per gas in WEI.
    #[staticmethod]
    pub fn custom(value: u128) -> Self {
        Self::Custom(value)
    }

    pub fn __str__(&self) -> String {
        match self {
            Self::Auto() => "Auto".to_string(),
            Self::LimitedAuto(val) => format!("LimitedAuto({val})"),
            Self::Unlimited() => "Unlimited".to_string(),
            Self::Custom(val) => format!("Custom({val})"),
        }
    }
}

/// Options for making payments on the network.
#[pyclass(name = "PaymentOption")]
#[derive(Clone)]
pub struct PyPaymentOption {
    pub(crate) inner: PaymentOption,
}

#[pymethods]
impl PyPaymentOption {
    /// Creates a payment option using the provided wallet.
    #[staticmethod]
    fn wallet(wallet: &PyWallet) -> Self {
        Self {
            inner: PaymentOption::Wallet(wallet.inner.clone()),
        }
    }
}

/// A cryptographic secret key used for signing operations.
/// Can be used to derive a public key and perform cryptographic operations.
#[pyclass(name = "SecretKey")]
#[derive(Debug, Clone)]
pub struct PySecretKey {
    inner: SecretKey,
}

#[pymethods]
impl PySecretKey {
    /// Creates a new random secret key.
    #[new]
    fn new() -> PyResult<Self> {
        Ok(Self {
            inner: SecretKey::random(),
        })
    }

    /// Creates a secret key from a hex string representation.
    #[staticmethod]
    fn from_hex(hex_str: &str) -> PyResult<Self> {
        SecretKey::from_hex(hex_str)
            .map(|key| Self { inner: key })
            .map_err(|e| PyValueError::new_err(format!("Invalid hex key: {e}")))
    }

    /// Derives and returns the corresponding public key.
    fn public_key(&self) -> PyPublicKey {
        PyPublicKey {
            inner: self.inner.public_key(),
        }
    }

    /// Returns the hex string representation of the key.
    fn hex(&self) -> String {
        self.inner.to_hex()
    }
}

#[pyclass(name = "Signature")]
#[derive(Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct PySignature {
    pub(crate) inner: Signature,
}

#[pymethods]
impl PySignature {
    /// Returns `true` if the signature contains an odd number of ones.
    fn parity(&self) -> bool {
        self.inner.parity()
    }

    /// Returns the signature with the given representation, if valid.
    #[staticmethod]
    fn from_bytes(bytes: [u8; bls::SIG_SIZE]) -> PyResult<Self> {
        Signature::from_bytes(bytes)
            .map(|inner| Self { inner })
            .map_err(|e| PyValueError::new_err(format!("Invalid signature: {e}")))
    }

    /// Returns a byte array representation of the signature.
    fn to_bytes(&self) -> [u8; bls::SIG_SIZE] {
        self.inner.to_bytes()
    }

    /// String representation
    fn __str__(&self) -> String {
        hex::encode(self.inner.to_bytes())
    }

    /// Debug representation
    fn __repr__(&self) -> String {
        format!("Signature('{}')", hex::encode(self.inner.to_bytes()))
    }
}

#[pyclass(name = "StoreQuote")]
pub struct PyStoreQuote {
    inner: StoreQuote,
}

#[pymethods]
impl PyStoreQuote {
    /// Returns the total price of all quotes
    pub fn price(&self) -> String {
        self.inner.price().to_string()
    }

    /// Returns the number of quotes
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns true if there are no quotes
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Returns a list of payment details (hash, rewards_address, price)
    pub fn payments(&self) -> Vec<(String, String, String)> {
        self.inner
            .payments()
            .into_iter()
            .map(|(hash, rewards_address, price)| {
                (
                    format!("0x{}", hex::encode(hash.0)),
                    format!("0x{}", hex::encode(rewards_address.0)),
                    price.to_string(),
                )
            })
            .collect()
    }
}

#[pyclass(name = "Backoff")]
#[derive(Debug, Clone)]
pub struct PyBackoff {
    inner: Backoff,
}

#[pymethods]
impl PyBackoff {
    /// String representation
    fn __str__(&self) -> String {
        format!("{:?}", self.inner)
    }

    /// Representation for debugging
    fn __repr__(&self) -> String {
        self.__str__()
    }
}

// Wrapper approach for RetryStrategy
#[pyclass(name = "RetryStrategy")]
#[derive(Clone, Debug, Copy, Default)]
pub struct PyRetryStrategy {
    inner: RetryStrategy,
}

#[pymethods]
impl PyRetryStrategy {
    /// Create a new RetryStrategy with 'None' setting (1 attempt, no retries)
    #[staticmethod]
    fn none() -> Self {
        Self {
            inner: RetryStrategy::None,
        }
    }

    /// Create a new RetryStrategy with 'Quick' setting (4 attempts)
    #[staticmethod]
    fn quick() -> Self {
        Self {
            inner: RetryStrategy::Quick,
        }
    }

    /// Create a new RetryStrategy with 'Balanced' setting (6 attempts)
    #[staticmethod]
    fn balanced() -> Self {
        Self {
            inner: RetryStrategy::Balanced,
        }
    }

    /// Create a new RetryStrategy with 'Persistent' setting (10 attempts)
    #[staticmethod]
    fn persistent() -> Self {
        Self {
            inner: RetryStrategy::Persistent,
        }
    }

    /// Get the default RetryStrategy (Balanced)
    #[staticmethod]
    fn default() -> Self {
        Self {
            inner: RetryStrategy::default(),
        }
    }

    /// Get the number of retry attempts
    fn attempts(&self) -> usize {
        self.inner.attempts()
    }

    /// Get a Backoff object configured for this retry strategy
    fn backoff(&self) -> PyBackoff {
        PyBackoff {
            inner: self.inner.backoff(),
        }
    }

    /// Get a string representation of the retry strategy
    fn __str__(&self) -> String {
        format!("{:?}", self.inner)
    }

    /// Get a representation for debugging
    fn __repr__(&self) -> String {
        self.__str__()
    }
}

#[pyclass(name = "Quorum")]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PyQuorum {
    inner: Quorum,
}

#[pymethods]
impl PyQuorum {
    /// Get a string representation of the quorum
    fn __str__(&self) -> String {
        match self.inner {
            Quorum::One => "Quorum::One".to_string(),
            Quorum::Majority => "Quorum::Majority".to_string(),
            Quorum::All => "Quorum::All".to_string(),
            Quorum::N(n) => format!("Quorum::N({n})"),
        }
    }

    /// Get a representation for debugging
    fn __repr__(&self) -> String {
        self.__str__()
    }
}

#[pyclass(name = "Strategy")]
#[derive(Debug, Clone)]
pub struct PyStrategy {
    inner: Strategy,
}

#[pymethods]
impl PyStrategy {
    /// Get the quorum for put operations
    #[getter]
    fn get_put_quorum(&self) -> PyQuorum {
        PyQuorum {
            inner: self.inner.put_quorum,
        }
    }

    /// Get the retry strategy for put operations
    #[getter]
    fn get_put_retry(&self) -> PyRetryStrategy {
        PyRetryStrategy {
            inner: self.inner.put_retry,
        }
    }

    /// Get the quorum for verification operations
    #[getter]
    fn get_verification_quorum(&self) -> PyQuorum {
        PyQuorum {
            inner: self.inner.verification_quorum,
        }
    }

    /// Get the quorum for get operations
    #[getter]
    fn get_get_quorum(&self) -> PyQuorum {
        PyQuorum {
            inner: self.inner.get_quorum,
        }
    }

    /// Get the retry strategy for get operations
    #[getter]
    fn get_get_retry(&self) -> PyRetryStrategy {
        PyRetryStrategy {
            inner: self.inner.get_retry,
        }
    }

    /// Return a string representation of the strategy
    fn __str__(&self) -> String {
        format!("{:?}", self.inner)
    }

    /// Get a representation for debugging
    fn __repr__(&self) -> String {
        self.__str__()
    }
}

#[pyclass(name = "UploadSummary")]
#[derive(Debug, Clone)]
pub struct PyUploadSummary {
    inner: UploadSummary,
}

#[pymethods]
impl PyUploadSummary {
    #[getter]
    fn records_paid(&self) -> usize {
        self.inner.records_paid
    }

    #[getter]
    fn records_already_paid(&self) -> usize {
        self.inner.records_already_paid
    }

    #[getter]
    fn tokens_spent(&self) -> String {
        self.inner.tokens_spent.to_string()
    }

    fn __str__(&self) -> PyResult<String> {
        Ok(format!(
            "UploadSummary {{ records_paid: {}, records_already_paid: {}, tokens_spent: {} }}",
            self.inner.records_paid, self.inner.records_already_paid, self.inner.tokens_spent
        ))
    }
}

/// A cryptographic public key derived from a secret key.
#[pyclass(name = "PublicKey")]
#[derive(Debug, Clone)]
pub struct PyPublicKey {
    inner: PublicKey,
}

#[pymethods]
impl PyPublicKey {
    /// Creates a random public key by generating a random secret key.
    #[staticmethod]
    fn random() -> PyResult<Self> {
        let secret = SecretKey::random();
        Ok(Self {
            inner: secret.public_key(),
        })
    }

    /// Creates a public key from a hex string representation.
    #[staticmethod]
    fn from_hex(hex_str: &str) -> PyResult<Self> {
        PublicKey::from_hex(hex_str)
            .map(|key| Self { inner: key })
            .map_err(|e| PyValueError::new_err(format!("Invalid hex key: {e}")))
    }

    /// Returns the hex string representation of the public key.
    fn hex(&self) -> String {
        self.inner.to_hex()
    }
}

#[pyclass(name = "QuoteForAddress")]
pub struct PyQuoteForAddress {
    inner: QuoteForAddress,
}

#[pymethods]
impl PyQuoteForAddress {
    pub fn price(&self) -> String {
        self.inner.price().to_string()
    }
}

#[pyclass(name = "QuotingMetrics")]
#[derive(Clone, Eq, PartialEq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PyQuotingMetrics {
    inner: QuotingMetrics,
}

#[pymethods]
impl PyQuotingMetrics {
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (data_type, data_size, close_records_stored, records_per_type, max_records, received_payment_count, live_time, network_density=None, network_size=None))]
    fn new(
        data_type: u32,
        data_size: usize,
        close_records_stored: usize,
        records_per_type: Vec<(u32, u32)>,
        max_records: usize,
        received_payment_count: usize,
        live_time: u64,
        network_density: Option<Vec<u8>>,
        network_size: Option<u64>,
    ) -> PyResult<Self> {
        // Convert network_density from Option<Vec<u8>> to Option<[u8; 32]>
        let network_density = if let Some(density) = network_density {
            if density.len() != 32 {
                return Err(PyValueError::new_err(
                    "network_density must be 32 bytes if provided",
                ));
            }
            let mut array = [0u8; 32];
            array.copy_from_slice(&density);
            Some(array)
        } else {
            None
        };

        Ok(Self {
            inner: QuotingMetrics {
                data_type,
                data_size,
                close_records_stored,
                records_per_type,
                max_records,
                received_payment_count,
                live_time,
                network_density,
                network_size,
            },
        })
    }

    // Getters
    #[getter]
    fn data_type(&self) -> u32 {
        self.inner.data_type
    }

    #[setter]
    fn set_data_type(&mut self, value: u32) {
        self.inner.data_type = value;
    }

    #[getter]
    fn data_size(&self) -> usize {
        self.inner.data_size
    }

    #[setter]
    fn set_data_size(&mut self, value: usize) {
        self.inner.data_size = value;
    }

    #[getter]
    fn close_records_stored(&self) -> usize {
        self.inner.close_records_stored
    }

    #[setter]
    fn set_close_records_stored(&mut self, value: usize) {
        self.inner.close_records_stored = value;
    }

    #[getter]
    fn records_per_type(&self) -> Vec<(u32, u32)> {
        self.inner.records_per_type.clone()
    }

    #[setter]
    fn set_records_per_type(&mut self, value: Vec<(u32, u32)>) {
        self.inner.records_per_type = value;
    }

    #[getter]
    fn max_records(&self) -> usize {
        self.inner.max_records
    }

    #[setter]
    fn set_max_records(&mut self, value: usize) {
        self.inner.max_records = value;
    }

    #[getter]
    fn received_payment_count(&self) -> usize {
        self.inner.received_payment_count
    }

    #[setter]
    fn set_received_payment_count(&mut self, value: usize) {
        self.inner.received_payment_count = value;
    }

    #[getter]
    fn live_time(&self) -> u64 {
        self.inner.live_time
    }

    #[setter]
    fn set_live_time(&mut self, value: u64) {
        self.inner.live_time = value;
    }

    #[getter]
    fn network_density(&self) -> Option<Vec<u8>> {
        self.inner.network_density.map(|array| array.to_vec())
    }

    #[setter]
    fn set_network_density(&mut self, value: Option<Vec<u8>>) -> PyResult<()> {
        self.inner.network_density = if let Some(density) = value {
            if density.len() != 32 {
                return Err(PyValueError::new_err(
                    "network_density must be 32 bytes if provided",
                ));
            }
            let mut array = [0u8; 32];
            array.copy_from_slice(&density);
            Some(array)
        } else {
            None
        };
        Ok(())
    }

    #[getter]
    fn network_size(&self) -> Option<u64> {
        self.inner.network_size
    }

    #[setter]
    fn set_network_size(&mut self, value: Option<u64>) {
        self.inner.network_size = value;
    }

    fn __str__(&self) -> String {
        format!("{:?}", self.inner)
    }

    fn __repr__(&self) -> String {
        self.__str__()
    }
}

#[pyclass(name = "PaymentQuote")]
#[derive(Clone)]
pub struct PyPaymentQuote {
    inner: PaymentQuote,
}

#[pymethods]
impl PyPaymentQuote {
    /// Creates a new PaymentQuote with the provided values
    #[new]
    #[pyo3(signature = (content, timestamp, quoting_metrics, rewards_address, pub_key, signature))]
    fn new(
        content: PyXorName,
        timestamp: u64, // seconds since UNIX_EPOCH
        quoting_metrics: PyQuotingMetrics,
        rewards_address: String, // hex string with optional 0x prefix
        pub_key: Vec<u8>,
        signature: Vec<u8>,
    ) -> PyResult<Self> {
        // Convert timestamp from u64 to SystemTime
        let timestamp = SystemTime::UNIX_EPOCH + Duration::from_secs(timestamp);

        // Convert rewards address from hex string to RewardsAddress
        let rewards_address = RewardsAddress::from_slice(
            &hex::decode(rewards_address.trim_start_matches("0x"))
                .map_err(|e| PyValueError::new_err(format!("Invalid rewards address: {e}")))?,
        );

        Ok(Self {
            inner: PaymentQuote {
                content: content.inner,
                timestamp,
                quoting_metrics: quoting_metrics.inner,
                rewards_address,
                pub_key,
                signature,
            },
        })
    }

    /// Returns the hash of the quote
    fn hash(&self) -> String {
        format!("0x{}", hex::encode(self.inner.hash().0))
    }

    /// Returns the bytes that would be signed for the given parameters
    #[staticmethod]
    fn bytes_for_signing(
        xorname: PyXorName,
        timestamp: u64,
        quoting_metrics: PyQuotingMetrics,
        rewards_address: String,
    ) -> PyResult<Vec<u8>> {
        // Convert timestamp from u64 to SystemTime
        let timestamp = SystemTime::UNIX_EPOCH + Duration::from_secs(timestamp);

        // Convert rewards address from hex string to RewardsAddress
        let rewards_address = RewardsAddress::from_slice(
            &hex::decode(rewards_address.trim_start_matches("0x"))
                .map_err(|e| PyValueError::new_err(format!("Invalid rewards address: {e}")))?,
        );

        Ok(PaymentQuote::bytes_for_signing(
            xorname.inner,
            timestamp,
            &quoting_metrics.inner,
            &rewards_address,
        ))
    }

    /// Returns the bytes to be signed from self
    fn bytes_for_sig(&self) -> Vec<u8> {
        self.inner.bytes_for_sig()
    }

    /// Returns the peer id of the node that created the quote
    fn peer_id(&self) -> PyResult<String> {
        match self.inner.peer_id() {
            Ok(peer_id) => Ok(peer_id.to_string()),
            Err(e) => Err(PyRuntimeError::new_err(format!(
                "Failed to get peer id: {e}"
            ))),
        }
    }

    /// Check if self is signed by the claimed peer
    fn check_is_signed_by_claimed_peer(&self, claimed_peer: String) -> PyResult<bool> {
        let peer_id = match PeerId::from_str(&claimed_peer) {
            Ok(id) => id,
            Err(e) => return Err(PyValueError::new_err(format!("Invalid peer ID: {e}"))),
        };

        Ok(self.inner.check_is_signed_by_claimed_peer(peer_id))
    }

    /// Check whether self is newer than the target quote
    fn is_newer_than(&self, other: &PyPaymentQuote) -> bool {
        self.inner.is_newer_than(&other.inner)
    }

    /// Check against a new quote, verify whether it is a valid one from self perspective
    fn historical_verify(&self, other: &PyPaymentQuote) -> bool {
        self.inner.historical_verify(&other.inner)
    }

    /// Returns the content of the quote
    #[getter]
    fn content(&self) -> PyXorName {
        PyXorName {
            inner: self.inner.content,
        }
    }

    /// Returns the timestamp of the quote as seconds since UNIX epoch
    #[getter]
    fn timestamp(&self) -> PyResult<u64> {
        self.inner
            .timestamp
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to get timestamp: {e}")))
    }

    /// Returns the quoting metrics
    #[getter]
    fn quoting_metrics(&self) -> PyQuotingMetrics {
        PyQuotingMetrics {
            inner: self.inner.quoting_metrics.clone(),
        }
    }

    /// Returns the rewards address as a hex string
    #[getter]
    fn rewards_address(&self) -> String {
        format!("0x{}", hex::encode(self.inner.rewards_address.as_slice()))
    }

    /// Returns the public key as bytes
    #[getter]
    fn pub_key(&self) -> Vec<u8> {
        self.inner.pub_key.clone()
    }

    /// Returns the signature as bytes
    #[getter]
    fn signature(&self) -> Vec<u8> {
        self.inner.signature.clone()
    }

    /// Returns the string representation
    fn __str__(&self) -> String {
        format!(
            "PaymentQuote(content={}, timestamp={})",
            self.inner.content,
            self.inner
                .timestamp
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        )
    }

    /// Returns the debug representation
    fn __repr__(&self) -> String {
        self.__str__()
    }
}

/// Contains the proof of payments for each XorName and the amount paid
#[pyclass(name = "Receipt")]
#[derive(Clone)]
pub struct PyReceipt {
    pub(crate) inner: Receipt,
}

#[pymethods]
impl PyReceipt {
    /// Creates a new empty receipt
    #[new]
    fn new() -> Self {
        Self {
            inner: Receipt::new(),
        }
    }

    /// Returns the number of entries in the receipt
    fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns true if the receipt has no entries
    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

#[pyclass(name = "RegisterAddress")]
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Debug)]
pub struct PyRegisterAddress {
    inner: RegisterAddress,
}

#[pymethods]
impl PyRegisterAddress {
    /// Create a new register address from a PublicKey
    #[new]
    fn new(owner: PyPublicKey) -> Self {
        Self {
            inner: RegisterAddress::new(owner.inner),
        }
    }

    /// Get the owner of the register
    fn owner(&self) -> PyPublicKey {
        PyPublicKey {
            inner: self.inner.owner(),
        }
    }

    /// Convert to underlying graph representation
    fn as_underlying_graph_root(&self) -> PyGraphEntryAddress {
        PyGraphEntryAddress {
            inner: self.inner.to_underlying_graph_root(),
        }
    }

    /// Convert to underlying head pointer
    fn as_underlying_head_pointer(&self) -> PyPointerAddress {
        PyPointerAddress {
            inner: self.inner.to_underlying_head_pointer(),
        }
    }

    /// Convert a register address to a hex string
    fn as_hex(&self) -> String {
        self.inner.to_hex()
    }

    /// Convert a hex string to a register address
    #[staticmethod]
    fn from_hex(hex: &str) -> PyResult<Self> {
        RegisterAddress::from_hex(hex)
            .map(|addr| Self { inner: addr })
            .map_err(|e| PyValueError::new_err(format!("Failed to parse hex: {e}")))
    }

    /// String representation (returns hex format)
    fn __str__(&self) -> String {
        self.inner.to_string()
    }

    /// Representation for debugging
    fn __repr__(&self) -> String {
        format!("RegisterAddress('{}')", self.inner.to_hex())
    }
}

/// A secret key used to encrypt and decrypt vault data.
#[pyclass(name = "VaultSecretKey")]
#[derive(Debug, Clone)]
pub struct PyVaultSecretKey {
    inner: VaultSecretKey,
}

#[pymethods]
impl PyVaultSecretKey {
    /// Creates a new random vault secret key.
    #[new]
    fn new() -> PyResult<Self> {
        Ok(Self {
            inner: VaultSecretKey::random(),
        })
    }

    #[staticmethod]
    fn from_hex(hex_str: &str) -> PyResult<Self> {
        VaultSecretKey::from_hex(hex_str)
            .map(|key| Self { inner: key })
            .map_err(|e| PyValueError::new_err(format!("Invalid hex key: {e}")))
    }

    /// Returns the hex string representation of the vault secret key.
    fn hex(&self) -> String {
        self.inner.to_hex()
    }
}

/// UserData is stored in Vaults and contains most of a user's private data:
/// It allows users to keep track of only the key to their User Data Vault
/// while having the rest kept on the Network encrypted in a Vault for them
/// Using User Data Vault is optional, one can decide to keep all their data locally instead.
#[pyclass(name = "UserData")]
#[derive(Debug, Clone)]
pub struct PyUserData {
    inner: UserData,
}

#[pymethods]
impl PyUserData {
    /// Creates a new empty UserData instance.
    #[new]
    fn new() -> Self {
        Self {
            inner: UserData::new(),
        }
    }

    /// Returns a list of public file archives as (address, name) pairs.
    fn file_archives(&self) -> Vec<(String, String)> {
        self.inner
            .file_archives
            .iter()
            .map(|(addr, name)| (addr.to_hex(), name.clone()))
            .collect()
    }

    /// Returns a list of private file archives as (data_map, name) pairs.
    fn private_file_archives(&self) -> Vec<(String, String)> {
        self.inner
            .private_file_archives
            .iter()
            .map(|(addr, name)| (addr.to_hex(), name.clone()))
            .collect()
    }
}

/// A map with encrypted data pieces on the network. Used to locate and reconstruct private data.
#[pyclass(name = "DataMapChunk")]
#[derive(Debug, Clone)]
pub struct PyDataMapChunk {
    inner: DataMapChunk,
}

#[pymethods]
impl PyDataMapChunk {
    /// Creates a DataMapChunk from a hex string representation.
    #[staticmethod]
    fn from_hex(hex: &str) -> PyResult<Self> {
        DataMapChunk::from_hex(hex)
            .map(|access| Self { inner: access })
            .map_err(|e| PyValueError::new_err(format!("Invalid hex: {e}")))
    }

    /// Returns the hex string representation of this DataMapChunk.
    fn hex(&self) -> String {
        self.inner.to_hex()
    }

    /// Returns the private address of this DataMapChunk.
    ///
    /// Note that this is not a network address, it is only used for refering to private data client side.
    fn address(&self) -> String {
        self.inner.address().to_string()
    }
}

#[pyfunction]
fn encrypt(data: Vec<u8>) -> PyResult<(Vec<u8>, Vec<Vec<u8>>)> {
    let (data_map, chunks) = self_encryption::encrypt(Bytes::from(data))
        .map_err(|e| PyRuntimeError::new_err(format!("Encryption failed: {e}")))?;

    let data_map_bytes = rmp_serde::to_vec(&data_map)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to serialize datamap: {e}")))?;

    let chunks_bytes: Vec<Vec<u8>> = chunks
        .into_iter()
        .map(|chunk| chunk.content.to_vec())
        .collect();

    Ok((data_map_bytes, chunks_bytes))
}

#[pyclass(name = "DerivationIndex")]
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, Hash)]
pub struct PyDerivationIndex {
    inner: DerivationIndex,
}

#[pymethods]
impl PyDerivationIndex {
    /// Generates a random derivation index
    #[staticmethod]
    fn random() -> Self {
        let mut rng = rand::thread_rng();
        Self {
            inner: DerivationIndex::random(&mut rng),
        }
    }

    /// Returns the inner bytes representation
    fn as_bytes(&self) -> [u8; 32] {
        *self.inner.as_bytes()
    }

    /// Returns the inner bytes
    fn bytes_owned(&self) -> [u8; 32] {
        self.inner.into_bytes()
    }

    /// Creates a new DerivationIndex from a bytes array
    #[staticmethod]
    fn from_bytes(bytes: [u8; 32]) -> Self {
        Self {
            inner: DerivationIndex::from_bytes(bytes),
        }
    }

    /// Returns a string representation of the derivation index
    fn __str__(&self) -> String {
        format!(
            "{:02x}{:02x}{:02x}..",
            self.inner.as_bytes()[0],
            self.inner.as_bytes()[1],
            self.inner.as_bytes()[2]
        )
    }

    /// Returns a debug representation of the derivation index
    fn __repr__(&self) -> String {
        format!(
            "DerivationIndex({:02x}{:02x}{:02x}..)",
            self.inner.as_bytes()[0],
            self.inner.as_bytes()[1],
            self.inner.as_bytes()[2]
        )
    }
}

#[pyclass(name = "DerivedPubkey")]
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PyDerivedPubkey {
    inner: DerivedPubkey,
}

#[pymethods]
impl PyDerivedPubkey {
    /// Create a new DerivedPubkey from a PublicKey
    #[new]
    fn new(public_key: PyPublicKey) -> Self {
        Self {
            inner: DerivedPubkey::new(public_key.inner),
        }
    }

    /// Convert to bytes representation
    fn as_bytes(&self) -> [u8; bls::PK_SIZE] {
        self.inner.to_bytes()
    }

    /// Verify a signature against a message
    fn verify(&self, sig: &PySignature, msg: &[u8]) -> bool {
        self.inner.verify(&sig.inner, msg)
    }

    /// Convert to hex string representation
    fn as_hex(&self) -> String {
        self.inner.to_hex()
    }

    /// Create a DerivedPubkey from a hex string
    #[staticmethod]
    fn from_hex(hex_str: &str) -> PyResult<Self> {
        DerivedPubkey::from_hex(hex_str)
            .map(|inner| Self { inner })
            .map_err(|e| PyValueError::new_err(format!("Failed to parse hex: {e}")))
    }

    /// Return string representation (hex format)
    fn __str__(&self) -> String {
        self.inner.to_hex()
    }

    /// Return representation for debugging
    fn __repr__(&self) -> String {
        format!("DerivedPubkey('{}')", self.inner.to_hex())
    }
}

#[pyclass(name = "DerivedSecretKey")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PyDerivedSecretKey {
    inner: DerivedSecretKey,
}

#[pymethods]
impl PyDerivedSecretKey {
    /// Create a new DerivedSecretKey from a SecretKey
    #[new]
    fn new(secret_key: PySecretKey) -> Self {
        Self {
            inner: DerivedSecretKey::new(secret_key.inner),
        }
    }

    /// Get the corresponding DerivedPubkey
    fn public_key(&self) -> PyDerivedPubkey {
        PyDerivedPubkey {
            inner: self.inner.public_key(),
        }
    }

    /// Sign a message with the secret key
    fn sign(&self, msg: &[u8]) -> PySignature {
        PySignature {
            inner: self.inner.sign(msg),
        }
    }

    /// Return string representation for debugging
    fn __repr__(&self) -> String {
        format!(
            "DerivedSecretKey(public_key={})",
            self.public_key().as_hex()
        )
    }
}

#[pyclass(name = "EVMNetwork", eq)]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PyEVMNetwork {
    inner: EVMNetwork,
}

#[pymethods]
impl PyEVMNetwork {
    /// Creates a new network configuration.
    ///
    /// If `local` is true, configures for local network connections.
    #[new]
    fn new(local: bool) -> PyResult<Self> {
        let inner =
            EVMNetwork::new(local).map_err(|e| PyRuntimeError::new_err(format!("{e:?}")))?;
        Ok(Self { inner })
    }

    #[staticmethod]
    fn new_custom(rpc_url: &str, payment_token_addr: &str, data_payments_addr: &str) -> Self {
        Self {
            inner: EVMNetwork::new_custom(rpc_url, payment_token_addr, data_payments_addr),
        }
    }

    fn identifier(&self) -> &str {
        self.inner.identifier()
    }

    fn rpc_url(&self) -> String {
        self.inner.rpc_url().as_str().to_string()
    }

    fn payment_token_address(&self) -> String {
        self.inner.payment_token_address().to_string()
    }

    fn data_payments_address(&self) -> String {
        self.inner.data_payments_address().to_string()
    }
}

/// Metadata for files in an archive, containing creation time, modification time, and size.
#[pyclass(name = "Metadata")]
#[derive(Debug, Clone)]
pub struct PyMetadata {
    inner: Metadata,
}

#[pymethods]
impl PyMetadata {
    /// Create new metadata with the given file size
    #[new]
    fn new(size: u64) -> Self {
        Self {
            inner: Metadata::new_with_size(size),
        }
    }

    /// Get the creation time as Unix timestamp in seconds
    #[getter]
    fn get_created(&self) -> u64 {
        self.inner.created
    }

    /// Set the creation time as Unix timestamp in seconds
    #[setter]
    fn set_created(&mut self, value: u64) {
        self.inner.created = value;
    }

    /// Get the modification time as Unix timestamp in seconds
    #[getter]
    fn get_modified(&self) -> u64 {
        self.inner.modified
    }

    /// Set the modification time as Unix timestamp in seconds
    #[setter]
    fn set_modified(&mut self, value: u64) {
        self.inner.modified = value;
    }

    /// Get the file size in bytes
    #[getter]
    fn get_size(&self) -> u64 {
        self.inner.size
    }

    /// Set the file size in bytes
    #[setter]
    fn set_size(&mut self, value: u64) {
        self.inner.size = value;
    }
}

/// A public archive containing files that can be accessed by anyone on the network.
#[pyclass(name = "PublicArchive")]
#[derive(Debug, Clone)]
pub struct PyPublicArchive {
    inner: PublicArchive,
}

#[pymethods]
impl PyPublicArchive {
    /// Create a new empty archive
    #[new]
    fn new() -> Self {
        Self {
            inner: PublicArchive::new(),
        }
    }

    /// Rename a file in the archive.
    ///
    /// Returns None on success, or error message on failure
    fn rename_file(&mut self, old_path: PathBuf, new_path: PathBuf) -> PyResult<()> {
        self.inner
            .rename_file(&old_path, &new_path)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to rename file: {e}")))
    }

    /// Add a file to the archive
    fn add_file(&mut self, path: PathBuf, addr: &PyDataAddress, metadata: &PyMetadata) {
        self.inner
            .add_file(path, addr.inner, metadata.inner.clone());
    }

    /// List all files in the archive.
    ///
    /// Returns a list of (path, metadata) tuples
    fn files(&self) -> Vec<(PathBuf, PyMetadata)> {
        self.inner
            .files()
            .into_iter()
            .map(|(path, meta)| (path, PyMetadata { inner: meta }))
            .collect()
    }

    /// List all data addresses of files in the archive
    fn addresses(&self) -> Vec<String> {
        self.inner
            .addresses()
            .into_iter()
            .map(|a| a.to_hex())
            .collect()
    }
}

/// A public archive containing files that can be accessed by anyone on the network.
#[pyclass(name = "PrivateArchive")]
#[derive(Debug, Clone)]
pub struct PyPrivateArchive {
    inner: PrivateArchive,
}

#[pymethods]
impl PyPrivateArchive {
    /// Create a new empty archive
    #[new]
    fn new() -> Self {
        Self {
            inner: PrivateArchive::new(),
        }
    }

    /// Rename a file in the archive.
    ///
    /// Returns None on success, or error message on failure
    fn rename_file(&mut self, old_path: PathBuf, new_path: PathBuf) -> PyResult<()> {
        self.inner
            .rename_file(&old_path, &new_path)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to rename file: {e}")))
    }

    /// Add a file to a local archive. Note that this does not upload the archive to the network.
    fn add_file(&mut self, path: PathBuf, data_map: &PyDataMapChunk, metadata: &PyMetadata) {
        self.inner
            .add_file(path, data_map.inner.clone(), metadata.inner.clone());
    }

    /// List all files in the archive.
    fn files(&self) -> Vec<(PathBuf, PyMetadata)> {
        self.inner
            .files()
            .into_iter()
            .map(|(path, meta)| (path, PyMetadata { inner: meta }))
            .collect()
    }

    /// List all datamaps of files in the archive
    fn data_maps(&self) -> Vec<PyDataMapChunk> {
        self.inner
            .data_maps()
            .into_iter()
            .map(|data_map| PyDataMapChunk { inner: data_map })
            .collect()
    }
}

/// A generic GraphEntry on the Network.
///
/// Graph entries are stored at the owner's public key. Note that there can only be one graph entry per owner.
/// Graph entries can be linked to other graph entries as parents or descendants.
/// Applications are free to define the meaning of these links, those are not enforced by the protocol.
/// The protocol only ensures that the graph entry is immutable once uploaded and that the signature is valid and matches the owner.
///
/// For convenience it is advised to make use of BLS key derivation to create multiple graph entries from a single key.
#[pyclass(name = "GraphEntry", eq, ord)]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct PyGraphEntry {
    inner: GraphEntry,
}

#[pymethods]
impl PyGraphEntry {
    /// Create a new graph entry, signing it with the provided secret key.
    #[new]
    fn new(
        owner: PySecretKey,
        parents: Vec<PyPublicKey>,
        content: [u8; 32],
        descendants: Vec<(PyPublicKey, [u8; 32])>,
    ) -> PyResult<Self> {
        Ok(Self {
            inner: GraphEntry::new(
                &owner.inner,
                parents.into_iter().map(|p| p.inner).collect(),
                content,
                descendants.into_iter().map(|p| (p.0.inner, p.1)).collect(),
            ),
        })
    }

    /// Returns the network address where this entry is stored.
    pub fn address(&self) -> PyGraphEntryAddress {
        PyGraphEntryAddress {
            inner: self.inner.address(),
        }
    }
}

/// Scratchpad, a mutable space for encrypted data on the Network
#[pyclass(name = "Scratchpad", eq, ord)]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct PyScratchpad {
    inner: Scratchpad,
}

#[pymethods]
impl PyScratchpad {
    /// Creates a new instance of Scratchpad. Encrypts the data, and signs all the elements.
    #[new]
    fn new(
        owner: PySecretKey,
        data_encoding: u64,
        unencrypted_data: Vec<u8>,
        counter: u64,
    ) -> PyResult<Self> {
        Ok(Self {
            inner: Scratchpad::new(
                &owner.inner,
                data_encoding,
                &Bytes::from(unencrypted_data),
                counter,
            ),
        })
    }

    /// Returns the address of the scratchpad.
    pub fn address(&self) -> PyScratchpadAddress {
        PyScratchpadAddress {
            inner: *self.inner.address(),
        }
    }

    /// Return the current data encoding.
    pub fn data_encoding(&self) -> u64 {
        self.inner.data_encoding()
    }

    /// Get the counter of the Scratchpad, the higher the counter, the more recent the Scratchpad is.
    ///
    /// Similarly to counter CRDTs only the latest version (highest counter) of the Scratchpad is kept on the network
    pub fn counter(&self) -> u64 {
        self.inner.counter()
    }

    /// Returns the encrypted_data, decrypted via the passed SecretKey
    pub fn decrypt_data(&self, sk: PySecretKey) -> PyResult<Vec<u8>> {
        let data = self
            .inner
            .decrypt_data(&sk.inner)
            .map_err(|e| PyRuntimeError::new_err(format!("{e}")))?;
        Ok(data.to_vec())
    }
}

/// A handle to the register history
#[pyclass(name = "RegisterHistory")]
#[derive(Clone)]
pub struct PyRegisterHistory {
    inner: Arc<futures::lock::Mutex<RegisterHistory>>,
}

impl PyRegisterHistory {
    fn new(history: RegisterHistory) -> Self {
        Self {
            inner: Arc::new(futures::lock::Mutex::new(history)),
        }
    }
}

#[pymethods]
impl PyRegisterHistory {
    fn next<'a>(&'a mut self, py: Python<'a>) -> PyResult<Bound<'a, PyAny>> {
        let arc = Arc::clone(&self.inner);

        future_into_py(py, async move {
            let mut register_history = arc.lock().await;
            let value = register_history
                .next()
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("history `next` failed: {e}")))?;

            Ok(value)
        })
    }

    fn collect<'a>(&'a mut self, py: Python<'a>) -> PyResult<Bound<'a, PyAny>> {
        let arc = Arc::clone(&self.inner);

        future_into_py(py, async move {
            let mut register_history = arc.lock().await;
            let values = register_history
                .collect()
                .await
                .map_err(|e| PyRuntimeError::new_err(format!("history `collect` failed: {e}")))?;

            Ok(values)
        })
    }
}

/// Configuration for the `Client` which can be provided through: `init_with_config`.
#[pyclass(name = "ClientConfig")]
#[derive(Debug, Clone)]
pub struct PyClientConfig {
    inner: ClientConfig,
}

#[pymethods]
impl PyClientConfig {
    #[new]
    fn new() -> Self {
        Self {
            inner: ClientConfig::default(),
        }
    }

    /// Whether we're expected to connect to a local network.
    #[getter]
    fn get_local(&self) -> bool {
        self.inner.init_peers_config.local
    }

    /// Whether we're expected to connect to a local network.
    #[setter]
    fn set_local(&mut self, value: bool) {
        self.inner.init_peers_config.local = value;
    }

    /// List of peers to connect to.
    ///
    /// If not provided, the client will use the default bootstrap peers.
    #[getter]
    fn get_peers(&self) -> Vec<String> {
        self.inner
            .init_peers_config
            .addrs
            .iter()
            .map(|p| p.to_string())
            .collect()
    }

    /// List of peers to connect to. If given empty list, the client will use the default bootstrap peers.
    #[setter]
    fn set_peers(&mut self, peers: Vec<String>) -> PyResult<()> {
        if peers.is_empty() {
            return Ok(());
        }

        let peers: Vec<Multiaddr> = peers
            .iter()
            .map(|p| Multiaddr::from_str(p))
            .collect::<Result<_, _>>()
            .map_err(|e| PyValueError::new_err(format!("Failed to parse peers: {e}")))?;

        self.inner.init_peers_config.addrs = peers;
        Ok(())
    }

    /// EVM network to use for quotations and payments.
    #[getter]
    fn get_network(&self) -> PyEVMNetwork {
        PyEVMNetwork {
            inner: self.inner.evm_network.clone(),
        }
    }

    /// EVM network to use for quotations and payments.
    #[setter]
    fn set_network(&mut self, network: PyEVMNetwork) {
        self.inner.evm_network = network.inner;
    }
}

/// A handle to a XorName.
#[pyclass(name = "XorName")]
#[derive(Eq, Copy, Clone, Default, Hash, Ord, PartialEq, PartialOrd)]
pub struct PyXorName {
    pub(crate) inner: XorName,
}

#[pymethods]
impl PyXorName {
    /// Generate a XorName for the given content
    #[staticmethod]
    fn from_content(content: &[u8]) -> Self {
        Self {
            inner: XorName::from_content(content),
        }
    }

    /// Generate a XorName from multiple content parts
    #[staticmethod]
    fn from_content_parts(content_parts: Vec<Vec<u8>>) -> Self {
        let refs: Vec<&[u8]> = content_parts.iter().map(|v| v.as_slice()).collect();
        Self {
            inner: XorName::from_content_parts(&refs),
        }
    }

    /// Generate a random XorName
    #[staticmethod]
    fn random() -> Self {
        Self {
            inner: XorName::random(&mut rand::thread_rng()),
        }
    }

    /// Returns `true` if the `i`-th bit is `1`
    fn bit(&self, i: u8) -> bool {
        self.inner.bit(i)
    }

    /// Compares the distance of the arguments to `self`
    /// Returns -1 if `lhs` is closer, 1 if `rhs` is closer, and 0 if equal
    fn cmp_distance(&self, lhs: &PyXorName, rhs: &PyXorName) -> i32 {
        match self.inner.cmp_distance(&lhs.inner, &rhs.inner) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }

    /// Returns a copy of `self`, with the `i`-th bit set to `bit`
    fn with_bit(&self, i: u8, bit: bool) -> Self {
        Self {
            inner: self.inner.with_bit(i, bit),
        }
    }

    /// Convert the XorName to a hex string
    fn as_hex(&self) -> String {
        hex::encode(self.inner.0)
    }

    /// Create a XorName from a hex string
    #[staticmethod]
    fn from_hex(hex_str: &str) -> PyResult<Self> {
        if hex_str.len() != XOR_NAME_LEN * 2 {
            return Err(PyValueError::new_err(format!(
                "Hex string must be exactly {} characters",
                XOR_NAME_LEN * 2
            )));
        }

        let bytes = hex::decode(hex_str)
            .map_err(|e| PyValueError::new_err(format!("Invalid hex string: {e}")))?;

        let mut array = [0u8; XOR_NAME_LEN];
        array.copy_from_slice(&bytes);

        Ok(Self {
            inner: XorName(array),
        })
    }

    /// Return string representation (short form with first bytes)
    fn __str__(&self) -> String {
        format!("{}", self.inner)
    }

    /// Return debug representation
    fn __repr__(&self) -> String {
        format!("XorName({})", hex::encode(&self.inner.0[..3]))
    }

    fn __richcmp__(&self, other: &PyXorName, op: CompareOp) -> bool {
        match op {
            CompareOp::Lt => self.inner < other.inner,
            CompareOp::Le => self.inner <= other.inner,
            CompareOp::Eq => self.inner == other.inner,
            CompareOp::Ne => self.inner != other.inner,
            CompareOp::Gt => self.inner > other.inner,
            CompareOp::Ge => self.inner >= other.inner,
        }
    }

    fn __hash__(&self) -> isize {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&self.inner, &mut hasher);
        std::hash::Hasher::finish(&hasher) as isize
    }
}

/// Generate a random XorName.
#[pyfunction]
fn random_xor() -> PyXorName {
    PyXorName {
        inner: XorName::random(&mut rand::thread_rng()),
    }
}

#[pymodule]
#[pyo3(name = "autonomi_client")]
fn autonomi_client_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // m.add_class::<PyANTNetwork>()?;
    m.add_class::<PyArchiveAddress>()?;
    m.add_class::<PyAttoTokens>()?;
    m.add_class::<PyBackoff>()?;
    m.add_class::<PyBootstrapCacheConfig>()?;
    m.add_class::<PyChunk>()?;
    m.add_class::<PyChunkAddress>()?;
    m.add_class::<PyClient>()?;
    m.add_class::<PyClientConfig>()?;
    m.add_class::<PyClientEvent>()?;
    m.add_class::<PyClientEventReceiver>()?;
    m.add_class::<PyClientOperatingStrategy>()?;
    m.add_class::<PyDataAddress>()?;
    m.add_class::<PyDataMapChunk>()?;
    m.add_class::<PyDataTypes>()?;
    m.add_class::<PyDerivationIndex>()?;
    m.add_class::<PyDerivedPubkey>()?;
    m.add_class::<PyDerivedSecretKey>()?;
    m.add_class::<PyEVMNetwork>()?;
    m.add_class::<PyGraphEntry>()?;
    m.add_class::<PyGraphEntryAddress>()?;
    m.add_class::<PyInitialPeersConfig>()?;
    m.add_class::<PyMainPubkey>()?;
    m.add_class::<PyMainSecretKey>()?;
    m.add_class::<PyMaxFeePerGas>()?;
    m.add_class::<PyMetadata>()?;
    m.add_class::<PyPaymentOption>()?;
    m.add_class::<PyPaymentQuote>()?;
    m.add_class::<PyPointer>()?;
    m.add_class::<PyPointerAddress>()?;
    m.add_class::<PyPointerTarget>()?;
    m.add_class::<PyPrivateArchive>()?;
    m.add_class::<PyPrivateArchiveDataMap>()?;
    m.add_class::<PyPublicArchive>()?;
    m.add_class::<PyPublicKey>()?;
    m.add_class::<PyQuorum>()?;
    m.add_class::<PyQuoteForAddress>()?;
    m.add_class::<PyQuotingMetrics>()?;
    m.add_class::<PyReceipt>()?;
    m.add_class::<PyRegisterAddress>()?;
    m.add_class::<PyRegisterHistory>()?;
    m.add_class::<PyRetryStrategy>()?;
    m.add_class::<PyScratchpad>()?;
    m.add_class::<PyScratchpadAddress>()?;

    m.add_class::<PySecretKey>()?;
    m.add_class::<PySignature>()?;
    m.add_class::<PyStoreQuote>()?;
    m.add_class::<PyStrategy>()?;
    m.add_class::<PyTransactionConfig>()?;
    m.add_class::<PyUploadSummary>()?;
    m.add_class::<PyUserData>()?;
    m.add_class::<PyVaultSecretKey>()?;
    m.add_class::<PyWallet>()?;
    m.add_class::<PyXorName>()?;
    m.add_function(wrap_pyfunction!(encrypt, m)?)?;
    m.add_function(wrap_pyfunction!(random_xor, m)?)?;
    Ok(())
}
