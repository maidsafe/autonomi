// TODO: Shall be removed once the python binding warnings resolved
#![allow(non_local_definitions)]

use crate::client::{
    chunk::DataMapChunk,
    files::{archive_private::PrivateArchiveAccess, archive_public::ArchiveAddr},
    payment::PaymentOption as RustPaymentOption,
    vault::{UserData, VaultSecretKey as RustVaultSecretKey},
    Client as RustClient,
};
use crate::{Bytes, Network, Wallet as RustWallet};
use ant_protocol::storage::{
    ChunkAddress, Pointer as RustPointer, PointerAddress as RustPointerAddress,
    PointerTarget as RustPointerTarget,
};
use bls::{PublicKey as RustPublicKey, SecretKey as RustSecretKey};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3_async_runtimes::tokio::future_into_py;
use xor_name::XorName;

#[pyclass(name = "Client")]
pub(crate) struct Client {
    inner: RustClient,
}

#[pymethods]
impl Client {
    #[staticmethod]
    fn init(py: Python) -> PyResult<Bound<PyAny>> {
        future_into_py(py, async {
            match RustClient::init().await {
                Ok(client) => Ok(Client { inner: client }),
                Err(e) => Err(PyValueError::new_err(format!("Failed to connect: {e}"))),
            }
        })
    }

    fn data_put(&self, data: Vec<u8>, payment: &PaymentOption) -> PyResult<PyDataMapChunk> {
        let rt = tokio::runtime::Runtime::new().expect("Could not start tokio runtime");
        let access = rt
            .block_on(
                self.inner
                    .data_put(Bytes::from(data), payment.inner.clone()),
            )
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Failed to put data: {e}"))
            })?;

        Ok(PyDataMapChunk { inner: access })
    }

    fn data_get(&self, access: &PyDataMapChunk) -> PyResult<Vec<u8>> {
        let rt = tokio::runtime::Runtime::new().expect("Could not start tokio runtime");
        let data = rt
            .block_on(self.inner.data_get(access.inner.clone()))
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Failed to get data: {e}"))
            })?;
        Ok(data.to_vec())
    }

    fn data_put_public<'a>(
        &self,
        py: Python<'a>,
        data: Vec<u8>,
        payment: &PaymentOption,
    ) -> PyResult<Bound<'a, PyAny>> {
        let this = self.inner.clone();
        let payment = payment.inner.clone();
        future_into_py(py, async move {
            match this
                .data_put_public(bytes::Bytes::from(data), payment)
                .await
            {
                Ok(addr) => Ok(crate::client::address::addr_to_str(addr)),
                Err(e) => Err(PyValueError::new_err(format!(
                    "Failed to put data: {:?}",
                    eyre::Report::from(e)
                ))),
            }
        })
    }

    fn data_get_public<'a>(&self, py: Python<'a>, addr: &str) -> PyResult<Bound<'a, PyAny>> {
        let this = self.inner.clone();
        let addr = crate::client::address::str_to_addr(addr).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Invalid address: {e}"))
        })?;
        future_into_py(py, async move {
            match this.data_get_public(addr).await {
                Ok(data) => Ok(data.to_vec()),
                Err(e) => Err(PyValueError::new_err(format!("Failed to put data: {e}"))),
            }
        })
    }

    fn vault_cost(&self, key: &PyVaultSecretKey) -> PyResult<String> {
        let rt = tokio::runtime::Runtime::new().expect("Could not start tokio runtime");
        let cost = rt
            .block_on(self.inner.vault_cost(&key.inner))
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Failed to get vault cost: {e}"))
            })?;
        Ok(cost.to_string())
    }

    fn write_bytes_to_vault(
        &self,
        data: Vec<u8>,
        payment: &PaymentOption,
        key: &PyVaultSecretKey,
        content_type: u64,
    ) -> PyResult<String> {
        let rt = tokio::runtime::Runtime::new().expect("Could not start tokio runtime");
        let cost = rt
            .block_on(self.inner.write_bytes_to_vault(
                bytes::Bytes::from(data),
                payment.inner.clone(),
                &key.inner,
                content_type,
            ))
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Failed to write to vault: {e}"))
            })?;
        Ok(cost.to_string())
    }

    fn fetch_and_decrypt_vault(&self, key: &PyVaultSecretKey) -> PyResult<(Vec<u8>, u64)> {
        let rt = tokio::runtime::Runtime::new().expect("Could not start tokio runtime");
        let (data, content_type) = rt
            .block_on(self.inner.fetch_and_decrypt_vault(&key.inner))
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Failed to fetch vault: {e}"))
            })?;
        Ok((data.to_vec(), content_type))
    }

    fn get_user_data_from_vault(&self, key: &PyVaultSecretKey) -> PyResult<PyUserData> {
        let rt = tokio::runtime::Runtime::new().expect("Could not start tokio runtime");
        let user_data = rt
            .block_on(self.inner.get_user_data_from_vault(&key.inner))
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!(
                    "Failed to get user data from vault: {e}"
                ))
            })?;

        Ok(PyUserData { inner: user_data })
    }

    fn put_user_data_to_vault(
        &self,
        key: &PyVaultSecretKey,
        payment: &PaymentOption,
        user_data: &PyUserData,
    ) -> PyResult<()> {
        let rt = tokio::runtime::Runtime::new().expect("Could not start tokio runtime");
        rt.block_on(self.inner.put_user_data_to_vault(
            &key.inner,
            payment.inner.clone(),
            user_data.inner.clone(),
        ))
        .map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Failed to put user data: {e}"))
        })?;
        Ok(())
    }

    fn pointer_get(&self, address: &str) -> PyResult<PyPointer> {
        let rt = tokio::runtime::Runtime::new().expect("Could not start tokio runtime");
        let xorname = XorName::from_content(&hex::decode(address).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Invalid pointer address: {e}"))
        })?);
        let address = RustPointerAddress::new(xorname);

        let pointer = rt.block_on(self.inner.pointer_get(address)).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Failed to get pointer: {e}"))
        })?;

        Ok(PyPointer { inner: pointer })
    }

    fn pointer_put(
        &self,
        counter: u32,
        target: &PyPointerTarget,
        key: &PySecretKey,
        payment_option: &PaymentOption,
    ) -> PyResult<PyPointerAddress> {
        let rt = tokio::runtime::Runtime::new().expect("Could not start tokio runtime");
        let pointer = RustPointer::new(&key.inner, counter, target.inner.clone());
        let (_price, addr) = rt
            .block_on(
                self.inner
                    .pointer_put(pointer, payment_option.inner.clone()),
            )
            .map_err(|e| PyValueError::new_err(format!("Failed to put pointer: {e}")))?;
        Ok(PyPointerAddress { inner: addr })
    }

    fn pointer_cost(&self, key: &PyPublicKey) -> PyResult<String> {
        let rt = tokio::runtime::Runtime::new().expect("Could not start tokio runtime");
        let cost = rt
            .block_on(self.inner.pointer_cost(key.inner))
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Failed to get pointer cost: {e}"))
            })?;
        Ok(cost.to_string())
    }
}

