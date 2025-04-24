use std::{path::PathBuf, str::FromStr};

use autonomi::{AttoTokens, Bytes, Chunk, Multiaddr, Signature};

use napi::bindgen_prelude::*;
use napi_derive::napi;
use tokio::sync::Mutex;

// Convert Rust errors to JavaScript errors
fn map_error<E>(err: E) -> napi::Error
where
    E: std::error::Error,
{
    let mut err_str = String::new();
    err_str.push_str(&format!("{err:?}: {err}\n"));
    let mut source = err.source();
    while let Some(err) = source {
        err_str.push_str(&format!(" Caused by: {err:?}: {err}\n"));
        source = err.source();
    }

    napi::Error::new(Status::GenericFailure, err_str)
}

fn big_int_to_u64(value: BigInt, arg: &str) -> Result<u64> {
    let (_signed, value, losless) = value.get_u64();
    if !losless {
        return Err(napi::Error::new(
            Status::InvalidArg,
            format!("expected `{arg}` to fit in a u64"),
        ));
    }

    Ok(value)
}

fn uint8_array_to_array<const LEN: usize>(value: Uint8Array, arg: &str) -> Result<[u8; LEN]> {
    value.as_ref().try_into().map_err(|_err| {
        napi::Error::new(
            Status::InvalidArg,
            format!(
                "`{arg}` is expected to be a {LEN}-byte array, but is {} bytes long",
                value.len()
            ),
        )
    })
}

/// Represents a client for the Autonomi network.
#[napi]
pub struct Client(autonomi::Client);

#[napi]
impl Client {
    /// Initialize the client with default configuration.
    ///
    /// See `init_with_config`.
    #[napi(factory)]
    pub async fn init() -> Result<Self> {
        let client = autonomi::Client::init().await.map_err(map_error)?;

        Ok(Self(client))
    }

    /// Initialize a client that is configured to be local.
    ///
    /// See `init_with_config`.
    #[napi(factory)]
    pub async fn init_local() -> Result<Self> {
        let client = autonomi::Client::init_local().await.map_err(map_error)?;

        Ok(Self(client))
    }

    /// Initialize a client that bootstraps from a list of peers.
    ///
    /// If any of the provided peers is a global address, the client will not be local.
    #[napi]
    pub async fn init_with_peers(peers: Vec<String>) -> Result<Self> {
        let peers = peers
            .iter()
            .map(|p| Multiaddr::from_str(p))
            .collect::<std::result::Result<Vec<Multiaddr>, _>>()
            .map_err(map_error)?;

        let client = autonomi::Client::init_with_peers(peers)
            .await
            .map_err(map_error)?;

        Ok(Self(client))
    }

    // /// Initialize the client with the given configuration.
    // ///
    // /// This will block until CLOSE_GROUP_SIZE have been added to the routing table.
    // ///
    // /// See ClientConfig.
    // #[napi]
    // pub async fn init_with_config(config: ClientConfig) -> Result<Self> {
    //     todo!()
    // }

    #[napi]
    pub fn evm_network(&self) -> Network {
        Network(self.0.evm_network().clone())
    }

    // Chunks

    /// Get a chunk from the network.
    #[napi]
    pub async fn chunk_get(&self, addr: &ChunkAddress) -> Result<Buffer> {
        let chunk = self.0.chunk_get(&addr.0).await.map_err(map_error)?;

        Ok(Buffer::from(chunk.value.to_vec()))
    }

    /// Manually upload a chunk to the network.
    ///
    /// It is recommended to use the `data_put` method instead to upload data.
    #[napi]
    pub async fn chunk_put(
        &self,
        data: Buffer,
        payment_option: &PaymentOption,
    ) -> Result<tuple_result::ChunkPut> {
        let chunk = Chunk::new(Bytes::from(data.as_ref().to_vec()));

        let (cost, addr) = self
            .0
            .chunk_put(&chunk, payment_option.0.clone())
            .await
            .map_err(map_error)?;

        Ok(tuple_result::ChunkPut { cost, addr })
    }

    /// Get the cost of a chunk.
    #[napi]
    pub async fn chunk_cost(&self, addr: &ChunkAddress) -> Result</* AttoTokens */ String> {
        let cost = self.0.chunk_cost(&addr.0).await.map_err(map_error)?;

        Ok(cost.to_string())
    }

    // /// Upload chunks and retry failed uploads up to RETRY_ATTEMPTS times.
    // #[napi]
    // pub async fn upload_chunks_with_retries(&self, chunks: Vec<Chunk>, receipt: &Receipt) -> Vec<(Chunk, PutError)> {
    //     todo!()
    // }

    // Graph entries

    /// Fetches a GraphEntry from the network.
    #[napi]
    pub async fn graph_entry_get(&self, address: &GraphEntryAddress) -> Result<GraphEntry> {
        let graph_entry = self
            .0
            .graph_entry_get(&address.0)
            .await
            .map_err(map_error)?;

        Ok(GraphEntry(graph_entry))
    }

    /// Check if a graph_entry exists on the network
    #[napi]
    pub async fn graph_entry_check_existance(&self, address: &GraphEntryAddress) -> Result<bool> {
        let exists = self
            .0
            .graph_entry_check_existence(&address.0)
            .await
            .map_err(map_error)?;

        Ok(exists)
    }

    /// Manually puts a GraphEntry to the network.
    #[napi]
    pub async fn graph_entry_put(
        &self,
        entry: &GraphEntry,
        payment_option: &PaymentOption,
    ) -> Result</* (AttoTokens, GraphEntryAddress) */ tuple_result::GraphEntryPut> {
        let (cost, addr) = self
            .0
            .graph_entry_put(entry.0.clone(), payment_option.0.clone())
            .await
            .map_err(map_error)?;

        Ok(tuple_result::GraphEntryPut { cost, addr })
    }

    /// Get the cost to create a GraphEntry
    #[napi]
    pub async fn graph_entry_cost(&self, key: &PublicKey) -> Result</* AttoTokens */ String> {
        self.0
            .graph_entry_cost(&key.0)
            .await
            .map(|c| c.to_string())
            .map_err(map_error)
    }

    // Pointers

    /// Get a pointer from the network
    #[napi]
    pub async fn pointer_get(&self, address: &PointerAddress) -> Result<Pointer> {
        self.0
            .pointer_get(&address.0)
            .await
            .map(Pointer)
            .map_err(map_error)
    }

    /// Check if a pointer exists on the network
    #[napi]
    pub async fn pointer_check_existance(&self, address: &PointerAddress) -> Result<bool> {
        self.0
            .pointer_check_existence(&address.0)
            .await
            .map_err(map_error)
    }

    /// Verify a pointer
    #[napi]
    pub fn pointer_verify(pointer: &Pointer) -> Result<()> {
        autonomi::Client::pointer_verify(&pointer.0).map_err(map_error)
    }

    /// Manually store a pointer on the network
    #[napi]
    pub async fn pointer_put(
        &self,
        pointer: &Pointer,
        payment_option: &PaymentOption,
    ) -> Result</* (AttoTokens, PointerAddress) */ tuple_result::PointerPut> {
        let (cost, addr) = self
            .0
            .pointer_put(pointer.0.clone(), payment_option.0.clone())
            .await
            .map_err(map_error)?;

        Ok(tuple_result::PointerPut { cost, addr })
    }

    /// Create a new pointer on the network.
    ///
    /// Make sure that the owner key is not already used for another pointer as each key is associated with one pointer
    #[napi]
    pub async fn pointer_create(
        &self,
        owner: &SecretKey,
        target: &PointerTarget,
        payment_option: &PaymentOption,
    ) -> Result</* (AttoTokens, PointerAddress) */ tuple_result::PointerPut> {
        let (cost, addr) = self
            .0
            .pointer_create(&owner.0, target.0.clone(), payment_option.0.clone())
            .await
            .map_err(map_error)?;

        Ok(tuple_result::PointerPut { cost, addr })
    }

    /// Update an existing pointer to point to a new target on the network.
    ///
    /// The pointer needs to be created first with Client::pointer_put.
    /// This operation is free as the pointer was already paid for at creation.
    /// Only the latest version of the pointer is kept on the Network,
    /// previous versions will be overwritten and unrecoverable.
    #[napi]
    pub async fn pointer_update(&self, owner: &SecretKey, target: &PointerTarget) -> Result<()> {
        self.0
            .pointer_update(&owner.0, target.0.clone())
            .await
            .map_err(map_error)
    }

