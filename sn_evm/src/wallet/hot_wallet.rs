// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::{
    api::{WalletApi, WALLET_DIR_NAME},
    data_payments::PaymentQuote,
    keys::{get_main_key_from_disk, store_new_keypair},
    wallet_file::{
        get_confirmed_spend, get_unconfirmed_spend_requests, has_confirmed_spend,
        load_created_cash_note, remove_cash_notes, remove_unconfirmed_spend_requests,
        store_created_cash_notes, store_unconfirmed_spend_requests,
    },
    watch_only::WatchOnlyWallet,
    Error, Result,
};
use crate::{wallet::encryption::EncryptedSecretKey, ProofOfPayment};
use crate::wallet::keys::{
    delete_encrypted_main_secret_key, delete_unencrypted_main_secret_key, get_main_pubkey,
    store_main_secret_key,
};
use crate::{
    calculate_royalties_fee, transfers::SignedTransaction, CashNote, CashNoteRedemption,
    DerivationIndex, DerivedSecretKey, MainPubkey, MainSecretKey, AttoTokens, SignedSpend,
    SpendAddress, SpendReason, Transfer, UniquePubkey, UnsignedTransaction, WalletError,
};
use crate::{wallet::authentication::AuthenticationManager, RewardsAddress};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    path::{Path, PathBuf},
    time::Instant,
};
use evmlib::{common::Amount, wallet::Wallet as EvmWallet};
use evmlib::Network as EvmNetwork;
use xor_name::XorName;

/// A locked file handle, that when dropped releases the lock.
pub type WalletExclusiveAccess = File;

/// A hot-wallet.
pub struct HotWallet {
    /// The secret key with which we can access
    /// all the tokens in the available_cash_notes.
    key: MainSecretKey,
    /// The wallet containing all data.
    watchonly_wallet: WatchOnlyWallet,
    /// These have not yet been successfully sent to the network
    /// and need to be, to reach network validity.
    unconfirmed_spend_requests: BTreeSet<SignedSpend>,
    /// Handles authentication of (encrypted) wallets.
    authentication_manager: AuthenticationManager,
}

impl HotWallet {
    #[cfg(feature = "test-utils")]
    pub fn key(&self) -> &MainSecretKey {
        &self.key
    }

    pub fn api(&self) -> &WalletApi {
        self.watchonly_wallet.api()
    }

    pub fn root_dir(&self) -> &Path {
        self.watchonly_wallet.api().wallet_dir()
    }

    /// Returns whether a wallet in the specified directory is encrypted or not.
    pub fn is_encrypted(root_dir: &Path) -> bool {
        let wallet_dir = root_dir.join(WALLET_DIR_NAME);
        EncryptedSecretKey::file_exists(&wallet_dir)
    }

    /// Stores the wallet to disk.
    /// This requires having exclusive access to the wallet to prevent concurrent processes from writing to it
    fn store(&self, exclusive_access: WalletExclusiveAccess) -> Result<()> {
        self.watchonly_wallet.store(exclusive_access)
    }

    /// Reloads the wallet from disk. If the wallet secret key is encrypted, you'll need to specify the password.
    fn reload(&mut self) -> Result<()> {
        // Password needed to decrypt wallet if it is encrypted
        let opt_password = self.authenticate()?;

        let wallet =
            Self::load_from_path_and_key(self.watchonly_wallet.wallet_dir(), None, opt_password)?;

        if *wallet.key.secret_key() != *self.key.secret_key() {
            return Err(WalletError::CurrentAndLoadedKeyMismatch(
                self.watchonly_wallet.wallet_dir().into(),
            ));
        }

        // if it's a matching key, we can overwrite our wallet
        *self = wallet;
        Ok(())
    }

    /// Authenticates the wallet and returns the password if it is encrypted.
    ///
    /// # Returns
    /// - `Ok(Some(String))`: The wallet is encrypted and the password is available.
    /// - `Ok(None)`: The wallet is not encrypted.
    /// - `Err`: The wallet is encrypted, but no password is available.
    ///
    /// # Errors
    /// Returns an error if the wallet is encrypted and the password is not available.
    /// In such cases, the password needs to be set using `authenticate_with_password()`.
    pub fn authenticate(&mut self) -> Result<Option<String>> {
        self.authentication_manager.authenticate()
    }

    /// Authenticates the wallet and saves the password for a certain amount of time.
    pub fn authenticate_with_password(&mut self, password: String) -> Result<()> {
        self.authentication_manager
            .authenticate_with_password(password)
    }

    /// Encrypts wallet with a password.
    ///
    /// Fails if wallet is already encrypted.
    pub fn encrypt(root_dir: &Path, password: &str) -> Result<()> {
        if Self::is_encrypted(root_dir) {
            return Err(Error::WalletAlreadyEncrypted);
        }

        let wallet_key = Self::load_from(root_dir)?.key;
        let wallet_dir = root_dir.join(WALLET_DIR_NAME);

        // Save the secret key as an encrypted file
        store_main_secret_key(&wallet_dir, &wallet_key, Some(password.to_owned()))?;

        // Delete the unencrypted secret key file
        // Cleanup if it fails
        if let Err(err) = delete_unencrypted_main_secret_key(&wallet_dir) {
            let _ = delete_encrypted_main_secret_key(&wallet_dir);
            return Err(err);
        }

        Ok(())
    }