#[pyclass(name = "PointerAddress")]
#[derive(Debug, Clone)]
pub struct PyPointerAddress {
    inner: RustPointerAddress,
}

#[pymethods]
impl PyPointerAddress {
    #[new]
    pub fn new(hex_str: String) -> PyResult<Self> {
        let bytes = hex::decode(&hex_str)
            .map_err(|e| PyValueError::new_err(format!("Invalid hex string: {e}")))?;
        let xorname = XorName::from_content(&bytes);
        Ok(Self {
            inner: RustPointerAddress::new(xorname),
        })
    }

    #[getter]
    pub fn hex(&self) -> String {
        let bytes: [u8; 32] = self.inner.xorname().0;
        hex::encode(bytes)
    }
}

#[pyclass(name = "Pointer")]
#[derive(Debug, Clone)]
pub struct PyPointer {
    inner: RustPointer,
}

#[pymethods]
impl PyPointer {
    #[new]
    pub fn new(counter: u32, target: &PyPointerTarget, key: &PySecretKey) -> PyResult<Self> {
        Ok(Self {
            inner: RustPointer::new(&key.inner, counter, target.inner.clone()),
        })
    }

    pub fn network_address(&self) -> PyPointerAddress {
        PyPointerAddress {
            inner: self.inner.network_address(),
        }
    }

    #[getter]
    fn hex(&self) -> String {
        let bytes: [u8; 32] = self.inner.xorname().0;
        hex::encode(bytes)
    }

    #[getter]
    fn target(&self) -> PyPointerTarget {
        PyPointerTarget {
            inner: RustPointerTarget::ChunkAddress(ChunkAddress::new(self.inner.xorname())),
        }
    }
}

#[pyclass(name = "PointerTarget")]
#[derive(Debug, Clone)]
pub struct PyPointerTarget {
    inner: RustPointerTarget,
}

#[pymethods]
impl PyPointerTarget {
    #[new]
    fn new(xorname: &[u8]) -> PyResult<Self> {
        Ok(Self {
            inner: RustPointerTarget::ChunkAddress(ChunkAddress::new(XorName::from_content(
                xorname,
            ))),
        })
    }