    /// Calculate the cost of storing a pointer
    #[napi]
    pub async fn pointer_cost(&self, key: &PublicKey) -> Result</* AttoTokens */ String> {
        let cost = self.0.pointer_cost(&key.0).await.map_err(map_error)?;

        Ok(cost.to_string())
    }

    // Scratchpad

    /// Get Scratchpad from the Network. A Scratchpad is stored at the owner's public key so we can derive the address from it.
    #[napi]
    pub async fn scratchpad_get_from_public_key(
        &self,
        public_key: &PublicKey,
    ) -> Result<Scratchpad> {
        self.0
            .scratchpad_get_from_public_key(&public_key.0)
            .await
            .map(Scratchpad)
            .map_err(map_error)
    }

    /// Get Scratchpad from the Network
    #[napi]
    pub async fn scratchpad_get(&self, address: &ScratchpadAddress) -> Result<Scratchpad> {
        self.0
            .scratchpad_get(&address.0)
            .await
            .map(Scratchpad)
            .map_err(map_error)
    }

    /// Check if a scratchpad exists on the network
    #[napi]
    pub async fn scratchpad_check_existance(&self, address: &ScratchpadAddress) -> Result<bool> {
        self.0
            .scratchpad_check_existence(&address.0)
            .await
            .map_err(map_error)
    }

    /// Verify a scratchpad
    #[napi]
    pub fn scratchpad_verify(scratchpad: &Scratchpad) -> Result<()> {
        autonomi::Client::scratchpad_verify(&scratchpad.0).map_err(map_error)
    }

    /// Manually store a scratchpad on the network
    #[napi]
    pub async fn scratchpad_put(
        &self,
        scratchpad: &Scratchpad,
        payment_option: &PaymentOption,
    ) -> Result</* (AttoTokens, ScratchpadAddress) */ tuple_result::ScratchpadPut> {
        let (cost, addr) = self
            .0
            .scratchpad_put(scratchpad.0.clone(), payment_option.0.clone())
            .await
            .map_err(map_error)?;

        Ok(tuple_result::ScratchpadPut { cost, addr })
    }

    /// Create a new scratchpad to the network.
    ///
    /// Make sure that the owner key is not already used for another scratchpad as each key is associated with one scratchpad. The data will be encrypted with the owner key before being stored on the network. The content type is used to identify the type of data stored in the scratchpad, the choice is up to the caller.
    ///
    /// Returns the cost and the address of the scratchpad.
    #[napi]
    pub async fn scratchpad_create(
        &self,
        owner: &SecretKey,
        content_type: BigInt, // `u64`
        initial_data: Buffer,
        payment_option: &PaymentOption,
    ) -> Result</* (AttoTokens, ScratchpadAddress) */ tuple_result::ScratchpadPut> {
        let content_type = big_int_to_u64(content_type, "content_type")?;

        let (cost, addr) = self
            .0
            .scratchpad_create(
                &owner.0,
                content_type,
                &Bytes::copy_from_slice(&initial_data),
                payment_option.0.clone(),
            )
            .await
            .map_err(map_error)?;

        Ok(tuple_result::ScratchpadPut { cost, addr })
    }

    /// Update an existing scratchpad to the network.
    /// The scratchpad needs to be created first with Client::scratchpad_create.
    /// This operation is free as the scratchpad was already paid for at creation.
    /// Only the latest version of the scratchpad is kept on the Network,
    /// previous versions will be overwritten and unrecoverable.
    #[napi]
    pub async fn scratchpad_update(
        &self,
        owner: &SecretKey,
        content_type: BigInt, // `u64`
        data: Buffer,
    ) -> Result<()> {
        let content_type = big_int_to_u64(content_type, "content_type")?;

        self.0
            .scratchpad_update(&owner.0, content_type, &Bytes::copy_from_slice(&data))
            .await
            .map_err(map_error)
    }

    /// Get the cost of creating a new Scratchpad
    #[napi]
    pub async fn scratchpad_cost(&self, owner: &PublicKey) -> Result</* AttoTokens */ String> {
        let cost = self.0.scratchpad_cost(&owner.0).await.map_err(map_error)?;

        Ok(cost.to_string())
    }

    // Data

    /// Fetch a blob of (private) data from the network
    #[napi]
    pub async fn data_get(&self, data_map: &DataMapChunk) -> Result<Buffer> {
        let data = self.0.data_get(&data_map.0).await.map_err(map_error)?;

        Ok(Buffer::from(data.as_ref()))
    }

    /// Upload a piece of private data to the network. This data will be self-encrypted.
    /// The DataMapChunk is not uploaded to the network, keeping the data private.
    ///
    /// Returns the DataMapChunk containing the map to the encrypted chunks.
    #[napi]
    pub async fn data_put(
        &self,
        data: Buffer,
        payment_option: &PaymentOption,
    ) -> Result</* (AttoTokens, DataMapChunk) */ tuple_result::DataPutResult> {
        let data = Bytes::copy_from_slice(&data);

        let (cost, data_map) = self
            .0
            .data_put(data, payment_option.0.clone())
            .await
            .map_err(map_error)?;

        Ok(tuple_result::DataPutResult { cost, data_map })
    }

    /// Fetch a blob of data from the network
    #[napi]
    pub async fn data_get_public(&self, addr: &DataAddress) -> Result<Buffer> {
        let data = self.0.data_get_public(&addr.0).await.map_err(map_error)?;

        Ok(Buffer::from(data.as_ref()))
    }

    /// Upload a piece of data to the network. This data is publicly accessible.
    ///
    /// Returns the Data Address at which the data was stored.
    #[napi]
    pub async fn data_put_public(
        &self,
        data: Buffer,
        payment_option: &PaymentOption,
    ) -> Result</* (AttoTokens, DataAddress) */ tuple_result::DataPutPublicResult> {
        let data = Bytes::copy_from_slice(&data);

        let (cost, addr) = self
            .0
            .data_put_public(data, payment_option.0.clone())
            .await
            .map_err(map_error)?;

        Ok(tuple_result::DataPutPublicResult { cost, addr })
    }

    /// Get the estimated cost of storing a piece of data.
    #[napi]
    pub async fn data_cost(&self, data: Buffer) -> Result</* AttoTokens */ String> {
        let cost = self
            .0
            .data_cost(Bytes::copy_from_slice(&data))
            .await
            .map_err(map_error)?;

        Ok(cost.to_string())
    }

    // Archives

    /// Fetch a PrivateArchive from the network
    #[napi]
    pub async fn archive_get(&self, addr: &PrivateArchiveDataMap) -> Result<PrivateArchive> {
        let archive = self.0.archive_get(&addr.0).await.map_err(map_error)?;

        Ok(PrivateArchive(archive))
    }

    /// Upload a PrivateArchive to the network
    #[napi]
    pub async fn archive_put(
        &self,
        archive: &PrivateArchive,
        payment_option: &PaymentOption,
    ) -> Result</*(AttoTokens, PrivateArchiveDataMap)*/ tuple_result::ArchivePutResult> {
        let (cost, data_map) = self
            .0
            .archive_put(&archive.0, payment_option.0.clone())
            .await
            .map_err(map_error)?;

        Ok(tuple_result::ArchivePutResult { cost, data_map })
    }

    // <TOPIC>

    /// Fetch an archive from the network
    #[napi]
    pub async fn archive_get_public(&self, addr: &ArchiveAddress) -> Result<PublicArchive> {
        let archive = self
            .0
            .archive_get_public(&addr.0)
            .await
            .map_err(map_error)?;

        Ok(PublicArchive(archive))
    }

    /// Upload an archive to the network
    #[napi]
    pub async fn archive_put_public(
        &self,
        archive: &PublicArchive,
        payment_option: &PaymentOption,
    ) -> Result</* (AttoTokens, ArchiveAddress) */ tuple_result::ArchivePutPublicResult> {
        let (cost, addr) = self
            .0
            .archive_put_public(&archive.0, payment_option.0.clone())
            .await
            .map_err(map_error)?;

        Ok(tuple_result::ArchivePutPublicResult { cost, addr })
    }

    /// Get the cost to upload an archive
    #[napi]
    pub async fn archive_cost(&self, archive: &PublicArchive) -> Result</* AttoTokens */ String> {
        let cost = self
            .0
            .archive_cost(&archive.0.clone())
            .await
            .map_err(map_error)?;

        Ok(cost.to_string())
    }

    // Files