    /// Locks the wallet and returns exclusive access to the wallet
    /// This lock prevents any other process from locking the wallet dir, effectively acts as a mutex for the wallet
    pub fn lock(&self) -> Result<WalletExclusiveAccess> {
        self.watchonly_wallet.lock()
    }

    /// Stores the given cash_notes to the `created cash_notes dir` in the wallet dir.
    /// These can then be sent to the recipients out of band, over any channel preferred.
    pub fn store_cash_notes_to_disk<'a, T>(&self, cash_notes: T) -> Result<()>
    where
        T: IntoIterator<Item = &'a CashNote>,
    {
        store_created_cash_notes(cash_notes, self.watchonly_wallet.wallet_dir())
    }
    /// Removes the given cash_notes from the `created cash_notes dir` in the wallet dir.
    pub fn remove_cash_notes_from_disk<'a, T>(&self, cash_notes: T) -> Result<()>
    where
        T: IntoIterator<Item = &'a UniquePubkey>,
    {
        remove_cash_notes(cash_notes, self.watchonly_wallet.wallet_dir())
    }

    /// Store unconfirmed_spend_requests to disk.
    pub fn store_unconfirmed_spend_requests(&mut self) -> Result<()> {
        store_unconfirmed_spend_requests(
            self.watchonly_wallet.wallet_dir(),
            self.unconfirmed_spend_requests(),
        )
    }

    /// Get confirmed spend from disk.
    pub fn get_confirmed_spend(&mut self, spend_addr: SpendAddress) -> Result<Option<SignedSpend>> {
        get_confirmed_spend(self.watchonly_wallet.wallet_dir(), spend_addr)
    }

    /// Check whether have the specific confirmed spend.
    pub fn has_confirmed_spend(&mut self, spend_addr: SpendAddress) -> bool {
        has_confirmed_spend(self.watchonly_wallet.wallet_dir(), spend_addr)
    }

    /// Remove unconfirmed_spend_requests from disk.
    fn remove_unconfirmed_spend_requests(&mut self) -> Result<()> {
        remove_unconfirmed_spend_requests(
            self.watchonly_wallet.wallet_dir(),
            self.unconfirmed_spend_requests(),
        )
    }

    /// Remove referenced CashNotes from available_cash_notes
    pub fn mark_notes_as_spent<'a, T>(&mut self, unique_pubkeys: T)
    where
        T: IntoIterator<Item = &'a UniquePubkey>,
    {
        self.watchonly_wallet.mark_notes_as_spent(unique_pubkeys);
    }

    pub fn unconfirmed_spend_requests_exist(&self) -> bool {
        !self.unconfirmed_spend_requests.is_empty()
    }

    /// Try to load any new cash_notes from the `cash_notes dir` in the wallet dir.
    pub fn try_load_cash_notes(&mut self) -> Result<()> {
        self.watchonly_wallet.try_load_cash_notes()
    }

    /// Loads a serialized wallet from a path and given main key.
    pub fn load_from_main_key(root_dir: &Path, main_key: MainSecretKey) -> Result<Self> {
        let wallet_dir = root_dir.join(WALLET_DIR_NAME);
        // This creates the received_cash_notes dir if it doesn't exist.
        std::fs::create_dir_all(&wallet_dir)?;
        // This creates the main_key file if it doesn't exist.
        Self::load_from_path_and_key(&wallet_dir, Some(main_key), None)
    }

    /// Creates a serialized wallet for a path and main key.
    /// This will overwrite any existing wallet, unlike load_from_main_key
    pub fn create_from_key(
        root_dir: &Path,
        key: MainSecretKey,
        password: Option<String>,
    ) -> Result<Self> {
        let wallet_dir = root_dir.join(WALLET_DIR_NAME);
        // This creates the received_cash_notes dir if it doesn't exist.
        std::fs::create_dir_all(&wallet_dir)?;
        // Create the new wallet for this key
        store_new_keypair(&wallet_dir, &key, password)?;
        let unconfirmed_spend_requests =
            (get_unconfirmed_spend_requests(&wallet_dir)?).unwrap_or_default();
        let watchonly_wallet = WatchOnlyWallet::load_from(&wallet_dir, key.main_pubkey())?;

        Ok(Self {
            key,
            watchonly_wallet,
            unconfirmed_spend_requests,
            authentication_manager: AuthenticationManager::new(wallet_dir),
        })
    }

    /// Loads a serialized wallet from a path.
    pub fn load_from(root_dir: &Path) -> Result<Self> {
        let wallet_dir = root_dir.join(WALLET_DIR_NAME);
        Self::load_from_path(&wallet_dir, None)
    }

    /// Tries to loads a serialized wallet from a path, bailing out if it doesn't exist.
    pub fn try_load_from(root_dir: &Path) -> Result<Self> {
        let wallet_dir = root_dir.join(WALLET_DIR_NAME);
        Self::load_from_path_and_key(&wallet_dir, None, None)
    }

    /// Loads a serialized wallet from a given path, no additional element will
    /// be added to the provided path and strictly taken as the wallet files location.
    pub fn load_from_path(wallet_dir: &Path, main_key: Option<MainSecretKey>) -> Result<Self> {
        std::fs::create_dir_all(wallet_dir)?;
        Self::load_from_path_and_key(wallet_dir, main_key, None)
    }