    #[getter]
    fn hex(&self) -> String {
        let bytes: [u8; 32] = self.inner.xorname().0;
        hex::encode(bytes)
    }

    #[getter]
    fn target(&self) -> PyPointerTarget {
        PyPointerTarget {
            inner: RustPointerTarget::ChunkAddress(ChunkAddress::new(self.inner.xorname())),
        }
    }

    #[staticmethod]
    fn from_xorname(xorname: &[u8]) -> PyResult<Self> {
        Ok(Self {
            inner: RustPointerTarget::ChunkAddress(ChunkAddress::new(XorName::from_content(
                xorname,
            ))),
        })
    }

    #[staticmethod]
    fn from_chunk_address(addr: &PyChunkAddress) -> Self {
        Self {
            inner: RustPointerTarget::ChunkAddress(addr.inner),
        }
    }
}

#[pyclass(name = "ChunkAddress")]
#[derive(Debug, Clone)]
pub struct PyChunkAddress {
    inner: ChunkAddress,
}

impl From<ChunkAddress> for PyChunkAddress {
    fn from(addr: ChunkAddress) -> Self {
        Self { inner: addr }
    }
}

impl From<PyChunkAddress> for ChunkAddress {
    fn from(addr: PyChunkAddress) -> Self {
        addr.inner
    }
}

#[pymethods]
impl PyChunkAddress {
    #[new]
    fn new(xorname: &[u8]) -> PyResult<Self> {
        Ok(Self {
            inner: ChunkAddress::new(XorName::from_content(xorname)),
        })
    }

    #[getter]
    fn hex(&self) -> String {
        let bytes: [u8; 32] = self.inner.xorname().0;
        hex::encode(bytes)
    }

    #[staticmethod]
    fn from_chunk_address(addr: &str) -> PyResult<Self> {
        let bytes = hex::decode(addr).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Invalid chunk address: {e}"))
        })?;

        if bytes.len() != 32 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "Invalid chunk address length: must be 32 bytes",
            ));
        }

        let mut xorname = [0u8; 32];
        xorname.copy_from_slice(&bytes);

        Ok(Self {
            inner: ChunkAddress::new(XorName(xorname)),
        })
    }

    fn __str__(&self) -> PyResult<String> {
        Ok(self.hex())
    }

    fn __repr__(&self) -> PyResult<String> {
        Ok(format!("ChunkAddress({})", self.hex()))
    }
}

#[pyclass(name = "Wallet")]
pub struct Wallet {
    pub(crate) inner: RustWallet,
}

#[pymethods]
impl Wallet {
    #[new]
    fn new(private_key: String) -> PyResult<Self> {
        let wallet = RustWallet::new_from_private_key(
            Network::ArbitrumOne, // TODO: Make this configurable
            &private_key,
        )
        .map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Invalid private key: {e}"))
        })?;

        Ok(Self { inner: wallet })
    }

    fn address(&self) -> String {
        format!("{:?}", self.inner.address())
    }

    fn balance(&self) -> PyResult<String> {
        let rt = tokio::runtime::Runtime::new().expect("Could not start tokio runtime");
        let balance = rt
            .block_on(async { self.inner.balance_of_tokens().await })
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Failed to get balance: {e}"))
            })?;

        Ok(balance.to_string())
    }

    fn balance_of_gas(&self) -> PyResult<String> {
        let rt = tokio::runtime::Runtime::new().expect("Could not start tokio runtime");
        let balance = rt
            .block_on(async { self.inner.balance_of_gas_tokens().await })
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Failed to get balance: {e}"))
            })?;

        Ok(balance.to_string())
    }
}

#[pyclass(name = "PaymentOption")]
pub struct PaymentOption {
    pub(crate) inner: RustPaymentOption,
}

#[pymethods]
impl PaymentOption {
    #[staticmethod]
    fn wallet(wallet: &Wallet) -> Self {
        Self {
            inner: RustPaymentOption::Wallet(wallet.inner.clone()),
        }
    }
}

#[pyclass(name = "SecretKey")]
#[derive(Debug, Clone)]
pub struct PySecretKey {
    inner: RustSecretKey,
}

#[pymethods]
impl PySecretKey {
    #[new]
    fn new() -> PyResult<Self> {
        Ok(Self {
            inner: RustSecretKey::random(),
        })
    }

    #[staticmethod]
    fn from_hex(hex_str: &str) -> PyResult<Self> {
        RustSecretKey::from_hex(hex_str)
            .map(|key| Self { inner: key })
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Invalid hex key: {e}")))
    }

    fn to_hex(&self) -> String {
        self.inner.to_hex()
    }
}