    /// Download a private file from network to local file system
    #[napi]
    pub async fn file_download(
        &self,
        data_map: &DataMapChunk,
        to_dest: /* PathBuf */ String,
    ) -> Result<()> {
        let to_dest = PathBuf::from(to_dest);

        self.0
            .file_download(&data_map.0, to_dest)
            .await
            .map_err(map_error)
    }

    /// Download a private directory from network to local file system
    #[napi]
    pub async fn dir_download(
        &self,
        archive_access: &PrivateArchiveDataMap,
        to_dest: /* PathBuf */ String,
    ) -> Result<()> {
        let to_dest = PathBuf::from(to_dest);

        self.0
            .dir_download(&archive_access.0, to_dest)
            .await
            .map_err(map_error)
    }

    /// Upload the content of all files in a directory to the network.
    /// The directory is recursively walked and each file is uploaded to the network.
    ///
    /// The data maps of these (private) files are not uploaded but returned within
    /// the PrivateArchive return type.

    #[napi]
    pub async fn dir_content_upload(
        &self,
        dir_path: /* PathBuf */ String,
        payment_option: &PaymentOption,
    ) -> Result</* (AttoTokens, PrivateArchive) */ tuple_result::DirContentUpload> {
        let dir_path = PathBuf::from(dir_path);

        let (cost, archive) = self
            .0
            .dir_content_upload(dir_path, payment_option.0.clone())
            .await
            .map_err(map_error)?;

        Ok(tuple_result::DirContentUpload { cost, archive })
    }

    /// Same as Client::dir_content_upload but also uploads the archive (privately) to the network.
    ///
    /// Returns the PrivateArchiveDataMap allowing the private archive to be downloaded from the network.
    #[napi]
    pub async fn dir_upload(
        &self,
        dir_path: /* PathBuf */ String,
        payment_option: &PaymentOption,
    ) -> Result</* (AttoTokens, PrivateArchiveDataMap) */ tuple_result::DirUpload> {
        let dir_path = PathBuf::from(dir_path);

        let (cost, data_map) = self
            .0
            .dir_upload(dir_path, payment_option.0.clone())
            .await
            .map_err(map_error)?;

        Ok(tuple_result::DirUpload { cost, data_map })
    }

    /// Upload the content of a private file to the network. Reads file, splits into
    /// chunks, uploads chunks, uploads datamap, returns DataMapChunk (pointing to the datamap)
    #[napi]
    pub async fn file_content_upload(
        &self,
        path: /* PathBuf */ String,
        payment_option: &PaymentOption,
    ) -> Result</* (AttoTokens, DataMapChunk) */ tuple_result::FileContentUpload> {
        let path = PathBuf::from(path);

        let (cost, data_map) = self
            .0
            .file_content_upload(path, payment_option.0.clone())
            .await
            .map_err(map_error)?;

        Ok(tuple_result::FileContentUpload { cost, data_map })
    }

    /// Download file from network to local file system
    #[napi]
    pub async fn file_download_public(
        &self,
        data_addr: &DataAddress,
        to_dest: /* PathBuf */ String,
    ) -> Result<()> {
        let to_dest = PathBuf::from(to_dest);

        self.0
            .file_download_public(&data_addr.0, to_dest)
            .await
            .map_err(map_error)
    }

    /// Download directory from network to local file system
    #[napi]
    pub async fn dir_download_public(
        &self,
        archive_addr: &ArchiveAddress,
        to_dest: /* PathBuf */ String,
    ) -> Result<()> {
        let to_dest = PathBuf::from(to_dest);

        self.0
            .dir_download_public(&archive_addr.0, to_dest)
            .await
            .map_err(map_error)
    }

    /// Upload the content of all files in a directory to the network. The directory is recursively walked and each file is uploaded to the network.
    ///
    /// The data maps of these files are uploaded on the network, making the individual files publicly available.
    ///
    /// This returns, but does not upload (!),the PublicArchive containing the data maps of the uploaded files.
    #[napi]
    pub async fn dir_content_upload_public(
        &self,
        dir_path: /* PathBuf */ String,
        payment_option: &PaymentOption,
    ) -> Result</* (AttoTokens, PublicArchive) */ tuple_result::DirContentUploadPublic> {
        let dir_path = PathBuf::from(dir_path);

        let (cost, archive) = self
            .0
            .dir_content_upload_public(dir_path, payment_option.0.clone())
            .await
            .map_err(map_error)?;

        Ok(tuple_result::DirContentUploadPublic { cost, archive })
    }

    /// Same as Client::dir_content_upload_public but also uploads the archive to the network.
    ///
    /// Returns the ArchiveAddress of the uploaded archive.
    #[napi]
    pub async fn dir_upload_public(
        &self,
        dir_path: /* PathBuf */ String,
        payment_option: &PaymentOption,
    ) -> Result</* (AttoTokens, ArchiveAddress) */ tuple_result::DirUploadPublic> {
        let dir_path = PathBuf::from(dir_path);

        let (cost, addr) = self
            .0
            .dir_upload_public(dir_path, payment_option.0.clone())
            .await
            .map_err(map_error)?;

        Ok(tuple_result::DirUploadPublic { cost, addr })
    }

    /// Upload the content of a file to the network. Reads file, splits into chunks,
    /// uploads chunks, uploads datamap, returns DataAddr (pointing to the datamap)
    #[napi]
    pub async fn file_content_upload_public(
        &self,
        _path: /* PathBuf */ String,
        _payment_option: &PaymentOption,
    ) -> Result</* (AttoTokens, DataAddress) */ tuple_result::FileContentUploadPublic> {
        todo!()
    }

    /// Get the cost to upload a file/dir to the network. quick and dirty implementation, please refactor once files are cleanly implemented
    #[napi]
    pub async fn file_cost(&self, path: /* &PathBuf */ String) -> Result</* AttoTokens */ String> {
        let cost = self
            .0
            .file_cost(&PathBuf::from(path))
            .await
            .map_err(map_error)?;

        Ok(cost.to_string())
    }

    // Vault/user data

    /// Get the user data from the vault
    #[napi]
    pub async fn get_user_data_from_vault(&self, secret_key: &VaultSecretKey) -> Result<UserData> {
        self.0
            .get_user_data_from_vault(&secret_key.0)
            .await
            .map(UserData)
            .map_err(map_error)
    }

    /// Put the user data to the vault
    ///
    /// Returns the total cost of the put operation
    #[napi]
    pub async fn put_user_data_to_vault(
        &self,
        secret_key: &VaultSecretKey,
        payment_option: &PaymentOption,
        user_data: &UserData,
    ) -> Result</* AttoTokens */ String> {
        self.0
            .put_user_data_to_vault(&secret_key.0, payment_option.0.clone(), user_data.0.clone())
            .await
            .map(|c| c.to_string())
            .map_err(map_error)
    }

    /// Retrieves and returns a decrypted vault if one exists.
    ///
    /// Returns the content type of the bytes in the vault.
    #[napi]
    pub async fn fetch_and_decrypt_vault(
        &self,
        secret_key: &VaultSecretKey,
    ) -> Result</* (Bytes, VaultContentType) */ tuple_result::FetchAndDecryptVault> {
        let (data, content_type) = self
            .0
            .fetch_and_decrypt_vault(&secret_key.0)
            .await
            .map_err(map_error)?;

        Ok(tuple_result::FetchAndDecryptVault { data, content_type })
    }

    /// Get the cost of creating a new vault A quick estimation of cost:
    /// num_of_graph_entry * graph_entry_cost + num_of_scratchpad * scratchpad_cost
    #[napi]
    pub async fn vault_cost(
        &self,
        owner: &VaultSecretKey,
        max_size: /* u64 */ BigInt,
    ) -> Result</* AttoTokens */ String> {
        let max_size = big_int_to_u64(max_size, "max_size")?;

        let cost = self
            .0
            .vault_cost(&owner.0.clone(), max_size)
            .await
            .map_err(map_error)?;

        Ok(cost.to_string())
    }

    /// Put data into the client’s VaultPacket
    ///
    /// Dynamically expand the vault capacity by paying for more space (Scratchpad) when needed.
    ///
    /// It is recommended to use the hash of the app name or unique identifier as the content type.

    #[napi]
    pub async fn write_bytes_to_vault(
        &self,
        data: Buffer,
        payment_option: &PaymentOption,
        secret_key: &VaultSecretKey,
        content_type: &VaultContentType,
    ) -> Result</* AttoTokens */ String> {
        let data = Bytes::copy_from_slice(&data);

        self.0
            .write_bytes_to_vault(
                data,
                payment_option.0.clone(),
                &secret_key.0,
                content_type.0,
            )
            .await
            .map(|c| c.to_string())
            .map_err(map_error)
    }

