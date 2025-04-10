// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

pub mod key;
pub mod user_data;

pub use key::{derive_vault_key, VaultSecretKey};
pub use user_data::UserData;

use crate::client::data_types::scratchpad::ScratchpadError;
use crate::client::high_level::files::FILE_UPLOAD_BATCH_SIZE;
use crate::client::key_derivation::{DerivationIndex, MainSecretKey};
use crate::client::payment::PaymentOption;
use crate::client::quote::CostError;
use crate::client::utils::process_tasks_with_max_concurrency;
use crate::client::Client;
use crate::graph::GraphError;
use ant_evm::{AttoTokens, U256};
use ant_networking::{GetRecordError, NetworkError};
use ant_protocol::storage::{
    GraphContent, GraphEntry, GraphEntryAddress, Scratchpad, ScratchpadAddress,
};
use ant_protocol::Bytes;
use bls::PublicKey;
use std::hash::{DefaultHasher, Hash, Hasher};
use tracing::info;

/// The content type of the vault data
/// The number is used to determine the type of the contents of the bytes contained in a vault
/// Custom apps can use this to store their own custom types of data in vaults
/// It is recommended to use the hash of the app name or an unique identifier as the content type using [`app_name_to_vault_content_type`]
/// The value 0 is reserved for tests
pub type VaultContentType = u64;

/// Defines the max size of content can be written into per ScratchPad
const MAX_CONTENT_PER_SCRATCHPAD: usize = Scratchpad::MAX_SIZE - 1024;

/// Defines the max number of Scratchpads that one GraphEntry can point to
/// The current value is assuming GraphEntry max_size to be 100KB.
const NUM_OF_SCRATCHPADS_PER_GRAPHENTRY: usize = 1_000;

/// Hard coded derivation index for the Vault's root GraphEntry.
/// Derive the Vault's main secret/public key by it to get the root GraphEntry owner/address
const VAULT_HEAD_DERIVATION_INDEX: [u8; 32] = [0; 32];