#[pyclass(name = "PublicKey")]
#[derive(Debug, Clone)]
pub struct PyPublicKey {
    inner: RustPublicKey,
}

#[pymethods]
impl PyPublicKey {
    #[new]
    fn new() -> PyResult<Self> {
        let secret = RustSecretKey::random();
        Ok(Self {
            inner: secret.public_key(),
        })
    }

    #[staticmethod]
    fn from_hex(hex_str: &str) -> PyResult<Self> {
        RustPublicKey::from_hex(hex_str)
            .map(|key| Self { inner: key })
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Invalid hex key: {e}")))
    }

    fn to_hex(&self) -> String {
        self.inner.to_hex()
    }
}

#[pyclass(name = "VaultSecretKey")]
#[derive(Debug, Clone)]
pub struct PyVaultSecretKey {
    inner: RustVaultSecretKey,
}

#[pymethods]
impl PyVaultSecretKey {
    #[new]
    fn new() -> PyResult<Self> {
        Ok(Self {
            inner: RustVaultSecretKey::random(),
        })
    }

    #[staticmethod]
    fn from_hex(hex_str: &str) -> PyResult<Self> {
        RustVaultSecretKey::from_hex(hex_str)
            .map(|key| Self { inner: key })
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Invalid hex key: {e}")))
    }

    fn to_hex(&self) -> String {
        self.inner.to_hex()
    }
}

#[pyclass(name = "UserData")]
#[derive(Debug, Clone)]
pub struct PyUserData {
    inner: UserData,
}

#[pymethods]
impl PyUserData {
    #[new]
    fn new() -> Self {
        Self {
            inner: UserData::new(),
        }
    }

    fn add_file_archive(&mut self, archive: &str) -> Option<String> {
        let name = XorName::from_content(archive.as_bytes());
        let archive_addr = ArchiveAddr::from_content(&name);
        self.inner.add_file_archive(archive_addr)
    }

    fn add_private_file_archive(&mut self, archive: &str) -> Option<String> {
        let name = XorName::from_content(archive.as_bytes());
        let private_access = match PrivateArchiveAccess::from_hex(&name.to_string()) {
            Ok(access) => access,
            Err(_e) => return None,
        };
        self.inner.add_private_file_archive(private_access)
    }

    fn file_archives(&self) -> Vec<(String, String)> {
        self.inner
            .file_archives
            .iter()
            .map(|(addr, name)| (format!("{addr:x}"), name.clone()))
            .collect()
    }

    fn private_file_archives(&self) -> Vec<(String, String)> {
        self.inner
            .private_file_archives
            .iter()
            .map(|(addr, name)| (addr.to_hex(), name.clone()))
            .collect()
    }
}

#[pyclass(name = "DataMapChunk")]
#[derive(Debug, Clone)]
pub struct PyDataMapChunk {
    inner: DataMapChunk,
}

#[pymethods]
impl PyDataMapChunk {
    #[staticmethod]
    fn from_hex(hex: &str) -> PyResult<Self> {
        DataMapChunk::from_hex(hex)
            .map(|access| Self { inner: access })
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Invalid hex: {e}")))
    }

    fn to_hex(&self) -> String {
        self.inner.to_hex()
    }

    fn address(&self) -> String {
        self.inner.address().to_string()
    }
}

#[pyfunction]
fn encrypt(data: Vec<u8>) -> PyResult<(Vec<u8>, Vec<Vec<u8>>)> {
    let (data_map, chunks) = self_encryption::encrypt(Bytes::from(data))
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Encryption failed: {e}")))?;

    let data_map_bytes = rmp_serde::to_vec(&data_map)
        .map_err(|e| PyValueError::new_err(format!("Failed to serialize data map: {e}")))?;

    let chunks_bytes: Vec<Vec<u8>> = chunks
        .into_iter()
        .map(|chunk| chunk.content.to_vec())
        .collect();

    Ok((data_map_bytes, chunks_bytes))
}

#[pymodule]
#[pyo3(name = "autonomi_client")]
fn autonomi_client_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Client>()?;
    m.add_class::<Wallet>()?;
    m.add_class::<PaymentOption>()?;
    m.add_class::<PyVaultSecretKey>()?;
    m.add_class::<PyUserData>()?;
    m.add_class::<PyDataMapChunk>()?;
    m.add_class::<PyPointer>()?;
    m.add_class::<PyPointerAddress>()?;
    m.add_class::<PyPointerTarget>()?;
    m.add_class::<PyChunkAddress>()?;
    m.add_class::<PySecretKey>()?;
    m.add_class::<PyPublicKey>()?;
    m.add_function(wrap_pyfunction!(encrypt, m)?)?;
    Ok(())
}