    // Registers

    /// Get the register history, starting from the root to the latest entry.
    ///
    /// This returns a RegisterHistory that can be use to get the register values from the history.
    ///
    /// RegisterHistory::next can be used to get the values one by one, from the first to the latest entry.
    /// RegisterHistory::collect can be used to get all the register values from the history from the first to the latest entry.
    #[napi]
    pub fn register_history(&self, addr: &RegisterAddress) -> RegisterHistory {
        let history = self.0.register_history(&addr.0);

        RegisterHistory(Mutex::new(history))
    }

    /// Create a new register key from a SecretKey and a name.
    ///
    /// This derives a new SecretKey from the owner’s SecretKey using the name. Note that you will need to keep track of the names you used to create the register key.
    #[napi]
    pub fn register_key_from_name(owner: &SecretKey, name: String) -> SecretKey {
        let key = autonomi::Client::register_key_from_name(&owner.0, &name);
        SecretKey(key)
    }

    /// Create a new RegisterValue from bytes, make sure the bytes are not longer than REGISTER_VALUE_SIZE
    #[napi]
    pub fn register_value_from_bytes(bytes: &[u8]) -> Result</* RegisterValue */ Uint8Array> {
        autonomi::Client::register_value_from_bytes(bytes)
            .map(Uint8Array::from)
            .map_err(map_error)
    }

    /// Create a new register with an initial value.
    ///
    /// Note that two payments are required, one for the underlying GraphEntry and one for the crate::Pointer
    #[napi]
    pub async fn register_create(
        &self,
        owner: &SecretKey,
        initial_value: /* RegisterValue */ Uint8Array,
        payment_option: &PaymentOption,
    ) -> Result</* (AttoTokens, RegisterAddress) */ tuple_result::RegisterCreate> {
        let initial_value: [u8; 32] = uint8_array_to_array(initial_value, "initial_value")?;

        let (cost, addr) = self
            .0
            .register_create(&owner.0, initial_value, payment_option.0.clone())
            .await
            .map_err(map_error)?;

        Ok(tuple_result::RegisterCreate { cost, addr })
    }

    /// Update the value of a register.
    ////
    /// The register needs to be created first with autonomi::Client::register_create
    #[napi]
    pub async fn register_update(
        &self,
        owner: &SecretKey,
        new_value: /* RegisterValue */ Uint8Array,
        payment_option: &PaymentOption,
    ) -> Result</* AttoTokens */ String> {
        let new_value: [u8; 32] = uint8_array_to_array(new_value, "new_value")?;
        self.0
            .register_update(&owner.0, new_value, payment_option.0.clone())
            .await
            .map(|c| c.to_string())
            .map_err(map_error)
    }

    /// Get the current value of the register
    #[napi]
    pub async fn register_get(
        &self,
        addr: &RegisterAddress,
    ) -> Result</* RegisterValue */ Uint8Array> {
        self.0
            .register_get(&addr.0)
            .await
            .map(Uint8Array::from)
            .map_err(map_error)
    }

    /// Get the cost of a register operation. Returns the cost of creation if it doesn’t exist, else returns the cost of an update
    #[napi]
    pub async fn register_cost(&self, owner: &PublicKey) -> Result</* AttoTokens */ String> {
        let cost = self
            .0
            .register_cost(&owner.0.clone())
            .await
            .map_err(map_error)?;

        Ok(cost.to_string())
    }

    // Quotes

    // /// Get raw quotes from nodes. These quotes do not include actual record prices. You will likely want to use get_store_quotes instead.
    // #[napi]
    // pub async fn get_raw_quotes(
    //     &self,
    //     data_type: DataTypes,
    //     content_addrs: impl Iterator<Item = (XorName, usize)>,
    // ) -> Vec<Result<(XorName, Vec<(PeerId, PaymentQuote)>)>> {
    //     todo!()
    // }

    // ///
    // #[napi]
    // pub async fn get_store_quotes(
    //     &self,
    //     data_type: DataTypes,
    //     content_addrs: impl Iterator<Item = (XorName, usize)>,
    // ) -> Result<StoreQuote> {
    //     todo!()
    // }
}

// These types exists because NAPI-RS does not support returning tuples.
pub mod tuple_result {
    use super::*;

    #[napi]
    pub struct ChunkPut {
        pub(crate) cost: AttoTokens,
        pub(crate) addr: autonomi::ChunkAddress, // Can not be `ChunkAddress` as NAPI-RS expects a reference in that case.
    }
    #[napi]
    impl ChunkPut {
        #[napi(getter)]
        pub fn cost(&self) -> String {
            self.cost.to_string()
        }
        #[napi(getter)]
        pub fn addr(&self) -> ChunkAddress {
            ChunkAddress(self.addr)
        }
    }

    #[napi]
    pub struct GraphEntryPut {
        pub(crate) cost: AttoTokens,
        pub(crate) addr: autonomi::GraphEntryAddress,
    }
    #[napi]
    impl GraphEntryPut {
        #[napi(getter)]
        pub fn cost(&self) -> String {
            self.cost.to_string()
        }
        #[napi(getter)]
        pub fn addr(&self) -> GraphEntryAddress {
            GraphEntryAddress(self.addr)
        }
    }

    #[napi]
    pub struct ScratchpadPut {
        pub(crate) cost: AttoTokens,
        pub(crate) addr: autonomi::ScratchpadAddress,
    }
    #[napi]
    impl ScratchpadPut {
        #[napi(getter)]
        pub fn cost(&self) -> String {
            self.cost.to_string()
        }
        #[napi(getter)]
        pub fn addr(&self) -> ScratchpadAddress {
            ScratchpadAddress(self.addr)
        }
    }

    #[napi]
    pub struct PointerPut {
        pub(crate) cost: AttoTokens,
        pub(crate) addr: autonomi::PointerAddress,
    }
    #[napi]
    impl PointerPut {
        #[napi(getter)]
        pub fn cost(&self) -> String {
            self.cost.to_string()
        }
        #[napi(getter)]
        pub fn addr(&self) -> PointerAddress {
            PointerAddress(self.addr)
        }
    }

    #[napi]
    pub struct DataPutResult {
        pub(crate) cost: AttoTokens,
        pub(crate) data_map: autonomi::data::private::DataMapChunk,
    }
    #[napi]
    impl DataPutResult {
        #[napi(getter)]
        pub fn cost(&self) -> String {
            self.cost.to_string()
        }
        #[napi(getter)]
        pub fn data_map(&self) -> DataMapChunk {
            DataMapChunk(self.data_map.clone())
        }
    }

    #[napi]
    pub struct DataPutPublicResult {
        pub(crate) cost: AttoTokens,
        pub(crate) addr: autonomi::data::DataAddress,
    }
    #[napi]
    impl DataPutPublicResult {
        #[napi(getter)]
        pub fn cost(&self) -> String {
            self.cost.to_string()
        }
        #[napi(getter)]
        pub fn addr(&self) -> DataAddress {
            DataAddress(self.addr)
        }
    }

    #[napi]
    pub struct ArchivePutResult {
        pub(crate) cost: AttoTokens,
        pub(crate) data_map: autonomi::files::archive_private::PrivateArchiveDataMap,
    }
    #[napi]
    impl ArchivePutResult {
        #[napi(getter)]
        pub fn cost(&self) -> String {
            self.cost.to_string()
        }
        #[napi(getter)]
        pub fn data_map(&self) -> PrivateArchiveDataMap {
            PrivateArchiveDataMap(self.data_map.clone())
        }
    }

    #[napi]
    pub struct ArchivePutPublicResult {
        pub(crate) cost: AttoTokens,
        pub(crate) addr: autonomi::data::DataAddress,
    }
    #[napi]
    impl ArchivePutPublicResult {
        #[napi(getter)]
        pub fn cost(&self) -> String {
            self.cost.to_string()
        }
        #[napi(getter)]
        pub fn addr(&self) -> DataAddress {
            DataAddress(self.addr)
        }
    }

    #[napi]
    pub struct DirContentUpload {
        pub(crate) cost: AttoTokens,
        pub(crate) archive: autonomi::files::PrivateArchive,
    }
    #[napi]
    impl DirContentUpload {
        #[napi(getter)]
        pub fn cost(&self) -> String {
            self.cost.to_string()
        }
        #[napi(getter)]
        pub fn archive(&self) -> PrivateArchive {
            PrivateArchive(self.archive.clone())
        }
    }