    /// Loads an encrypted serialized wallet from a given root path.
    pub fn load_encrypted_from_path(root_dir: &Path, password: String) -> Result<Self> {
        let wallet_dir = root_dir.join(WALLET_DIR_NAME);
        std::fs::create_dir_all(&wallet_dir)?;
        Self::load_from_path_and_key(&wallet_dir, None, Some(password))
    }

    pub fn address(&self) -> MainPubkey {
        self.key.main_pubkey()
    }

    pub fn unconfirmed_spend_requests(&self) -> &BTreeSet<SignedSpend> {
        &self.unconfirmed_spend_requests
    }

    /// Moves all files for the current wallet, including keys and cashnotes
    /// to directory root_dir/wallet_ADDRESS
    pub fn stash(root_dir: &Path) -> Result<PathBuf> {
        let wallet_dir = root_dir.join(WALLET_DIR_NAME);
        let wallet_pub_key =
            get_main_pubkey(&wallet_dir)?.ok_or(Error::PubkeyNotFound(wallet_dir.clone()))?;
        let addr_hex = wallet_pub_key.to_hex();
        let new_name = format!("{WALLET_DIR_NAME}_{addr_hex}");
        let moved_dir = root_dir.join(new_name);
        std::fs::rename(wallet_dir, &moved_dir)?;
        Ok(moved_dir)
    }

    /// Moves a previously stashed wallet to the root wallet directory.
    pub fn unstash(root_dir: &Path, addr_hex: &str) -> Result<()> {
        let cleared_name = format!("{WALLET_DIR_NAME}_{addr_hex}");
        let cleared_dir = root_dir.join(cleared_name);
        let wallet_dir = root_dir.join(WALLET_DIR_NAME);

        // Stash old wallet if it exists
        if wallet_dir.exists() {
            if let Ok(_wallet) = HotWallet::load_from(root_dir) {
                Self::stash(root_dir)?;
            }

            std::fs::remove_dir_all(&wallet_dir)?;
        }

        std::fs::rename(cleared_dir, wallet_dir)?;
        Ok(())
    }

    /// Removes all files for the current wallet, including keys and cashnotes
    pub fn remove(root_dir: &Path) -> Result<()> {
        let wallet_dir = root_dir.join(WALLET_DIR_NAME);
        std::fs::remove_dir_all(wallet_dir)?;
        Ok(())
    }

    /// To remove a specific spend from the requests, if eg, we see one spend is _bad_
    pub fn clear_specific_spend_request(&mut self, unique_pub_key: UniquePubkey) {
        if let Err(error) = self.remove_cash_notes_from_disk(vec![&unique_pub_key]) {
            warn!("Could not clean spend {unique_pub_key:?} due to {error:?}");
        }

        self.unconfirmed_spend_requests
            .retain(|signed_spend| signed_spend.spend.unique_pubkey.ne(&unique_pub_key))
    }

    /// Once spends are verified we can clear them and clean up
    pub fn clear_confirmed_spend_requests(&mut self) {
        if let Err(error) = self.remove_cash_notes_from_disk(
            self.unconfirmed_spend_requests
                .iter()
                .map(|s| &s.spend.unique_pubkey),
        ) {
            warn!("Could not clean confirmed spent cash_notes due to {error:?}");
        }

        // Also need to remove unconfirmed_spend_requests from disk if was pre-loaded.
        let _ = self.remove_unconfirmed_spend_requests();

        self.unconfirmed_spend_requests = Default::default();
    }

    pub fn balance(&self) -> AttoTokens {
        self.watchonly_wallet.balance()
    }

    pub fn sign(&self, unsigned_tx: UnsignedTransaction) -> Result<SignedTransaction> {
        if let Err(err) = unsigned_tx.verify() {
            return Err(Error::CouldNotSignTransaction(format!(
                "Failed to verify unsigned transaction: {err:?}"
            )));
        }
        let signed_tx = unsigned_tx
            .sign(&self.key)
            .map_err(|e| Error::CouldNotSignTransaction(e.to_string()))?;
        if let Err(err) = signed_tx.verify() {
            return Err(Error::CouldNotSignTransaction(format!(
                "Failed to verify signed transaction: {err:?}"
            )));
        }
        Ok(signed_tx)
    }

    /// Checks whether the specified cash_note already presents
    pub fn cash_note_presents(&mut self, id: &UniquePubkey) -> bool {
        self.watchonly_wallet
            .available_cash_notes()
            .contains_key(id)
    }

    /// Returns all available cash_notes and an exclusive access to the wallet so no concurrent processes can
    /// get available cash_notes while we're modifying the wallet
    /// once the updated wallet is stored to disk it is safe to drop the WalletExclusiveAccess
    pub fn available_cash_notes(&mut self) -> Result<(Vec<CashNote>, WalletExclusiveAccess)> {
        trace!("Trying to lock wallet to get available cash_notes...");
        // lock and load from disk to make sure we're up to date and others can't modify the wallet concurrently
        let exclusive_access = self.lock()?;
        self.reload()?;
        trace!("Wallet locked and loaded!");

        // get the available cash_notes
        let mut available_cash_notes = vec![];
        let wallet_dir = self.watchonly_wallet.wallet_dir().to_path_buf();
        for (id, _token) in self.watchonly_wallet.available_cash_notes().iter() {
            let held_cash_note = load_created_cash_note(id, &wallet_dir);
            if let Some(cash_note) = held_cash_note {
                if cash_note.derived_key(&self.key).is_ok() {
                    available_cash_notes.push(cash_note.clone());
                } else {
                    warn!(
                        "Skipping CashNote {:?} because we don't have the key to spend it",
                        cash_note.unique_pubkey()
                    );
                }
            } else {
                warn!("Skipping CashNote {:?} because we don't have it", id);
            }
        }

        Ok((available_cash_notes, exclusive_access))
    }