/// For custom apps using Vault, this function converts an app identifier or name to a [`VaultContentType`]
pub fn app_name_to_vault_content_type<T: Hash>(s: T) -> VaultContentType {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("Vault Scratchpad related error: {0}")]
    Scratchpad(#[from] ScratchpadError),
    #[error("Vault GraphEntry related error: {0}")]
    GraphEntry(#[from] GraphError),
    #[error("Vault Cost related error: {0}")]
    Cost(#[from] CostError),
    #[error("Protocol: {0}")]
    Protocol(#[from] ant_protocol::Error),
    #[error("Vault doesn't have enough graph descendants: {0}")]
    VaultNotEnoughGraphDescendants(String),
    #[error("Vault with empty content")]
    VaultWithZeroContentSize,
}

impl Client {
    /// Retrieves and returns a decrypted vault if one exists.
    ///
    /// Returns the content type of the bytes in the vault.
    pub async fn fetch_and_decrypt_vault(
        &self,
        secret_key: &VaultSecretKey,
    ) -> Result<(Bytes, VaultContentType), VaultError> {
        info!("Fetching and decrypting vault...");
        let main_secret_key = MainSecretKey::new(secret_key.clone());
        let public_key = main_secret_key
            .derive_key(&DerivationIndex::from_bytes(VAULT_HEAD_DERIVATION_INDEX))
            .public_key();

        let mut cur_graph_entry_addr = GraphEntryAddress::new(public_key.into());
        let mut decrypted_full_text = vec![];
        let mut content_type = 0;
        let mut has_end_reached = false;

        while !has_end_reached {
            let graph_entry = self.graph_entry_get(&cur_graph_entry_addr).await?;

            // The first descendant is reserved for `expand GraphEntry`.
            match graph_entry.descendants.split_first() {
                Some((&(first, _), rest)) => {
                    cur_graph_entry_addr = GraphEntryAddress::new(first);
                    let scratchpad_addresses = rest.to_vec();

                    let (decrypt_data, cur_content_type, is_end_reached) = self
                        .fetch_scratchpads_of_one_graph_entry_and_decrypt(
                            &main_secret_key,
                            scratchpad_addresses,
                        )
                        .await?;
                    decrypted_full_text.push(decrypt_data);
                    content_type = cur_content_type;
                    has_end_reached = is_end_reached;
                }
                None => {
                    let msg = format!(
                        "Vault's GraphEntry at {cur_graph_entry_addr:?} only has {} descendants.",
                        graph_entry.descendants.len()
                    );
                    return Err(VaultError::VaultNotEnoughGraphDescendants(msg));
                }
            }
        }

        debug!("vault data is successfully fetched and decrypted");
        Ok((Bytes::from(decrypted_full_text.concat()), content_type))
    }

    /// Get the cost of creating a new vault.
    /// A quick estimation of cost:
    ///   num_of_graph_entry * graph_entry_cost + num_of_scratchpad * scratchpad_cost
    pub async fn vault_cost(
        &self,
        owner: &VaultSecretKey,
        max_size: u64,
    ) -> Result<AttoTokens, VaultError> {
        if max_size == 0 {
            return Err(VaultError::VaultWithZeroContentSize);
        }

        info!("Getting cost for vault");
        let public_key = MainSecretKey::new(owner.clone())
            .derive_key(&DerivationIndex::from_bytes(VAULT_HEAD_DERIVATION_INDEX))
            .public_key();
        let graph_entry_cost = self.graph_entry_cost(&public_key.into()).await?;
        if graph_entry_cost.is_zero() {
            // Has been created, assuming all Scratchpads have been created and paid
            Ok(graph_entry_cost)
        } else {
            let scratchpad_cost = self.scratchpad_cost(&public_key.into()).await?;

            let num_of_scratchpads = max_size / MAX_CONTENT_PER_SCRATCHPAD as u64 + 1;
            let num_of_graph_entry =
                num_of_scratchpads / NUM_OF_SCRATCHPADS_PER_GRAPHENTRY as u64 + 1;

            let total_cost = U256::from(num_of_graph_entry) * graph_entry_cost.as_atto()
                + U256::from(num_of_scratchpads) * scratchpad_cost.as_atto();
            Ok(AttoTokens::from_atto(total_cost))
        }
    }

    /// Put data into the client's vault.
    ///
    /// Dynamically expand the vault capacity by paying for more space ([`Scratchpad`]) when needed.
    ///
    /// It is recommended to use the hash of the app name or unique identifier as the content type.
    pub async fn write_bytes_to_vault(
        &self,
        data: Bytes,
        payment_option: PaymentOption,
        secret_key: &VaultSecretKey,
        content_type: VaultContentType,
    ) -> Result<AttoTokens, VaultError> {
        if data.is_empty() {
            return Err(VaultError::VaultWithZeroContentSize);
        }

        info!("Writing {} bytes to vault ...", data.len());
        let mut total_cost = AttoTokens::zero();
        let main_secret_key = MainSecretKey::new(secret_key.clone());

        // scratchpad_derivations ordered by the collection order
        let (mut cur_free_graphentry_derivation, mut scratchpad_derivations) = self
            .vault_claimed_capacity(
                &main_secret_key,
                DerivationIndex::from_bytes(VAULT_HEAD_DERIVATION_INDEX),
            )
            .await?;

        let contents = split_bytes(data);

        info!(
            "Current capacity is {}, meanwhile requiring {}",
            scratchpad_derivations.len(),
            contents.len()
        );

        // claim more capacity if short of.
        // Note: as the Scratchpad is `created on use`, hence during the `claim stage`,
        //       NUM_OF_SCRATCHPADS_PER_GRAPHENTRY to be claimed in one newly created GraphEntry.
        while scratchpad_derivations.len() < contents.len() {
            let (new_free_graphentry_derivation, new_scratchpad_derivations, graph_cost) = self
                .expand_capacity(
                    &main_secret_key,
                    &cur_free_graphentry_derivation,
                    payment_option.clone(),
                )
                .await?;
            cur_free_graphentry_derivation = new_free_graphentry_derivation;
            scratchpad_derivations.extend(&new_scratchpad_derivations);
            total_cost = AttoTokens::from_atto(total_cost.as_atto() + graph_cost.as_atto());
        }

        // Convert to Vec of futures
        let update_futures: Vec<_> = contents
            .into_iter()
            .enumerate()
            .map(|(i, content)| {
                let sp_secret_key = main_secret_key
                    .derive_key(&DerivationIndex::from_bytes(scratchpad_derivations[i].1));
                let client = self.clone();
                let payment_option_clone = payment_option.clone();

                async move {
                    let target_addr = ScratchpadAddress::new(sp_secret_key.public_key().into());
                    let already_exists = self.scratchpad_check_existance(&target_addr).await?;

                    if already_exists {
                        info!(
                            "Updating Scratchpad at {target_addr:?} with content of {} bytes",
                            content.len()
                        );
                        match client
                            .scratchpad_update(&sp_secret_key.clone().into(), content_type, &content)
                            .await
                        {
                            Ok(()) => {
                                info!(
                                    "Updated Scratchpad at {target_addr:?} with content of {} bytes",
                                    content.len()
                                );
                                Ok(None)
                            }
                            Err(err) => Err(err.into()),
                        }
                    } else {
                        info!("Creating Scratchpad at {target_addr:?}");
                        let (price, addr) = client
                            .scratchpad_create(
                                &sp_secret_key.into(),
                                content_type,
                                &content,
                                payment_option_clone,
                            )
                            .await?;
                        info!("Created Scratchpad at {addr:?} with cost of {price:?}");
                        Ok(Some(price))
                    }
                }
            })
            .collect();

        let update_results =
            process_tasks_with_max_concurrency(update_futures, *FILE_UPLOAD_BATCH_SIZE).await;

        // Process results
        for result in update_results {
            match result {
                Ok(Some(price)) => {
                    total_cost = AttoTokens::from_atto(total_cost.as_atto() + price.as_atto());
                }
                Ok(None) => (),
                Err(e) => return Err(e),
            }
        }

        Ok(total_cost)
    }

    // Expand the capacity, i.e. upload one GraphEntry
    // The returned value is:
    //   * cur_free_graphentry_derivation: the output[0] of the tail of the linked GraphEntry
    //   * scratchpad_derivations: ordered by the creating order
    //   * graph_cost: cost paid to upload the GraphEntry
    async fn expand_capacity(
        &self,
        main_secret_key: &MainSecretKey,
        cur_graphentry_derivation: &DerivationIndex,
        payment_option: PaymentOption,
    ) -> Result<(DerivationIndex, Vec<(PublicKey, GraphContent)>, AttoTokens), VaultError> {
        let own_secret_key = main_secret_key.derive_key(cur_graphentry_derivation);

        // For Vault, doesn't need the backward poining. i.e. one-direction link shall be enough.
        let parents = vec![];
        // For Vault, doesn't need this field to be populated.
        let initial_value = [0u8; 32];

        // Poining to the next GraphEntry
        let new_graphentry_derivation = DerivationIndex::random(&mut rand::thread_rng());
        let public_key: PublicKey = main_secret_key
            .derive_key(&new_graphentry_derivation)
            .public_key()
            .into();
        let mut descendants = vec![(public_key, new_graphentry_derivation.into_bytes())];

        // Pointing to other future Scrachpads
        descendants.extend((0..NUM_OF_SCRATCHPADS_PER_GRAPHENTRY).map(|_| {
            let derivation_index = DerivationIndex::random(&mut rand::thread_rng());
            let public_key: PublicKey = main_secret_key
                .derive_key(&derivation_index)
                .public_key()
                .into();
            (public_key, derivation_index.into_bytes())
        }));

        let graph_entry = GraphEntry::new(
            &own_secret_key.into(),
            parents,
            initial_value,
            descendants.clone(),
        );

        // Upload the GraphEntry
        let (graph_cost, _addr) = self.graph_entry_put(graph_entry, payment_option).await?;

        let scratchpad_derivations = descendants.split_off(1);
        Ok((
            new_graphentry_derivation,
            scratchpad_derivations,
            graph_cost,
        ))
    }

    // Collects the current claimed capacity (i.e. the uploaded `GrapthEntry`s)
    // The returned value is:
    //   * cur_free_graphentry_derivation: i.e. the root if no graph_entry uploaded,
    //       otherwise, the first un-used one (the output[0] of the tail of the linked GraphEntry)
    //   * scratchpad_derivations: ordered by the collection order
    async fn vault_claimed_capacity(
        &self,
        main_secret_key: &MainSecretKey,
        mut cur_free_graphentry_derivation: DerivationIndex,
    ) -> Result<(DerivationIndex, Vec<(PublicKey, GraphContent)>), VaultError> {
        let mut scratchpad_derivations = vec![];
        loop {
            let public_key = main_secret_key
                .derive_key(&cur_free_graphentry_derivation)
                .public_key();
            let cur_graph_entry_addr = GraphEntryAddress::new(public_key.into());

            match self.graph_entry_get(&cur_graph_entry_addr).await {
                Ok(entry) => {
                    // A GraphEntry was created with all NUM_OF_SCRATCHPADS_PER_GRAPHENTRY
                    // scratchpad claimed:
                    //   * the first descendant pointing to next GraphEntry.
                    //   * other descendants pointing to Scratchpads for content.
                    if entry.descendants.len() <= NUM_OF_SCRATCHPADS_PER_GRAPHENTRY {
                        let msg = format!("Vault's GraphEntry at {cur_graph_entry_addr:?} only has {} descendants.",
                            entry.descendants.len());
                        return Err(VaultError::VaultNotEnoughGraphDescendants(msg));
                    }
                    cur_free_graphentry_derivation =
                        DerivationIndex::from_bytes(entry.descendants[0].1);
                    scratchpad_derivations.extend(&entry.descendants[1..]);
                }
                Err(GraphError::Network(NetworkError::GetRecordError(
                    GetRecordError::RecordNotFound,
                ))) => {
                    // GraphEntry not existed, return the current snapshot.
                    info!(
                        "vault capacity is successfully fetched, with {} scratchpads",
                        scratchpad_derivations.len()
                    );
                    return Ok((cur_free_graphentry_derivation, scratchpad_derivations));
                }
                Err(err) => {
                    return Err(err.into());
                }
            }
        }
    }

    async fn fetch_scratchpads_of_one_graph_entry_and_decrypt(
        &self,
        main_secret_key: &MainSecretKey,
        scratchpad_addresses: Vec<(PublicKey, [u8; 32])>,
    ) -> Result<(Bytes, VaultContentType, bool), VaultError> {
        let mut decrypted_full_text = vec![];
        let mut content_type = 0;
        let mut has_end_reached = false;
        // Any non-max-sized ScratchPad indicates the end-of-vault-content.
        for (pub_key, derive_bytes) in scratchpad_addresses {
            let addr = ScratchpadAddress::new(pub_key);
            let secret_key = main_secret_key.derive_key(&DerivationIndex::from_bytes(derive_bytes));

            let sp = self.scratchpad_get(&addr).await?;
            content_type = sp.data_encoding();
            let decrypt_data = sp.decrypt_data(&secret_key.into())?;
            decrypted_full_text.push(decrypt_data);
            if sp.encrypted_data().len() < MAX_CONTENT_PER_SCRATCHPAD {
                has_end_reached = true;
                break;
            }
        }

        Ok((
            Bytes::from(decrypted_full_text.concat()),
            content_type,
            has_end_reached,
        ))
    }
}

fn split_bytes(input: Bytes) -> Vec<Bytes> {
    let mut contents = Vec::new();
    let mut offset = 0;

    while offset < input.len() {
        let end = (offset + MAX_CONTENT_PER_SCRATCHPAD).min(input.len());
        contents.push(input.slice(offset..end));
        offset = end;
    }

    contents
}