    #[napi]
    pub struct DirUpload {
        pub(crate) cost: AttoTokens,
        pub(crate) data_map: autonomi::data::private::DataMapChunk,
    }
    #[napi]
    impl DirUpload {
        #[napi(getter)]
        pub fn cost(&self) -> String {
            self.cost.to_string()
        }
        #[napi(getter)]
        pub fn data_map(&self) -> DataMapChunk {
            DataMapChunk(self.data_map.clone())
        }
    }

    #[napi]
    pub struct FileContentUpload {
        pub(crate) cost: AttoTokens,
        pub(crate) data_map: autonomi::data::private::DataMapChunk,
    }
    #[napi]
    impl FileContentUpload {
        #[napi(getter)]
        pub fn cost(&self) -> String {
            self.cost.to_string()
        }
        #[napi(getter)]
        pub fn data_map(&self) -> DataMapChunk {
            DataMapChunk(self.data_map.clone())
        }
    }

    #[napi]
    pub struct DirContentUploadPublic {
        pub(crate) cost: AttoTokens,
        pub(crate) archive: autonomi::files::PublicArchive,
    }
    #[napi]
    impl DirContentUploadPublic {
        #[napi(getter)]
        pub fn cost(&self) -> String {
            self.cost.to_string()
        }
        #[napi(getter)]
        pub fn addr(&self) -> PublicArchive {
            PublicArchive(self.archive.clone())
        }
    }

    #[napi]
    pub struct DirUploadPublic {
        pub(crate) cost: AttoTokens,
        pub(crate) addr: autonomi::files::archive_public::ArchiveAddress,
    }
    #[napi]
    impl DirUploadPublic {
        #[napi(getter)]
        pub fn cost(&self) -> String {
            self.cost.to_string()
        }
        #[napi(getter)]
        pub fn addr(&self) -> ArchiveAddress {
            ArchiveAddress(self.addr)
        }
    }

    #[napi]
    pub struct FileContentUploadPublic {
        pub(crate) cost: AttoTokens,
        pub(crate) addr: autonomi::PointerAddress,
    }
    #[napi]
    impl FileContentUploadPublic {
        #[napi(getter)]
        pub fn cost(&self) -> String {
            self.cost.to_string()
        }
        #[napi(getter)]
        pub fn addr(&self) -> PointerAddress {
            PointerAddress(self.addr)
        }
    }

    #[napi]
    pub struct FetchAndDecryptVault {
        pub(crate) data: Bytes,
        pub(crate) content_type: autonomi::vault::VaultContentType,
    }
    #[napi]
    impl FetchAndDecryptVault {
        #[napi(getter)]
        pub fn data(&self) -> Buffer {
            Buffer::from(self.data.as_ref())
        }
        #[napi(getter)]
        pub fn content_type(&self) -> u64 {
            self.content_type
        }
    }

    #[napi]
    pub struct RegisterCreate {
        pub(crate) cost: AttoTokens,
        pub(crate) addr: autonomi::register::RegisterAddress,
    }
    #[napi]
    impl RegisterCreate {
        #[napi(getter)]
        pub fn cost(&self) -> String {
            self.cost.to_string()
        }
        #[napi(getter)]
        pub fn addr(&self) -> RegisterAddress {
            RegisterAddress(self.addr)
        }
    }

    #[napi]
    pub struct GraphEntryDescendant {
        pub(crate) public_key: autonomi::PublicKey,
        pub(crate) content: [u8; 32],
    }
    #[napi]
    impl GraphEntryDescendant {
        #[napi(getter)]
        pub fn public_key(&self) -> PublicKey {
            PublicKey(self.public_key)
        }
        #[napi(getter)]
        pub fn content(&self) -> Uint8Array {
            Uint8Array::from(self.content.as_ref())
        }
    }

    #[napi(object)]
    pub struct ArchiveFile {
        pub path: String,
        pub created: BigInt,
        pub modified: BigInt,
        pub size: BigInt,
        pub extra: Option<String>,
    }
}

/// A 256-bit number, viewed as a point in XOR space.
///
/// This wraps an array of 32 bytes, i. e. a number between 0 and 2<sup>256</sup> - 1.
///
/// XOR space is the space of these numbers, with the [XOR metric][1] as a notion of distance,
/// i. e. the points with IDs `x` and `y` are considered to have distance `x xor y`.
///
/// [1]: https://en.wikipedia.org/wiki/Kademlia#System_details
#[napi]
pub struct XorName(autonomi::XorName);
#[napi]
impl XorName {
    /// Generate a XorName for the given content.
    #[napi(factory)]
    pub fn from_content(content: &[u8]) -> Self {
        Self(autonomi::XorName::from_content_parts(&[content]))
    }

    /// Generate a random XorName
    #[napi(factory)]
    pub fn random() -> Self {
        Self(autonomi::XorName::random(&mut rand::thread_rng()))
    }
}

/// Address of a chunk.
///
/// It is derived from the content of the chunk.
#[napi]
pub struct ChunkAddress(autonomi::ChunkAddress);

#[napi]
impl ChunkAddress {
    /// Creates a new ChunkAddress.
    #[napi(constructor)]
    pub fn new(xor_name: &XorName) -> Self {
        Self(autonomi::ChunkAddress::new(xor_name.0))
    }

    /// Returns the XorName.
    #[napi]
    pub fn xorname(&self) -> XorName {
        XorName(*self.0.xorname())
    }

    /// Returns the hex string representation of the address.
    #[napi]
    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }

    /// Creates a new ChunkAddress from a hex string.
    #[napi(factory)]
    pub fn from_hex(hex: String) -> Result<Self> {
        let addr = autonomi::ChunkAddress::from_hex(&hex).map_err(map_error)?;

        Ok(Self(addr))
    }
}

/// Address of a `GraphEntry`.
///
/// It is derived from the owner's unique public key
#[napi]
pub struct GraphEntryAddress(autonomi::GraphEntryAddress);

#[napi]
impl GraphEntryAddress {
    /// Creates a new GraphEntryAddress.
    #[napi(constructor)]
    pub fn new(owner: &PublicKey) -> Self {
        Self(autonomi::GraphEntryAddress::new(owner.0))
    }

    /// Return the network name of the scratchpad.
    /// This is used to locate the scratchpad on the network.
    #[napi]
    pub fn xorname(&self) -> XorName {
        XorName(self.0.xorname())
    }

    /// Serialize this `GraphEntryAddress` into a hex-encoded string.
    #[napi]
    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }

    /// Parse a hex-encoded string into a `GraphEntryAddress`.
    #[napi(factory)]
    pub fn from_hex(hex: String) -> Result<Self> {
        let addr = autonomi::GraphEntryAddress::from_hex(&hex).map_err(map_error)?;

        Ok(Self(addr))
    }
}

#[napi]
pub struct DataAddress(autonomi::data::DataAddress);

#[napi]
impl DataAddress {
    /// Creates a new DataAddress.
    #[napi(constructor)]
    pub fn new(xor_name: &XorName) -> Self {
        Self(autonomi::data::DataAddress::new(xor_name.0))
    }

    /// Returns the XorName.
    #[napi]
    pub fn xorname(&self) -> XorName {
        XorName(*self.0.xorname())
    }

    /// Returns the hex string representation of the address.
    #[napi]
    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }

    /// Creates a new DataAddress from a hex string.
    #[napi(factory)]
    pub fn from_hex(hex: String) -> Result<Self> {
        autonomi::data::DataAddress::from_hex(&hex)
            .map(Self)
            .map_err(map_error)
    }
}

#[napi]
pub struct ArchiveAddress(autonomi::files::archive_public::ArchiveAddress);

#[napi]
impl ArchiveAddress {
    /// Creates a new ArchiveAddress.
    #[napi(constructor)]
    pub fn new(xor_name: &XorName) -> Self {
        Self(autonomi::files::archive_public::ArchiveAddress::new(
            xor_name.0,
        ))
    }

    /// Returns the XorName.
    #[napi]
    pub fn xorname(&self) -> XorName {
        XorName(*self.0.xorname())
    }

    /// Returns the hex string representation of the address.
    #[napi]
    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }

    /// Creates a new ArchiveAddress from a hex string.
    #[napi(factory)]
    pub fn from_hex(hex: String) -> Result<Self> {
        autonomi::files::archive_public::ArchiveAddress::from_hex(&hex)
            .map(Self)
            .map_err(map_error)
    }
}

/// A wallet for interacting with the network's payment system
#[napi]
pub struct Wallet(autonomi::Wallet);