    /// Remove the payment_details of the given XorName from disk.
    pub fn remove_payment_for_xorname(&self, name: &XorName) {
        self.api().remove_payment_transaction(name)
    }

    pub fn build_unsigned_transaction(
        &mut self,
        to: Vec<(AttoTokens, MainPubkey)>,
        reason: Option<SpendReason>,
    ) -> Result<UnsignedTransaction> {
        self.watchonly_wallet.build_unsigned_transaction(to, reason)
    }

    /// Make a transfer and return all created cash_notes
    pub fn local_send(
        &mut self,
        to: Vec<(AttoTokens, MainPubkey)>,
        reason: Option<SpendReason>,
    ) -> Result<Vec<CashNote>> {
        let mut rng = &mut rand::rngs::OsRng;
        // create a unique key for each output
        let to_unique_keys: Vec<_> = to
            .into_iter()
            .map(|(amount, address)| (amount, address, DerivationIndex::random(&mut rng), false))
            .collect();

        let (available_cash_notes, exclusive_access) = self.available_cash_notes()?;
        println!("Available CashNotes for local send: {available_cash_notes:#?}");

        let reason = reason.unwrap_or_default();

        let signed_tx = SignedTransaction::new(
            available_cash_notes,
            to_unique_keys,
            self.address(),
            reason,
            &self.key,
        )?;

        let created_cash_notes = signed_tx.output_cashnotes.clone();

        self.update_local_wallet(signed_tx, exclusive_access, true)?;

        trace!("Releasing wallet lock"); // by dropping _exclusive_access
        Ok(created_cash_notes)
    }

    /// Performs a payment for each content address.
    /// Includes payment of network royalties.
    /// Returns the amount paid for storage, including the network royalties fee paid.
    pub async fn send_storage_payment(
        &mut self,
        price_map: &BTreeMap<XorName, (RewardsAddress, PaymentQuote, Vec<u8>)>,
    ) -> Result<(AttoTokens, AttoTokens)> {
        let mut total_storage_cost = AttoTokens::zero();
        let mut total_royalties_fees = AttoTokens::zero();

        let start = Instant::now();

        // calculate total storage cost incl royalties fees
        let mut quotes_payment = BTreeSet::new();
        let mut quotes_by_hash = BTreeMap::new();
        for (_xorname, (rewards_address, quote, _peer_id_bytes)) in price_map.iter() {
            let royalties_fee = calculate_royalties_fee(quote.cost);

            total_storage_cost = total_storage_cost
                .checked_add(quote.cost)
                .ok_or(WalletError::TotalPriceTooHigh)?;
            total_royalties_fees = total_royalties_fees
                .checked_add(royalties_fee)
                .ok_or(WalletError::TotalPriceTooHigh)?;

            let cost = Amount::from(quote.cost.as_atto());

            quotes_payment.insert((quote.hash(), *rewards_address, cost));
            quotes_by_hash.insert(quote.hash(), quote.clone());
        }

        // load evm wallet
        let evm_sk = std::env::var("EVM_SK")
            .map_err(|_| Error::EvmWallet(
                "No evm wallet secret key found. EVM_SK env var not set. Please set the EVM_SK env var to your evm wallet secret key hex string.".to_string()
            )).expect("Failed to get EVM_SK env var");
        // TODO make network configurable here
        let evm_wallet = EvmWallet::new_from_private_key(EvmNetwork::ArbitrumOne, &evm_sk)
            .map_err(|_| Error::EvmWallet(
                "Failed to parse EVM key from EVM_SK env var. Please set the EVM_SK env var to your evm wallet secret key in hex string format.".to_string()
            ))?;

        // perform evm payments
        let tx_hashes_by_quote = evm_wallet.pay_for_quotes(quotes_payment).await.map_err(|e| Error::EvmWallet(format!("Failed to pay for quotes {e}")))?;
        for (quote_hash, tx_hash) in tx_hashes_by_quote.iter() {
            let quote = quotes_by_hash.get(quote_hash).ok_or(Error::EvmWallet(format!("Quote was forgotten during payment {quote_hash:?}")))?;
            let proof = ProofOfPayment {
                quote: quote.clone(),
                tx_hash: *tx_hash,
            };
            // save payment to disk so it can be recovered later
            let _ = self.watchonly_wallet.insert_payment_transaction(quote.content, proof);
        }

        trace!(
            "local_send_storage_payment completed payments insertion in {:?}",
            start.elapsed()
        );

        Ok((total_storage_cost, total_royalties_fees))
    }

    #[cfg(feature = "test-utils")]
    pub fn test_update_local_wallet(
        &mut self,
        transfer: SignedTransaction,
        exclusive_access: WalletExclusiveAccess,
        insert_into_pending_spends: bool,
    ) -> Result<()> {
        self.update_local_wallet(transfer, exclusive_access, insert_into_pending_spends)
    }