#[napi]
impl Wallet {
    /// Convenience function that creates a new Wallet with a random EthereumWallet.
    pub fn new_with_random_wallet(network: autonomi::Network) -> Self {
        Wallet(autonomi::Wallet::new_with_random_wallet(network))
    }

    /// Creates a new Wallet based on the given Ethereum private key. It will fail with Error::PrivateKeyInvalid if private_key is invalid.
    #[napi(factory)]
    pub fn new_from_private_key(network: &Network, private_key: String) -> Result<Self> {
        let wallet = autonomi::Wallet::new_from_private_key(network.0.clone(), &private_key)
            .map_err(map_error)?;

        Ok(Self(wallet))
    }

    /// Returns a string representation of the wallet's address
    #[napi]
    pub fn address(&self) -> String {
        self.0.address().to_string()
    }

    /// Returns the `Network` of this wallet.
    pub fn network(&self) -> Network {
        Network(self.0.network().clone())
    }

    /// Returns the raw balance of payment tokens in the wallet
    #[napi]
    pub async fn balance(&self) -> Result<String> {
        let balance = self.0.balance_of_tokens().await.map_err(map_error)?;

        Ok(balance.to_string())
    }

    /// Returns the current balance of gas tokens in the wallet
    #[napi]
    pub async fn balance_of_gas(&self) -> Result<String> {
        let balance = self.0.balance_of_gas_tokens().await.map_err(map_error)?;

        Ok(balance.to_string())
    }
}

/// Options for making payments on the network
#[napi]
pub struct PaymentOption(autonomi::client::payment::PaymentOption);

#[napi]
impl PaymentOption {
    #[napi(factory)]
    pub fn from_wallet(wallet: &Wallet) -> Self {
        Self(autonomi::client::payment::PaymentOption::from(&wallet.0))
    }

    #[napi(factory)]
    pub fn from_receipt() -> Self {
        unimplemented!()
    }
}

#[napi]
pub struct Network(autonomi::Network);

#[napi]
impl Network {
    #[napi(constructor)]
    pub fn new(local: bool) -> Result<Self> {
        let network = autonomi::Network::new(local).map_err(map_error)?;
        Ok(Self(network))
    }
}

#[napi]
pub struct PublicKey(autonomi::PublicKey);

#[napi]
impl PublicKey {
    /// Returns a byte string representation of the public key.
    #[napi]
    pub fn to_bytes(&self) -> Uint8Array {
        Uint8Array::from(self.0.to_bytes())
    }

    /// Returns the key with the given representation, if valid.
    #[napi(factory)]
    pub fn from_bytes(bytes: Uint8Array) -> Result<Self> {
        let bytes = uint8_array_to_array(bytes, "bytes")?;
        let key = autonomi::PublicKey::from_bytes(bytes).map_err(map_error)?;
        Ok(Self(key))
    }

    /// Returns the hex string representation of the public key.
    #[napi]
    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }

    /// Creates a new PublicKey from a hex string.
    #[napi(factory)]
    pub fn from_hex(hex: String) -> Result<Self> {
        let key = autonomi::PublicKey::from_hex(&hex).map_err(map_error)?;
        Ok(Self(key))
    }
}

#[napi]
pub struct SecretKey(autonomi::SecretKey);

#[napi]
impl SecretKey {
    /// Generate a random SecretKey
    #[napi(factory)]
    pub fn random() -> Self {
        Self(autonomi::SecretKey::random())
    }

    /// Returns the public key corresponding to this secret key.
    #[napi]
    pub fn public_key(&self) -> PublicKey {
        PublicKey(self.0.public_key())
    }

    /// Converts the secret key to big endian bytes
    #[napi]
    pub fn to_bytes(&self) -> Uint8Array {
        Uint8Array::from(self.0.to_bytes())
    }

    /// Deserialize from big endian bytes
    #[napi(factory)]
    pub fn from_bytes(bytes: Uint8Array) -> Result<Self> {
        let bytes = uint8_array_to_array(bytes, "bytes")?;
        let key = autonomi::SecretKey::from_bytes(bytes).map_err(map_error)?;
        Ok(Self(key))
    }

    /// Returns the hex string representation of the secret key.
    #[napi]
    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }

    /// Creates a new SecretKey from a hex string.
    #[napi(factory)]
    pub fn from_hex(hex: String) -> Result<Self> {
        let key = autonomi::SecretKey::from_hex(&hex).map_err(map_error)?;
        Ok(Self(key))
    }
}

#[napi]
pub struct GraphEntry(autonomi::GraphEntry);

#[napi]
impl GraphEntry {
    /// Create a new graph entry, signing it with the provided secret key.
    #[napi(constructor)]
    pub fn new(
        owner: &SecretKey,
        parents: Vec<&PublicKey>,
        content: Uint8Array,
        descendants: Vec<(&PublicKey, Uint8Array)>,
    ) -> Result<Self> {
        let content: [u8; 32] = uint8_array_to_array(content, "content")?;

        let parents = parents.iter().map(|p| p.0).collect();

        let descendants = descendants
            .iter()
            .map(|(pk, content)| {
                let content_array: [u8; 32] = uint8_array_to_array(content.clone(), "content")?;
                Ok((pk.0, content_array))
            })
            .collect::<Result<Vec<(autonomi::PublicKey, [u8; 32])>>>()?;

        Ok(Self(autonomi::GraphEntry::new(
            &owner.0,
            parents,
            content,
            descendants,
        )))
    }

    /// Create a new graph entry with the signature already calculated.
    #[napi(factory)]
    pub fn new_with_signature(
        owner: &PublicKey,
        parents: Vec<&PublicKey>,
        content: Uint8Array,
        descendants: Vec<(&PublicKey, Uint8Array)>,
        signature: Uint8Array,
    ) -> Result<Self> {
        let content: [u8; 32] = uint8_array_to_array(content, "content")?;

        let parents = parents.iter().map(|p| p.0).collect();

        let descendants_result: Result<Vec<(autonomi::PublicKey, [u8; 32])>> = descendants
            .iter()
            .map(|(pk, content)| {
                let content_array: [u8; 32] = uint8_array_to_array(content.clone(), "content")?;
                Ok((pk.0, content_array))
            })
            .collect();

        let descendants = descendants_result?;

        let signature = uint8_array_to_array(signature, "signature")?;
        let signature = Signature::from_bytes(signature).map_err(map_error)?;

        Ok(Self(autonomi::GraphEntry::new_with_signature(
            owner.0,
            parents,
            content,
            descendants,
            signature,
        )))
    }

    /// Get the address of the graph entry
    #[napi]
    pub fn address(&self) -> GraphEntryAddress {
        GraphEntryAddress(self.0.address())
    }

    /// Get the owner of the graph entry
    #[napi]
    pub fn owner(&self) -> PublicKey {
        PublicKey(self.0.owner)
    }

    /// Get the parents of the graph entry
    #[napi]
    pub fn parents(&self) -> Vec<PublicKey> {
        self.0.parents.iter().map(|p| PublicKey(*p)).collect()
    }

    /// Get the content of the graph entry
    #[napi]
    pub fn content(&self) -> Buffer {
        Buffer::from(self.0.content.to_vec())
    }

    /// Get the descendants of the graph entry
    #[napi]
    pub fn descendants(&self) -> Vec<tuple_result::GraphEntryDescendant> {
        self.0
            .descendants
            .iter()
            .map(|(pk, data)| tuple_result::GraphEntryDescendant {
                public_key: *pk,
                content: *data,
            })
            .collect()
    }

    /// Get the bytes that were signed for this graph entry
    #[napi]
    pub fn bytes_for_signature(&self) -> Buffer {
        Buffer::from(self.0.bytes_for_signature())
    }

    /// Verifies if the graph entry has a valid signature
    #[napi]
    pub fn verify_signature(&self) -> bool {
        self.0.verify_signature()
    }

    /// Size of the graph entry
    #[napi]
    pub fn size(&self) -> usize {
        self.0.size()
    }

    #[napi(getter)]
    pub fn signature(&self) -> Uint8Array {
        Uint8Array::from(self.0.signature.to_bytes())
    }

    /// Returns true if the graph entry is too big
    #[napi]
    pub fn is_too_big(&self) -> bool {
        self.0.is_too_big()
    }
}

#[napi]
pub struct Pointer(autonomi::Pointer);

#[napi]
impl Pointer {
    /// Create a new pointer, signing it with the provided secret key.
    /// This pointer would be stored on the network at the provided key's public key.
    /// There can only be one pointer at a time at the same address (one per key).
    #[napi(constructor)]
    pub fn new(owner: &SecretKey, counter: u32, target: &PointerTarget) -> Self {
        Pointer(autonomi::Pointer::new(&owner.0, counter, target.0.clone()))
    }

    /// Get the address of the pointer
    #[napi]
    pub fn address(&self) -> PointerAddress {
        PointerAddress(self.0.address())
    }

    /// Get the owner of the pointer
    #[napi]
    pub fn owner(&self) -> PublicKey {
        PublicKey(*self.0.owner())
    }

    /// Get the target of the pointer
    #[napi]
    pub fn target(&self) -> PointerTarget {
        PointerTarget(self.0.target().clone())
    }

    /// Get the bytes that were signed for this pointer
    #[napi]
    pub fn bytes_for_signature(&self) -> Buffer {
        Buffer::from(self.0.bytes_for_signature())
    }

    /// Get the xorname of the pointer target
    #[napi]
    pub fn xorname(&self) -> XorName {
        XorName(self.0.xorname())
    }

    /// Get the counter of the pointer, the higher the counter, the more recent the pointer is
    /// Similarly to counter CRDTs only the latest version (highest counter) of the pointer is kept on the network
    #[napi]
    pub fn counter(&self) -> u32 {
        self.0.counter()
    }

    /// Verifies if the pointer has a valid signature
    #[napi]
    pub fn verify_signature(&self) -> bool {
        self.0.verify_signature()
    }

    /// Size of the pointer
    #[napi]
    pub fn size() -> usize {
        autonomi::Pointer::size()
    }
}

#[napi]
pub struct PointerTarget(autonomi::pointer::PointerTarget);

#[napi]
impl PointerTarget {
    /// Returns the xorname of the target
    #[napi]
    pub fn xorname(&self) -> XorName {
        XorName(self.0.xorname())
    }

    /// Returns the hex string representation of the target
    #[napi]
    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }

    /// Creates a new PointerTarget from a ChunkAddress
    #[napi(factory, js_name = "ChunkAddress")]
    pub fn from_chunk_address(addr: &ChunkAddress) -> Self {
        Self(autonomi::pointer::PointerTarget::ChunkAddress(addr.0))
    }

    /// Creates a new PointerTarget from a GraphEntryAddress
    #[napi(factory, js_name = "GraphEntryAddress")]
    pub fn from_graph_entry_address(addr: &GraphEntryAddress) -> Self {
        Self(autonomi::pointer::PointerTarget::GraphEntryAddress(addr.0))
    }

    /// Creates a new PointerTarget from a PointerAddress
    #[napi(factory, js_name = "PointerAddress")]
    pub fn from_pointer_address(addr: &PointerAddress) -> Self {
        Self(autonomi::pointer::PointerTarget::PointerAddress(addr.0))
    }

    /// Creates a new PointerTarget from a ScratchpadAddress
    #[napi(factory, js_name = "ScratchpadAddress")]
    pub fn from_scratchpad_address(addr: &ScratchpadAddress) -> Self {
        Self(autonomi::pointer::PointerTarget::ScratchpadAddress(addr.0))
    }
}

#[napi]
pub struct PointerAddress(autonomi::PointerAddress);

#[napi]
impl PointerAddress {
    /// Creates a new PointerAddress.
    #[napi(constructor)]
    pub fn new(owner: &PublicKey) -> Self {
        Self(autonomi::PointerAddress::new(owner.0))
    }

    /// Return the network name of the pointer.
    /// This is used to locate the pointer on the network.
    #[napi]
    pub fn xorname(&self) -> XorName {
        XorName(self.0.xorname())
    }

    /// Return the owner.
    #[napi]
    pub fn owner(&self) -> PublicKey {
        PublicKey(*self.0.owner())
    }

    /// Serialize this PointerAddress into a hex-encoded string.
    #[napi]
    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }

    /// Parse a hex-encoded string into a PointerAddress.
    #[napi(factory)]
    pub fn from_hex(hex: String) -> Result<Self> {
        let addr = autonomi::PointerAddress::from_hex(&hex).map_err(map_error)?;
        Ok(Self(addr))
    }
}

#[napi]
pub struct Scratchpad(autonomi::Scratchpad);

#[napi]
impl Scratchpad {
    /// Create a new scratchpad, signing it with the provided secret key.
    #[napi(constructor)]
    pub fn new(
        owner: &SecretKey,
        data_encoding: BigInt, // `u64`
        data: Buffer,
        counter: BigInt, // `u64`
    ) -> Result<Self> {
        let data_encoding = big_int_to_u64(data_encoding, "data_encoding")?;
        let counter = big_int_to_u64(counter, "counter")?;
        let data = Bytes::copy_from_slice(&data);

        Ok(Self(autonomi::Scratchpad::new(
            &owner.0,
            data_encoding,
            &data,
            counter,
        )))
    }

    /// Get the address of the scratchpad
    #[napi]
    pub fn address(&self) -> ScratchpadAddress {
        ScratchpadAddress(*self.0.address())
    }

    /// Get the owner of the scratchpad
    #[napi]
    pub fn owner(&self) -> PublicKey {
        PublicKey(*self.0.owner())
    }

    /// Get the data encoding (content type) of the scratchpad
    #[napi]
    pub fn data_encoding(&self) -> u64 {
        self.0.data_encoding()
    }

    /// Decrypt the data of the scratchpad
    #[napi]
    pub fn decrypt_data(&self, key: &SecretKey) -> Result<Buffer> {
        let data = self.0.decrypt_data(&key.0).map_err(map_error)?;
        Ok(Buffer::from(data.to_vec()))
    }

    /// Get the counter of the scratchpad
    #[napi]
    pub fn counter(&self) -> u64 {
        self.0.counter()
    }

    /// Verify the signature of the scratchpad
    #[napi]
    pub fn verify_signature(&self) -> bool {
        self.0.verify_signature()
    }
}

#[napi]
pub struct ScratchpadAddress(autonomi::ScratchpadAddress);

#[napi]
impl ScratchpadAddress {
    /// Creates a new ScratchpadAddress.
    #[napi(constructor)]
    pub fn new(owner: &PublicKey) -> Self {
        Self(autonomi::ScratchpadAddress::new(owner.0))
    }

    /// Return the network name of the scratchpad.
    /// This is used to locate the scratchpad on the network.
    #[napi]
    pub fn xorname(&self) -> XorName {
        XorName(self.0.xorname())
    }

    /// Return the owner.
    #[napi]
    pub fn owner(&self) -> PublicKey {
        PublicKey(*self.0.owner())
    }

    /// Serialize this ScratchpadAddress into a hex-encoded string.
    #[napi]
    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }

    /// Parse a hex-encoded string into a ScratchpadAddress.
    #[napi(factory)]
    pub fn from_hex(hex: String) -> Result<Self> {
        let addr = autonomi::ScratchpadAddress::from_hex(&hex).map_err(map_error)?;
        Ok(Self(addr))
    }
}

#[napi]
pub struct DataMapChunk(autonomi::data::private::DataMapChunk);

#[napi]
pub struct PrivateArchiveDataMap(autonomi::files::archive_private::PrivateArchiveDataMap);

#[napi]
impl PrivateArchiveDataMap {
    /// Serialize this PrivateArchiveDataMap into a hex-encoded string.
    #[napi]
    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }

    /// Parse a hex-encoded string into a PrivateArchiveDataMap.
    #[napi(factory)]
    pub fn from_hex(hex: String) -> Result<Self> {
        let data_map = autonomi::files::archive_private::PrivateArchiveDataMap::from_hex(&hex)
            .map_err(map_error)?;
        Ok(Self(data_map))
    }
}

#[napi]
pub struct PrivateArchive(autonomi::files::PrivateArchive);

#[napi]
impl PrivateArchive {
    /// Create a new empty local archive
    #[napi(constructor)]
    #[allow(clippy::new_without_default, reason = "`Default` not useful")]
    pub fn new() -> Self {
        Self(autonomi::files::PrivateArchive::new())
    }

    /// Add a file to a local archive
    #[napi]
    pub fn add_file(&mut self, path: String, data_map: &DataMapChunk, metadata: &Metadata) {
        self.0
            .add_file(PathBuf::from(path), data_map.0.clone(), metadata.0.clone());
    }

    /// Rename a file in an archive
    #[napi]
    pub fn rename_file(&mut self, old_path: String, new_path: String) -> Result<()> {
        self.0
            .rename_file(&PathBuf::from(old_path), &PathBuf::from(new_path))
            .map_err(map_error)
    }