    fn update_local_wallet(
        &mut self,
        signed_tx: SignedTransaction,
        exclusive_access: WalletExclusiveAccess,
        insert_into_pending_spends: bool,
    ) -> Result<()> {
        // First of all, update client local state.
        let spent_unique_pubkeys: BTreeSet<_> =
            signed_tx.spends.iter().map(|s| s.unique_pubkey()).collect();

        self.watchonly_wallet
            .mark_notes_as_spent(spent_unique_pubkeys.clone());

        if let Some(cash_note) = signed_tx.change_cashnote {
            let start = Instant::now();
            self.watchonly_wallet.deposit(&[cash_note.clone()])?;
            trace!(
                "update_local_wallet completed deposit change cash_note in {:?}",
                start.elapsed()
            );
            let start = Instant::now();

            // Only the change_cash_note, i.e. the pay-in one, needs to be stored to disk.
            //
            // Paying out cash_note doesn't need to be stored into disk.
            // As it is the transfer, that generated from it, to be sent out to network,
            // and be stored within the unconfirmed_spends, and to be re-sent in case of failure.
            self.store_cash_notes_to_disk(&[cash_note])?;
            trace!(
                "update_local_wallet completed store change cash_note to disk in {:?}",
                start.elapsed()
            );
        }
        if insert_into_pending_spends {
            for request in signed_tx.spends {
                self.unconfirmed_spend_requests.insert(request);
            }
        }

        // store wallet to disk
        let start = Instant::now();
        self.store(exclusive_access)?;
        trace!(
            "update_local_wallet completed store self wallet to disk in {:?}",
            start.elapsed()
        );
        Ok(())
    }

    /// Deposit the given cash_notes on the wallet (without storing them to disk).
    pub fn deposit(&mut self, received_cash_notes: &Vec<CashNote>) -> Result<()> {
        self.watchonly_wallet.deposit(received_cash_notes)
    }

    /// Store the given cash_notes to the `cash_notes` dir in the wallet dir.
    /// Update and store the updated wallet to disk
    /// This function locks the wallet to prevent concurrent processes from writing to it
    pub fn deposit_and_store_to_disk(&mut self, received_cash_notes: &Vec<CashNote>) -> Result<()> {
        self.watchonly_wallet
            .deposit_and_store_to_disk(received_cash_notes)
    }

    pub fn unwrap_transfer(&self, transfer: &Transfer) -> Result<Vec<CashNoteRedemption>> {
        transfer
            .cashnote_redemptions(&self.key)
            .map_err(|_| Error::FailedToDecypherTransfer)
    }

    pub fn derive_key(&self, derivation_index: &DerivationIndex) -> DerivedSecretKey {
        self.key.derive_key(derivation_index)
    }

    /// Loads a serialized wallet from a path.
    // TODO: what's the behaviour here if path has stored key and we pass one in?
    fn load_from_path_and_key(
        wallet_dir: &Path,
        main_key: Option<MainSecretKey>,
        main_key_password: Option<String>,
    ) -> Result<Self> {
        let key = match get_main_key_from_disk(wallet_dir, main_key_password.to_owned()) {
            Ok(key) => {
                if let Some(passed_key) = main_key {
                    if key.secret_key() != passed_key.secret_key() {
                        warn!("main_key passed to load_from_path_and_key, but a key was found in the wallet dir. Using the one found in the wallet dir.");
                    }
                }

                key
            }
            Err(error) => {
                if let Some(key) = main_key {
                    store_new_keypair(wallet_dir, &key, main_key_password)?;
                    key
                } else {
                    error!(
                        "No main key found when loading wallet from path {:?}",
                        wallet_dir
                    );

                    return Err(error);
                }
            }
        };
        let unconfirmed_spend_requests =
            (get_unconfirmed_spend_requests(wallet_dir)?).unwrap_or_default();
        let watchonly_wallet = WatchOnlyWallet::load_from(wallet_dir, key.main_pubkey())?;

        Ok(Self {
            key,
            watchonly_wallet,
            unconfirmed_spend_requests,
            authentication_manager: AuthenticationManager::new(wallet_dir.to_path_buf()),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::HotWallet;
    use crate::wallet::authentication::AuthenticationManager;
    use crate::{
        genesis::{create_first_cash_note_from_key, GENESIS_CASHNOTE_AMOUNT},
        wallet::{
            data_payments::PaymentQuote, hot_wallet::WALLET_DIR_NAME, wallet_file::store_wallet,
            watch_only::WatchOnlyWallet, KeyLessWallet,
        },
        MainSecretKey, AttoTokens, SpendAddress,
    };
    use assert_fs::TempDir;
    use evmlib::common::Amount;
    use evmlib::utils::dummy_address;
    use eyre::Result;
    use xor_name::XorName;

    #[tokio::test]
    async fn keyless_wallet_to_and_from_file() -> Result<()> {
        let key = MainSecretKey::random();
        let mut wallet = KeyLessWallet::default();
        let genesis = create_first_cash_note_from_key(&key).expect("Genesis creation to succeed.");

        let dir = create_temp_dir();
        let wallet_dir = dir.path().to_path_buf();

        wallet
            .available_cash_notes
            .insert(genesis.unique_pubkey(), genesis.value());

        store_wallet(&wallet_dir, &wallet)?;

        let deserialized =
            KeyLessWallet::load_from(&wallet_dir)?.expect("There to be a wallet on disk.");

        assert_eq!(Amount::from(GENESIS_CASHNOTE_AMOUNT), wallet.balance().as_atto());
        assert_eq!(Amount::from(GENESIS_CASHNOTE_AMOUNT), deserialized.balance().as_atto());

        Ok(())
    }

    #[test]
    fn wallet_basics() -> Result<()> {
        let key = MainSecretKey::random();
        let main_pubkey = key.main_pubkey();
        let dir = create_temp_dir();

        let deposit_only = HotWallet {
            key,
            watchonly_wallet: WatchOnlyWallet::new(main_pubkey, &dir, KeyLessWallet::default()),
            unconfirmed_spend_requests: Default::default(),
            authentication_manager: AuthenticationManager::new(dir.to_path_buf()),
        };

        assert_eq!(main_pubkey, deposit_only.address());
        assert_eq!(AttoTokens::zero(), deposit_only.balance());

        assert!(deposit_only
            .watchonly_wallet
            .available_cash_notes()
            .is_empty());

        Ok(())
    }

    /// -----------------------------------
    /// <-------> DepositWallet <--------->
    /// -----------------------------------

    #[tokio::test]
    async fn deposit_empty_list_does_nothing() -> Result<()> {
        let key = MainSecretKey::random();
        let main_pubkey = key.main_pubkey();
        let dir = create_temp_dir();

        let mut deposit_only = HotWallet {
            key,
            watchonly_wallet: WatchOnlyWallet::new(main_pubkey, &dir, KeyLessWallet::default()),
            unconfirmed_spend_requests: Default::default(),
            authentication_manager: AuthenticationManager::new(dir.to_path_buf()),
        };

        deposit_only.deposit_and_store_to_disk(&vec![])?;

        assert_eq!(AttoTokens::zero(), deposit_only.balance());

        assert!(deposit_only
            .watchonly_wallet
            .available_cash_notes()
            .is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn deposit_adds_cash_notes_that_belongs_to_the_wallet() -> Result<()> {
        let key = MainSecretKey::random();
        let main_pubkey = key.main_pubkey();
        let genesis = create_first_cash_note_from_key(&key).expect("Genesis creation to succeed.");
        let dir = create_temp_dir();

        let mut deposit_only = HotWallet {
            key,
            watchonly_wallet: WatchOnlyWallet::new(main_pubkey, &dir, KeyLessWallet::default()),
            unconfirmed_spend_requests: Default::default(),
            authentication_manager: AuthenticationManager::new(dir.to_path_buf()),
        };

        deposit_only.deposit_and_store_to_disk(&vec![genesis])?;

        assert_eq!(Amount::from(GENESIS_CASHNOTE_AMOUNT), deposit_only.balance().as_atto());

        Ok(())
    }

    #[tokio::test]
    async fn deposit_does_not_add_cash_notes_not_belonging_to_the_wallet() -> Result<()> {
        let key = MainSecretKey::random();
        let main_pubkey = key.main_pubkey();
        let genesis = create_first_cash_note_from_key(&MainSecretKey::random())
            .expect("Genesis creation to succeed.");
        let dir = create_temp_dir();

        let mut local_wallet = HotWallet {
            key,
            watchonly_wallet: WatchOnlyWallet::new(main_pubkey, &dir, KeyLessWallet::default()),
            unconfirmed_spend_requests: Default::default(),
            authentication_manager: AuthenticationManager::new(dir.to_path_buf()),
        };

        local_wallet.deposit_and_store_to_disk(&vec![genesis])?;

        assert_eq!(AttoTokens::zero(), local_wallet.balance());

        Ok(())
    }

    #[tokio::test]
    async fn deposit_is_idempotent() -> Result<()> {
        let key = MainSecretKey::random();
        let main_pubkey = key.main_pubkey();
        let genesis_0 =
            create_first_cash_note_from_key(&key).expect("Genesis creation to succeed.");
        let genesis_1 =
            create_first_cash_note_from_key(&key).expect("Genesis creation to succeed.");
        let dir = create_temp_dir();

        let mut deposit_only = HotWallet {
            key,
            watchonly_wallet: WatchOnlyWallet::new(main_pubkey, &dir, KeyLessWallet::default()),
            unconfirmed_spend_requests: Default::default(),
            authentication_manager: AuthenticationManager::new(dir.to_path_buf()),
        };

        deposit_only.deposit_and_store_to_disk(&vec![genesis_0.clone()])?;
        assert_eq!(Amount::from(GENESIS_CASHNOTE_AMOUNT), deposit_only.balance().as_atto());

        deposit_only.deposit_and_store_to_disk(&vec![genesis_0])?;
        assert_eq!(Amount::from(GENESIS_CASHNOTE_AMOUNT), deposit_only.balance().as_atto());

        deposit_only.deposit_and_store_to_disk(&vec![genesis_1])?;
        assert_eq!(Amount::from(GENESIS_CASHNOTE_AMOUNT), deposit_only.balance().as_atto());

        Ok(())
    }

    #[tokio::test]
    async fn deposit_wallet_to_and_from_file() -> Result<()> {
        let dir = create_temp_dir();
        let root_dir = dir.path().to_path_buf();

        let new_wallet = MainSecretKey::random();
        let mut depositor = HotWallet::create_from_key(&root_dir, new_wallet, None)?;
        let genesis =
            create_first_cash_note_from_key(&depositor.key).expect("Genesis creation to succeed.");
        depositor.deposit_and_store_to_disk(&vec![genesis])?;

        let deserialized = HotWallet::load_from(&root_dir)?;

        assert_eq!(depositor.address(), deserialized.address());
        assert_eq!(Amount::from(GENESIS_CASHNOTE_AMOUNT), depositor.balance().as_atto());
        assert_eq!(Amount::from(GENESIS_CASHNOTE_AMOUNT), deserialized.balance().as_atto());

        assert_eq!(1, depositor.watchonly_wallet.available_cash_notes().len());

        assert_eq!(
            1,
            deserialized.watchonly_wallet.available_cash_notes().len()
        );

        let a_available = depositor
            .watchonly_wallet
            .available_cash_notes()
            .values()
            .last()
            .expect("There to be an available CashNote.");
        let b_available = deserialized
            .watchonly_wallet
            .available_cash_notes()
            .values()
            .last()
            .expect("There to be an available CashNote.");
        assert_eq!(a_available, b_available);

        Ok(())
    }

    /// --------------------------------
    /// <-------> SendWallet <--------->
    /// --------------------------------

    #[tokio::test]
    async fn sending_decreases_balance() -> Result<()> {
        let dir = create_temp_dir();
        let root_dir = dir.path().to_path_buf();
        let new_wallet = MainSecretKey::random();
        let mut sender = HotWallet::create_from_key(&root_dir, new_wallet, None)?;
        let sender_cash_note =
            create_first_cash_note_from_key(&sender.key).expect("Genesis creation to succeed.");
        sender.deposit_and_store_to_disk(&vec![sender_cash_note])?;

        assert_eq!(Amount::from(GENESIS_CASHNOTE_AMOUNT), sender.balance().as_atto());

        // We send to a new address.
        let send_amount = 100;
        let recipient_key = MainSecretKey::random();
        let recipient_main_pubkey = recipient_key.main_pubkey();
        let to = vec![(AttoTokens::from_u64(send_amount), recipient_main_pubkey)];
        let created_cash_notes = sender.local_send(to, None)?;

        assert_eq!(1, created_cash_notes.len());
        assert_eq!(
            Amount::from(GENESIS_CASHNOTE_AMOUNT - send_amount),
            sender.balance().as_atto()
        );

        let recipient_cash_note = &created_cash_notes[0];
        assert_eq!(AttoTokens::from_u64(send_amount), recipient_cash_note.value());
        assert_eq!(&recipient_main_pubkey, recipient_cash_note.main_pubkey());

        Ok(())
    }

    #[tokio::test]
    async fn send_wallet_to_and_from_file() -> Result<()> {
        let dir = create_temp_dir();
        let root_dir = dir.path().to_path_buf();

        let new_wallet = MainSecretKey::random();
        let mut sender = HotWallet::create_from_key(&root_dir, new_wallet, None)?;

        let sender_cash_note =
            create_first_cash_note_from_key(&sender.key).expect("Genesis creation to succeed.");
        sender.deposit_and_store_to_disk(&vec![sender_cash_note])?;

        // We send to a new address.
        let send_amount = 100;
        let recipient_key = MainSecretKey::random();
        let recipient_main_pubkey = recipient_key.main_pubkey();
        let to = vec![(AttoTokens::from_u64(send_amount), recipient_main_pubkey)];
        let _created_cash_notes = sender.local_send(to, None)?;

        let deserialized = HotWallet::load_from(&root_dir)?;

        assert_eq!(sender.address(), deserialized.address());
        assert_eq!(
            Amount::from(GENESIS_CASHNOTE_AMOUNT - send_amount),
            sender.balance().as_atto()
        );
        assert_eq!(
            Amount::from(GENESIS_CASHNOTE_AMOUNT - send_amount),
            deserialized.balance().as_atto()
        );

        assert_eq!(1, sender.watchonly_wallet.available_cash_notes().len());

        assert_eq!(
            1,
            deserialized.watchonly_wallet.available_cash_notes().len()
        );

        let a_available = sender
            .watchonly_wallet
            .available_cash_notes()
            .values()
            .last()
            .expect("There to be an available CashNote.");
        let b_available = deserialized
            .watchonly_wallet
            .available_cash_notes()
            .values()
            .last()
            .expect("There to be an available CashNote.");
        assert_eq!(a_available, b_available);

        Ok(())
    }

    #[tokio::test]
    async fn store_created_cash_note_gives_file_that_try_load_cash_notes_can_use() -> Result<()> {
        let sender_root_dir = create_temp_dir();
        let sender_root_dir = sender_root_dir.path().to_path_buf();
        let new_wallet = MainSecretKey::random();
        let mut sender = HotWallet::create_from_key(&sender_root_dir, new_wallet, None)?;

        let sender_cash_note =
            create_first_cash_note_from_key(&sender.key).expect("Genesis creation to succeed.");
        sender.deposit_and_store_to_disk(&vec![sender_cash_note])?;

        let send_amount = 100;

        // Send to a new address.
        let recipient_root_dir = create_temp_dir();
        let recipient_root_dir = recipient_root_dir.path().to_path_buf();

        let new_wallet = MainSecretKey::random();
        let mut recipient = HotWallet::create_from_key(&recipient_root_dir, new_wallet, None)?;

        let recipient_main_pubkey = recipient.key.main_pubkey();

        let to = vec![(AttoTokens::from_u64(send_amount), recipient_main_pubkey)];
        let created_cash_notes = sender.local_send(to, None)?;
        let cash_note = created_cash_notes[0].clone();
        let unique_pubkey = cash_note.unique_pubkey();
        sender.store_cash_notes_to_disk(&[cash_note])?;

        let unique_pubkey_name = *SpendAddress::from_unique_pubkey(&unique_pubkey).xorname();
        let unique_pubkey_file_name = format!("{}.cash_note", hex::encode(unique_pubkey_name));

        let created_cash_notes_dir = sender_root_dir.join(WALLET_DIR_NAME).join("cash_notes");
        let created_cash_note_file = created_cash_notes_dir.join(&unique_pubkey_file_name);

        let received_cash_note_dir = recipient_root_dir.join(WALLET_DIR_NAME).join("cash_notes");

        std::fs::create_dir_all(&received_cash_note_dir)?;
        let received_cash_note_file = received_cash_note_dir.join(&unique_pubkey_file_name);

        // Move the created cash_note to the recipient's received_cash_notes dir.
        std::fs::rename(created_cash_note_file, received_cash_note_file)?;

        assert_eq!(Amount::from(0), recipient.balance().as_atto());

        recipient.try_load_cash_notes()?;

        assert_eq!(1, recipient.watchonly_wallet.available_cash_notes().len());

        let available = recipient
            .watchonly_wallet
            .available_cash_notes()
            .keys()
            .last()
            .expect("There to be an available CashNote.");

        assert_eq!(available, &unique_pubkey);
        assert_eq!(Amount::from(send_amount), recipient.balance().as_atto());

        Ok(())
    }

    #[tokio::test]
    async fn test_local_send_storage_payment_returns_correct_cost() -> Result<()> {
        let dir = create_temp_dir();
        let root_dir = dir.path().to_path_buf();

        let new_wallet = MainSecretKey::random();
        let mut sender = HotWallet::create_from_key(&root_dir, new_wallet, None)?;

        let sender_cash_note =
            create_first_cash_note_from_key(&sender.key).expect("Genesis creation to succeed.");
        sender.deposit_and_store_to_disk(&vec![sender_cash_note])?;

        let mut rng = bls::rand::thread_rng();
        let xor1 = XorName::random(&mut rng);
        let xor2 = XorName::random(&mut rng);
        let xor3 = XorName::random(&mut rng);
        let xor4 = XorName::random(&mut rng);

        let key1a = dummy_address();
        let key2a = dummy_address();
        let key3a = dummy_address();
        let key4a = dummy_address();

        let map = BTreeMap::from([
            (
                xor1,
                (key1a, PaymentQuote::test_dummy(xor1, 100.into()), vec![]),
            ),
            (
                xor2,
                (key2a, PaymentQuote::test_dummy(xor2, 200.into()), vec![]),
            ),
            (
                xor3,
                (key3a, PaymentQuote::test_dummy(xor3, 300.into()), vec![]),
            ),
            (
                xor4,
                (key4a, PaymentQuote::test_dummy(xor4, 400.into()), vec![]),
            ),
        ]);

        let (price, _) = sender.send_storage_payment(&map).await?;

        let expected_price: Amount = map.values().map(|(_, quote, _)| quote.cost.as_atto()).sum();
        assert_eq!(price.as_atto(), expected_price);

        Ok(())
    }

    /// --------------------------------
    /// <-------> Encryption <--------->
    /// --------------------------------

    #[test]
    fn test_encrypting_existing_unencrypted_wallet() -> Result<()> {
        let password: &'static str = "safenetwork";
        let wrong_password: &'static str = "unsafenetwork";

        let dir = create_temp_dir();
        let root_dir = dir.path().to_path_buf();
        let wallet_key = MainSecretKey::random();

        let unencrypted_wallet = HotWallet::create_from_key(&root_dir, wallet_key, None)?;

        HotWallet::encrypt(&root_dir, password)?;

        let mut encrypted_wallet =
            HotWallet::load_encrypted_from_path(&root_dir, password.to_owned())?;

        // Should fail when not authenticated with password yet
        assert!(encrypted_wallet.authenticate().is_err());

        // Authentication should fail with wrong password
        assert!(encrypted_wallet
            .authenticate_with_password(wrong_password.to_owned())
            .is_err());

        encrypted_wallet.authenticate_with_password(password.to_owned())?;

        encrypted_wallet.reload()?;

        assert_eq!(encrypted_wallet.address(), unencrypted_wallet.address());

        Ok(())
    }

    /// --------------------------------
    /// <-------> Other <--------->
    /// --------------------------------

    #[test]
    fn test_stashing_and_unstashing() -> Result<()> {
        let dir = create_temp_dir();
        let root_dir = dir.path().to_path_buf();
        let wallet_key = MainSecretKey::random();
        let wallet = HotWallet::create_from_key(&root_dir, wallet_key, None)?;
        let pub_key_hex_str = wallet.address().to_hex();

        // Stash wallet
        HotWallet::stash(&root_dir)?;

        // There should be no active wallet now
        assert!(HotWallet::load_from(&root_dir).is_err());

        // Unstash wallet
        HotWallet::unstash(&root_dir, &pub_key_hex_str)?;

        let unstashed_wallet = HotWallet::load_from(&root_dir)?;

        assert_eq!(unstashed_wallet.address().to_hex(), pub_key_hex_str);

        Ok(())
    }

    fn create_temp_dir() -> TempDir {
        TempDir::new().expect("Should be able to create a temp dir.")
    }
}