    /// List all files in the archive with their metadata
    #[napi]
    pub fn files(&self) -> Vec<tuple_result::ArchiveFile> {
        self.0
            .files()
            .into_iter()
            .map(|(path, meta)| tuple_result::ArchiveFile {
                path: path.to_string_lossy().to_string(),
                created: BigInt::from(meta.created),
                modified: BigInt::from(meta.modified),
                size: BigInt::from(meta.size),
                extra: meta.extra.clone(),
            })
            .collect()
    }

    /// List all data maps of the files in the archive
    #[napi]
    pub fn data_maps(&self) -> Vec<DataMapChunk> {
        self.0.data_maps().into_iter().map(DataMapChunk).collect()
    }

    /// Convert the archive to bytes
    #[napi]
    pub fn to_bytes(&self) -> Result<Buffer> {
        let bytes = self.0.to_bytes().map_err(|e| {
            napi::Error::new(
                Status::GenericFailure,
                format!("Failed to serialize archive: {e:?}"),
            )
        })?;

        Ok(Buffer::from(bytes.to_vec()))
    }

    /// Create an archive from bytes
    #[napi(factory)]
    pub fn from_bytes(data: Buffer) -> Result<Self> {
        let bytes = Bytes::from(data.as_ref().to_vec());
        let archive = autonomi::files::PrivateArchive::from_bytes(bytes).map_err(|e| {
            napi::Error::new(
                Status::GenericFailure,
                format!("Failed to deserialize archive: {e:?}"),
            )
        })?;

        Ok(Self(archive))
    }

    /// Merge with another archive
    #[napi]
    pub fn merge(&mut self, other: &PrivateArchive) {
        self.0.merge(&other.0);
    }
}

#[napi]
pub struct VaultSecretKey(autonomi::vault::VaultSecretKey);

#[napi]
pub struct UserData(autonomi::vault::UserData);

#[napi]
pub struct VaultContentType(autonomi::vault::VaultContentType);

/// File metadata
#[napi]
pub struct Metadata(autonomi::files::Metadata);

#[napi]
impl Metadata {
    /// Create a new metadata struct with the current time as uploaded, created and modified.
    #[napi(factory)]
    pub fn new_with_size(size: BigInt) -> Result<Self> {
        let size = big_int_to_u64(size, "size")?;
        Ok(Self(autonomi::files::Metadata::new_with_size(size)))
    }

    /// Create new metadata with all custom fields
    #[napi(factory)]
    pub fn with_custom_fields(
        created: BigInt,
        modified: BigInt,
        size: BigInt,
        extra: Option<String>,
    ) -> Result<Self> {
        let created = big_int_to_u64(created, "created")?;
        let modified = big_int_to_u64(modified, "modified")?;
        let size = big_int_to_u64(size, "size")?;

        Ok(Self(autonomi::files::Metadata {
            created,
            modified,
            size,
            extra,
        }))
    }

    /// Create a new empty metadata struct with zeros
    #[napi(factory)]
    pub fn empty() -> Self {
        Self(autonomi::files::Metadata::empty())
    }

    /// Get the creation timestamp
    #[napi(getter)]
    pub fn created(&self) -> u64 {
        self.0.created
    }

    /// Get the modification timestamp
    #[napi(getter)]
    pub fn modified(&self) -> u64 {
        self.0.modified
    }

    /// Get the file size
    #[napi(getter)]
    pub fn size(&self) -> u64 {
        self.0.size
    }

    /// Get the extra metadata
    #[napi(getter)]
    pub fn extra(&self) -> Option<String> {
        self.0.extra.clone()
    }
}

#[napi]
pub struct RegisterAddress(autonomi::register::RegisterAddress);

#[napi]
impl RegisterAddress {
    /// Creates a new RegisterAddress.
    #[napi(constructor)]
    pub fn new(owner: &PublicKey) -> Self {
        Self(autonomi::register::RegisterAddress::new(owner.0))
    }

    /// Get the owner of the register
    #[napi]
    pub fn owner(&self) -> PublicKey {
        PublicKey(self.0.owner())
    }

    /// Get the underlying graph root address
    #[napi]
    pub fn to_underlying_graph_root(&self) -> GraphEntryAddress {
        GraphEntryAddress(self.0.to_underlying_graph_root())
    }

    /// Get the underlying head pointer address
    #[napi]
    pub fn to_underlying_head_pointer(&self) -> PointerAddress {
        PointerAddress(self.0.to_underlying_head_pointer())
    }

    /// Serialize this RegisterAddress into a hex-encoded string.
    #[napi]
    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }

    /// Parse a hex-encoded string into a RegisterAddress.
    #[napi(factory)]
    pub fn from_hex(hex: String) -> Result<Self> {
        let addr = autonomi::register::RegisterAddress::from_hex(&hex).map_err(map_error)?;
        Ok(Self(addr))
    }
}

#[napi]
pub struct RegisterHistory(Mutex<autonomi::register::RegisterHistory>);

#[napi]
impl RegisterHistory {
    // Somehow without this stub, NAPI-RS fails to create this object with an error:
    // error: `Failed to get constructor of class`
    #[allow(clippy::new_without_default, reason = "`Default` not useful")]
    #[napi(constructor)]
    pub fn new() -> Self {
        unimplemented!()
    }

    /// Fetch and go to the next register value from the history.
    ///
    /// Returns null when we reached the end.
    #[napi]
    pub async fn next(&self) -> Result<Option<Uint8Array>> {
        self.0
            .lock()
            .await
            .next()
            .await
            .map(|v| v.map(Uint8Array::from))
            .map_err(map_error)
    }

    /// Get all the register values from the history, starting from the first to the latest entry
    #[napi]
    pub async fn collect(&self) -> Result<Vec<Uint8Array>> {
        let values = self.0.lock().await.collect().await.map_err(map_error)?;
        let values = values.into_iter().map(Uint8Array::from).collect();
        Ok(values)
    }
}

#[napi]
pub struct PublicArchive(autonomi::files::PublicArchive);

#[napi]
impl PublicArchive {
    /// Create a new empty local archive
    #[napi(constructor)]
    #[allow(clippy::new_without_default, reason = "`Default` not useful")]
    pub fn new() -> Self {
        Self(autonomi::files::PublicArchive::new())
    }

    /// Add a file to a local archive
    #[napi]
    pub fn add_file(&mut self, path: String, data_addr: &DataAddress, metadata: &Metadata) {
        self.0
            .add_file(PathBuf::from(path), data_addr.0, metadata.0.clone());
    }

    /// Rename a file in an archive
    #[napi]
    pub fn rename_file(&mut self, old_path: String, new_path: String) -> Result<()> {
        self.0
            .rename_file(&PathBuf::from(old_path), &PathBuf::from(new_path))
            .map_err(map_error)
    }

    /// List all files in the archive with their metadata
    #[napi]
    pub fn files(&self) -> Vec<tuple_result::ArchiveFile> {
        self.0
            .files()
            .into_iter()
            .map(|(path, meta)| tuple_result::ArchiveFile {
                path: path.to_string_lossy().to_string(),
                created: BigInt::from(meta.created),
                modified: BigInt::from(meta.modified),
                size: BigInt::from(meta.size),
                extra: meta.extra.clone(),
            })
            .collect()
    }

    /// List all data addresses of the files in the archive
    #[napi]
    pub fn addresses(&self) -> Vec<DataAddress> {
        self.0.addresses().into_iter().map(DataAddress).collect()
    }

    /// Convert the archive to bytes
    #[napi]
    pub fn to_bytes(&self) -> Result<Buffer> {
        let bytes = self.0.to_bytes().map_err(|e| {
            napi::Error::new(
                Status::GenericFailure,
                format!("Failed to serialize archive: {e:?}"),
            )
        })?;

        Ok(Buffer::from(bytes.to_vec()))
    }

    /// Create an archive from bytes
    #[napi(factory)]
    pub fn from_bytes(data: Buffer) -> Result<Self> {
        let bytes = Bytes::from(data.as_ref().to_vec());
        let archive = autonomi::files::PublicArchive::from_bytes(bytes).map_err(|e| {
            napi::Error::new(
                Status::GenericFailure,
                format!("Failed to deserialize archive: {e:?}"),
            )
        })?;

        Ok(Self(archive))
    }

    /// Merge with another archive
    #[napi]
    pub fn merge(&mut self, other: &PublicArchive) {
        self.0.merge(&other.0);
    }
}
