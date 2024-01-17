// Copyright 2023 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.
use super::{
    data_payments::{PaymentDetails, PaymentQuote},
    keys::{get_main_key, store_new_keypair},
    wallet_file::{
        get_unconfirmed_spend_requests, load_cash_notes_from_disk, load_created_cash_note,
        remove_cash_notes, store_created_cash_notes, store_unconfirmed_spend_requests,
    },
    watch_only::WatchOnlyWallet,
    Error, Result,
};

use crate::{
    calculate_royalties_fee,
    transfers::{
        create_offline_transfer, create_unsigned_transaction, offline_transfer_from_transaction,
        OfflineTransfer, UnsignedTransfer,
    },
    CashNote, CashNoteRedemption, DerivationIndex, DerivedSecretKey, Hash, MainPubkey,
    MainSecretKey, NanoTokens, SignedSpend, Spend, Transaction, Transfer, UniquePubkey,
    WalletError, NETWORK_ROYALTIES_PK,
};
use xor_name::XorName;

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs::File,
    path::{Path, PathBuf},
};

const WALLET_DIR_NAME: &str = "wallet";

/// A locked file handle, that when dropped releases the lock.
pub type WalletExclusiveAccess = File;

/// A wallet that can only receive tokens.
pub struct LocalWallet {
    /// The secret key with which we can access
    /// all the tokens in the available_cash_notes.
    key: MainSecretKey,
    /// The wallet containing all data.
    watchonly_wallet: WatchOnlyWallet,
    /// These have not yet been successfully sent to the network
    /// and need to be, to reach network validity.
    unconfirmed_spend_requests: BTreeSet<SignedSpend>,
}

impl LocalWallet {
    /// Stores the wallet to disk.
    /// This requires having exclusive access to the wallet to prevent concurrent processes from writing to it
    fn store(&self, exclusive_access: WalletExclusiveAccess) -> Result<()> {
        self.watchonly_wallet.store(exclusive_access)
    }

    /// reloads the wallet from disk.
    fn reload(&mut self) -> Result<()> {
        // placeholder random MainSecretKey to take it out
        let current_key = std::mem::replace(&mut self.key, MainSecretKey::random());
        let wallet =
            Self::load_from_path_and_key(self.watchonly_wallet.wallet_dir(), Some(current_key))?;

        // and move the original back in
        *self = wallet;
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

    /// Remove CashNote from available_cash_notes and add it to spent_cash_notes.
    pub fn mark_note_as_spent(&mut self, cash_note_id: UniquePubkey) {
        self.watchonly_wallet.mark_notes_as_spent(&[cash_note_id]);
    }

    pub fn unconfirmed_spend_requests_exist(&self) -> bool {
        !self.unconfirmed_spend_requests.is_empty()
    }

    /// Try to load any new cash_notes from the `cash_notes dir` in the wallet dir.
    pub fn try_load_cash_notes(&mut self) -> Result<()> {
        let deposited = load_cash_notes_from_disk(self.watchonly_wallet.wallet_dir())?;
        self.deposit_and_store_to_disk(&deposited)?;
        Ok(())
    }

    /// Loads a serialized wallet from a path and given main key.
    pub fn load_from_main_key(root_dir: &Path, main_key: MainSecretKey) -> Result<Self> {
        let wallet_dir = root_dir.join(WALLET_DIR_NAME);
        // This creates the received_cash_notes dir if it doesn't exist.
        std::fs::create_dir_all(&wallet_dir)?;
        // This creates the main_key file if it doesn't exist.
        Self::load_from_path_and_key(&wallet_dir, Some(main_key))
    }

    /// Creates a serialized wallet for a path and main key.
    /// This will overwrite any existing wallet, unlike load_from_main_key
    pub fn create_from_key(root_dir: &Path, key: MainSecretKey) -> Result<Self> {
        let wallet_dir = root_dir.join(WALLET_DIR_NAME);
        // This creates the received_cash_notes dir if it doesn't exist.
        std::fs::create_dir_all(&wallet_dir)?;
        // Create the new wallet for this key
        store_new_keypair(&wallet_dir, &key)?;
        let unconfirmed_spend_requests = match get_unconfirmed_spend_requests(&wallet_dir)? {
            Some(unconfirmed_spend_requests) => unconfirmed_spend_requests,
            None => Default::default(),
        };
        let watchonly_wallet = WatchOnlyWallet::load_from(&wallet_dir, key.main_pubkey())?;

        Ok(Self {
            key,
            watchonly_wallet,
            unconfirmed_spend_requests,
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
        Self::load_from_path_and_key(&wallet_dir, None)
    }

    /// Loads a serialized wallet from a given path, no additional element will
    /// be added to the provided path and strictly taken as the wallet files location.
    pub fn load_from_path(wallet_dir: &Path, main_key: Option<MainSecretKey>) -> Result<Self> {
        std::fs::create_dir_all(wallet_dir)?;
        Self::load_from_path_and_key(wallet_dir, main_key)
    }

    pub fn address(&self) -> MainPubkey {
        self.key.main_pubkey()
    }

    pub fn unconfirmed_spend_requests(&self) -> &BTreeSet<SignedSpend> {
        &self.unconfirmed_spend_requests
    }

    /// Moves all files for the current wallet, including keys and cashnotes
    /// to directory root_dir/wallet_<short_address>
    pub fn clear(root_dir: &Path) -> Result<PathBuf> {
        let wallet = LocalWallet::load_from(root_dir)?;
        let wallet_dir = root_dir.join(WALLET_DIR_NAME);
        let addr_short = &format!("{:?}", wallet.address())[0..10];
        let new_name = format!("{WALLET_DIR_NAME}_{addr_short}");
        let moved_dir = root_dir.join(new_name);
        let _ = std::fs::rename(wallet_dir, moved_dir.clone());
        Ok(moved_dir)
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
        self.unconfirmed_spend_requests = Default::default();
    }

    pub fn balance(&self) -> NanoTokens {
        self.watchonly_wallet.balance()
    }

    pub fn sign(
        &self,
        spends: impl IntoIterator<Item = (Spend, DerivationIndex)>,
    ) -> BTreeSet<SignedSpend> {
        spends
            .into_iter()
            .map(|(spend, dindex)| {
                let derived_sk = self.key.derive_key(&dindex);
                let derived_key_sig = derived_sk.sign(&spend.to_bytes());
                SignedSpend {
                    spend,
                    derived_key_sig,
                }
            })
            .collect()
    }

    /// Returns all available cash_notes and an exclusive access to the wallet so no concurrent processes can
    /// get available cash_notes while we're modifying the wallet
    /// once the updated wallet is stored to disk it is safe to drop the WalletExclusiveAccess
    pub fn available_cash_notes(
        &mut self,
    ) -> Result<(
        Vec<(CashNote, Option<DerivedSecretKey>, DerivationIndex)>,
        WalletExclusiveAccess,
    )> {
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
                if let Ok(derived_key) = cash_note.derived_key(&self.key) {
                    available_cash_notes.push((
                        cash_note.clone(),
                        Some(derived_key),
                        cash_note.derivation_index,
                    ));
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

    /// Return the payment cash_note ids for the given content address name if cached.
    pub fn get_cached_payment_for_xorname(&self, name: &XorName) -> Option<&PaymentDetails> {
        self.watchonly_wallet.get_payment_transaction(name)
    }

    pub fn build_unsigned_transaction(
        &mut self,
        to: Vec<(NanoTokens, MainPubkey)>,
        reason_hash: Option<Hash>,
    ) -> Result<UnsignedTransfer> {
        let mut rng = &mut rand::rngs::OsRng;
        // create a unique key for each output
        let to_unique_keys: Vec<_> = to
            .into_iter()
            .map(|(amount, address)| (amount, address, DerivationIndex::random(&mut rng)))
            .collect();

        trace!("Trying to lock wallet to get available cash_notes...");
        // lock and load from disk to make sure we're up to date and others can't modify the wallet concurrently
        let exclusive_access = self.lock()?;
        self.reload()?;
        trace!("Wallet locked and loaded!");

        // get the available cash_notes
        let mut available_cash_notes = vec![];
        let wallet_dir = self.watchonly_wallet.wallet_dir().to_path_buf();
        for (id, _token) in self.watchonly_wallet.available_cash_notes().iter() {
            if let Some(cash_note) = load_created_cash_note(id, &wallet_dir) {
                available_cash_notes.push((cash_note.clone(), None, cash_note.derivation_index));
            } else {
                warn!("Skipping CashNote {:?} because we don't have it", id);
            }
        }
        debug!(
            "Available CashNotes for local send: {:#?}",
            available_cash_notes
        );

        let reason_hash = reason_hash.unwrap_or_default();

        let unsigned_transfer = create_unsigned_transaction(
            available_cash_notes,
            to_unique_keys,
            self.address(),
            reason_hash,
        )?;

        trace!("Releasing wallet lock"); // by dropping exclusive_access
        std::mem::drop(exclusive_access);

        Ok(unsigned_transfer)
    }

    /// Make a transfer and return all created cash_notes
    pub fn local_send(
        &mut self,
        to: Vec<(NanoTokens, MainPubkey)>,
        reason_hash: Option<Hash>,
    ) -> Result<Vec<CashNote>> {
        let mut rng = &mut rand::rngs::OsRng;
        // create a unique key for each output
        let to_unique_keys: Vec<_> = to
            .into_iter()
            .map(|(amount, address)| (amount, address, DerivationIndex::random(&mut rng)))
            .collect();

        let (available_cash_notes, exclusive_access) = self.available_cash_notes()?;
        debug!(
            "Available CashNotes for local send: {:#?}",
            available_cash_notes
        );

        let reason_hash = reason_hash.unwrap_or_default();

        let transfer = create_offline_transfer(
            available_cash_notes,
            to_unique_keys,
            self.address(),
            reason_hash,
        )?;

        let created_cash_notes = transfer.created_cash_notes.clone();

        self.update_local_wallet(transfer, exclusive_access)?;

        trace!("Releasing wallet lock"); // by dropping _exclusive_access
        Ok(created_cash_notes)
    }

    /// Prepare a signed transaction in local wallet and return all created cash_notes
    pub fn prepare_signed_transfer(
        &mut self,
        signed_spends: BTreeSet<SignedSpend>,
        tx: Transaction,
        change_id: UniquePubkey,
        output_details: BTreeMap<UniquePubkey, (MainPubkey, DerivationIndex)>,
    ) -> Result<Vec<CashNote>> {
        let transfer =
            offline_transfer_from_transaction(signed_spends, tx, change_id, output_details)?;

        let created_cash_notes = transfer.created_cash_notes.clone();

        trace!("Trying to lock wallet to get available cash_notes...");
        // lock and load from disk to make sure we're up to date and others can't modify the wallet concurrently
        let exclusive_access = self.lock()?;
        self.reload()?;
        trace!("Wallet locked and loaded!");

        self.update_local_wallet(transfer, exclusive_access)?;

        trace!("Releasing wallet lock"); // by dropping _exclusive_access
        Ok(created_cash_notes)
    }

    /// Performs a payment for each content address.
    /// Includes payment of network royalties.
    /// Returns the amount paid for storage, including the network royalties fee paid.
    pub fn local_send_storage_payment(
        &mut self,
        price_map: &BTreeMap<XorName, (MainPubkey, PaymentQuote)>,
    ) -> Result<(NanoTokens, NanoTokens)> {
        let mut rng = &mut rand::thread_rng();
        let mut storage_cost = NanoTokens::zero();
        let mut royalties_fees = NanoTokens::zero();

        // create random derivation indexes for recipients
        let mut recipients_by_xor = BTreeMap::new();
        for (xorname, (main_pubkey, quote)) in price_map.iter() {
            let storage_payee = (quote.cost, *main_pubkey, DerivationIndex::random(&mut rng));
            let royalties_fee = calculate_royalties_fee(quote.cost);
            let royalties_payee = (
                royalties_fee,
                *NETWORK_ROYALTIES_PK,
                DerivationIndex::random(&mut rng),
            );

            storage_cost = storage_cost
                .checked_add(quote.cost)
                .ok_or(WalletError::TotalPriceTooHigh)?;
            royalties_fees = royalties_fees
                .checked_add(royalties_fee)
                .ok_or(WalletError::TotalPriceTooHigh)?;

            recipients_by_xor.insert(xorname, (storage_payee, royalties_payee));
        }

        // create offline transfers
        let recipients = recipients_by_xor
            .values()
            .flat_map(|(node, roy)| vec![node, roy])
            .cloned()
            .collect();
        let (available_cash_notes, exclusive_access) = self.available_cash_notes()?;
        debug!("Available CashNotes: {:#?}", available_cash_notes);
        let reason_hash = Default::default();
        let offline_transfer = create_offline_transfer(
            available_cash_notes,
            recipients,
            self.address(),
            reason_hash,
        )?;

        // cache transfer payments in the wallet
        let mut cashnotes_to_use: HashSet<CashNote> = offline_transfer
            .created_cash_notes
            .iter()
            .cloned()
            .collect();
        for (xorname, recipients_info) in recipients_by_xor {
            let (storage_payee, royalties_payee) = recipients_info;
            let node_key = storage_payee.1;
            let pay_amount = storage_payee.0;
            let cash_note_for_node = cashnotes_to_use
                .iter()
                .find(|cash_note| {
                    cash_note.value() == Ok(pay_amount) && cash_note.main_pubkey() == &node_key
                })
                .ok_or(Error::CouldNotSendMoney(format!(
                    "No cashnote found to pay node for {xorname:?}"
                )))?
                .clone();
            cashnotes_to_use.remove(&cash_note_for_node);
            let transfer_amount = cash_note_for_node.value()?;
            let transfer_for_node = Transfer::transfer_from_cash_note(&cash_note_for_node)?;
            trace!("Created transaction regarding {xorname:?} paying {transfer_amount:?} to {node_key:?}.");

            let royalties_key = royalties_payee.1;
            let royalties_amount = royalties_payee.0;
            let cash_note_for_royalties = cashnotes_to_use
                .iter()
                .find(|cash_note| {
                    cash_note.value() == Ok(royalties_amount)
                        && cash_note.main_pubkey() == &royalties_key
                })
                .ok_or(Error::CouldNotSendMoney(format!(
                    "No cashnote found to pay royalties for {xorname:?}"
                )))?
                .clone();
            cashnotes_to_use.remove(&cash_note_for_royalties);
            let royalties = Transfer::royalties_transfer_from_cash_note(&cash_note_for_royalties)?;
            let royalties_amount = cash_note_for_royalties.value()?;
            trace!("Created network royalties cnr regarding {xorname:?} paying {royalties_amount:?} to {royalties_key:?}.");

            let quote = price_map
                .get(xorname)
                .ok_or(Error::CouldNotSendMoney(format!(
                    "No quote found for {xorname:?}"
                )))?
                .1
                .clone();
            let payment = PaymentDetails {
                recipient: node_key,
                transfer: (transfer_for_node, transfer_amount),
                royalties: (royalties, royalties_amount),
                quote,
            };

            self.watchonly_wallet
                .insert_payment_transaction(*xorname, payment);
        }

        // write all changes to local wallet
        self.update_local_wallet(offline_transfer, exclusive_access)?;
        Ok((storage_cost, royalties_fees))
    }

    fn update_local_wallet(
        &mut self,
        transfer: OfflineTransfer,
        exclusive_access: WalletExclusiveAccess,
    ) -> Result<()> {
        // First of all, update client local state.
        let spent_unique_pubkeys: BTreeSet<_> = transfer
            .tx
            .inputs
            .iter()
            .map(|input| input.unique_pubkey())
            .collect();

        self.watchonly_wallet
            .mark_notes_as_spent(spent_unique_pubkeys.clone());

        if let Some(cash_note) = transfer.change_cash_note {
            self.watchonly_wallet.deposit(&[cash_note.clone()])?;
            self.store_cash_notes_to_disk(&[cash_note])?;
        }

        // Store created CashNotes in a batch, improving IO performance
        self.store_cash_notes_to_disk(&transfer.created_cash_notes)?;

        for request in transfer.all_spend_requests {
            self.unconfirmed_spend_requests.insert(request);
        }

        // store wallet to disk
        self.store(exclusive_access)?;
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
    fn load_from_path_and_key(wallet_dir: &Path, main_key: Option<MainSecretKey>) -> Result<Self> {
        let key = match get_main_key(wallet_dir)? {
            Some(key) => key,
            None => {
                let key = main_key.unwrap_or(MainSecretKey::random());
                store_new_keypair(wallet_dir, &key)?;
                warn!("No main key found when loading wallet from path, generating a new one with pubkey: {:?}", key.main_pubkey());
                key
            }
        };
        let unconfirmed_spend_requests = match get_unconfirmed_spend_requests(wallet_dir)? {
            Some(unconfirmed_spend_requests) => unconfirmed_spend_requests,
            None => Default::default(),
        };
        let watchonly_wallet = WatchOnlyWallet::load_from(wallet_dir, key.main_pubkey())?;

        Ok(Self {
            key,
            watchonly_wallet,
            unconfirmed_spend_requests,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::LocalWallet;
    use crate::{
        genesis::{create_first_cash_note_from_key, GENESIS_CASHNOTE_AMOUNT},
        wallet::{
            data_payments::PaymentQuote,
            local_store::WALLET_DIR_NAME,
            wallet_file::{get_wallet, store_wallet},
            watch_only::WatchOnlyWallet,
            KeyLessWallet,
        },
        MainSecretKey, NanoTokens, SpendAddress,
    };
    use assert_fs::TempDir;
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
            .insert(genesis.unique_pubkey(), genesis.value()?);

        store_wallet(&wallet_dir, &wallet)?;

        let deserialized = get_wallet(&wallet_dir)?.expect("There to be a wallet on disk.");

        assert_eq!(GENESIS_CASHNOTE_AMOUNT, wallet.balance().as_nano());
        assert_eq!(GENESIS_CASHNOTE_AMOUNT, deserialized.balance().as_nano());

        Ok(())
    }

    #[test]
    fn wallet_basics() -> Result<()> {
        let key = MainSecretKey::random();
        let main_pubkey = key.main_pubkey();
        let dir = create_temp_dir();

        let deposit_only = LocalWallet {
            key,
            watchonly_wallet: WatchOnlyWallet::new(main_pubkey, &dir, KeyLessWallet::default()),
            unconfirmed_spend_requests: Default::default(),
        };

        assert_eq!(main_pubkey, deposit_only.address());
        assert_eq!(NanoTokens::zero(), deposit_only.balance());

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

        let mut deposit_only = LocalWallet {
            key,
            watchonly_wallet: WatchOnlyWallet::new(main_pubkey, &dir, KeyLessWallet::default()),
            unconfirmed_spend_requests: Default::default(),
        };

        deposit_only.deposit_and_store_to_disk(&vec![])?;

        assert_eq!(NanoTokens::zero(), deposit_only.balance());

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

        let mut deposit_only = LocalWallet {
            key,
            watchonly_wallet: WatchOnlyWallet::new(main_pubkey, &dir, KeyLessWallet::default()),
            unconfirmed_spend_requests: Default::default(),
        };

        deposit_only.deposit_and_store_to_disk(&vec![genesis])?;

        assert_eq!(GENESIS_CASHNOTE_AMOUNT, deposit_only.balance().as_nano());

        Ok(())
    }

    #[tokio::test]
    async fn deposit_does_not_add_cash_notes_not_belonging_to_the_wallet() -> Result<()> {
        let key = MainSecretKey::random();
        let main_pubkey = key.main_pubkey();
        let genesis = create_first_cash_note_from_key(&MainSecretKey::random())
            .expect("Genesis creation to succeed.");
        let dir = create_temp_dir();

        let mut local_wallet = LocalWallet {
            key,
            watchonly_wallet: WatchOnlyWallet::new(main_pubkey, &dir, KeyLessWallet::default()),
            unconfirmed_spend_requests: Default::default(),
        };

        local_wallet.deposit_and_store_to_disk(&vec![genesis])?;

        assert_eq!(NanoTokens::zero(), local_wallet.balance());

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

        let mut deposit_only = LocalWallet {
            key,
            watchonly_wallet: WatchOnlyWallet::new(main_pubkey, &dir, KeyLessWallet::default()),
            unconfirmed_spend_requests: Default::default(),
        };

        deposit_only.deposit_and_store_to_disk(&vec![genesis_0.clone()])?;
        assert_eq!(GENESIS_CASHNOTE_AMOUNT, deposit_only.balance().as_nano());

        deposit_only.deposit_and_store_to_disk(&vec![genesis_0])?;
        assert_eq!(GENESIS_CASHNOTE_AMOUNT, deposit_only.balance().as_nano());

        deposit_only.deposit_and_store_to_disk(&vec![genesis_1])?;
        assert_eq!(GENESIS_CASHNOTE_AMOUNT, deposit_only.balance().as_nano());

        Ok(())
    }

    #[tokio::test]
    async fn deposit_wallet_to_and_from_file() -> Result<()> {
        let dir = create_temp_dir();
        let root_dir = dir.path().to_path_buf();

        let mut depositor = LocalWallet::load_from(&root_dir)?;
        let genesis =
            create_first_cash_note_from_key(&depositor.key).expect("Genesis creation to succeed.");
        depositor.deposit_and_store_to_disk(&vec![genesis])?;

        let deserialized = LocalWallet::load_from(&root_dir)?;

        assert_eq!(depositor.address(), deserialized.address());
        assert_eq!(GENESIS_CASHNOTE_AMOUNT, depositor.balance().as_nano());
        assert_eq!(GENESIS_CASHNOTE_AMOUNT, deserialized.balance().as_nano());

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

        let mut sender = LocalWallet::load_from(&root_dir)?;
        let sender_cash_note =
            create_first_cash_note_from_key(&sender.key).expect("Genesis creation to succeed.");
        sender.deposit_and_store_to_disk(&vec![sender_cash_note])?;

        assert_eq!(GENESIS_CASHNOTE_AMOUNT, sender.balance().as_nano());

        // We send to a new address.
        let send_amount = 100;
        let recipient_key = MainSecretKey::random();
        let recipient_main_pubkey = recipient_key.main_pubkey();
        let to = vec![(NanoTokens::from(send_amount), recipient_main_pubkey)];
        let created_cash_notes = sender.local_send(to, None)?;

        assert_eq!(1, created_cash_notes.len());
        assert_eq!(
            GENESIS_CASHNOTE_AMOUNT - send_amount,
            sender.balance().as_nano()
        );

        let recipient_cash_note = &created_cash_notes[0];
        assert_eq!(NanoTokens::from(send_amount), recipient_cash_note.value()?);
        assert_eq!(&recipient_main_pubkey, recipient_cash_note.main_pubkey());

        Ok(())
    }

    #[tokio::test]
    async fn send_wallet_to_and_from_file() -> Result<()> {
        let dir = create_temp_dir();
        let root_dir = dir.path().to_path_buf();

        let mut sender = LocalWallet::load_from(&root_dir)?;
        let sender_cash_note =
            create_first_cash_note_from_key(&sender.key).expect("Genesis creation to succeed.");
        sender.deposit_and_store_to_disk(&vec![sender_cash_note])?;

        // We send to a new address.
        let send_amount = 100;
        let recipient_key = MainSecretKey::random();
        let recipient_main_pubkey = recipient_key.main_pubkey();
        let to = vec![(NanoTokens::from(send_amount), recipient_main_pubkey)];
        let _created_cash_notes = sender.local_send(to, None)?;

        let deserialized = LocalWallet::load_from(&root_dir)?;

        assert_eq!(sender.address(), deserialized.address());
        assert_eq!(
            GENESIS_CASHNOTE_AMOUNT - send_amount,
            sender.balance().as_nano()
        );
        assert_eq!(
            GENESIS_CASHNOTE_AMOUNT - send_amount,
            deserialized.balance().as_nano()
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

        let mut sender = LocalWallet::load_from(&sender_root_dir)?;
        let sender_cash_note =
            create_first_cash_note_from_key(&sender.key).expect("Genesis creation to succeed.");
        sender.deposit_and_store_to_disk(&vec![sender_cash_note])?;

        let send_amount = 100;

        // Send to a new address.
        let recipient_root_dir = create_temp_dir();
        let recipient_root_dir = recipient_root_dir.path().to_path_buf();
        let mut recipient = LocalWallet::load_from(&recipient_root_dir)?;
        let recipient_main_pubkey = recipient.key.main_pubkey();

        let to = vec![(NanoTokens::from(send_amount), recipient_main_pubkey)];
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

        assert_eq!(0, recipient.balance().as_nano());

        recipient.try_load_cash_notes()?;

        assert_eq!(1, recipient.watchonly_wallet.available_cash_notes().len());

        let available = recipient
            .watchonly_wallet
            .available_cash_notes()
            .keys()
            .last()
            .expect("There to be an available CashNote.");

        assert_eq!(available, &unique_pubkey);
        assert_eq!(send_amount, recipient.balance().as_nano());

        Ok(())
    }

    #[tokio::test]
    async fn test_local_send_storage_payment_returns_correct_cost() -> Result<()> {
        let dir = create_temp_dir();
        let root_dir = dir.path().to_path_buf();

        let mut sender = LocalWallet::load_from(&root_dir)?;
        let sender_cash_note =
            create_first_cash_note_from_key(&sender.key).expect("Genesis creation to succeed.");
        sender.deposit_and_store_to_disk(&vec![sender_cash_note])?;

        let mut rng = bls::rand::thread_rng();
        let xor1 = XorName::random(&mut rng);
        let xor2 = XorName::random(&mut rng);
        let xor3 = XorName::random(&mut rng);
        let xor4 = XorName::random(&mut rng);

        let key1a = MainSecretKey::random().main_pubkey();
        let key2a = MainSecretKey::random().main_pubkey();
        let key3a = MainSecretKey::random().main_pubkey();
        let key4a = MainSecretKey::random().main_pubkey();

        let map = BTreeMap::from([
            (xor1, (key1a, PaymentQuote::test_dummy(xor1, 100.into()))),
            (xor2, (key2a, PaymentQuote::test_dummy(xor2, 200.into()))),
            (xor3, (key3a, PaymentQuote::test_dummy(xor3, 300.into()))),
            (xor4, (key4a, PaymentQuote::test_dummy(xor4, 400.into()))),
        ]);

        let (price, _) = sender.local_send_storage_payment(&map)?;

        let expected_price: u64 = map.values().map(|(_, quote)| quote.cost.as_nano()).sum();
        assert_eq!(price.as_nano(), expected_price);

        Ok(())
    }

    fn create_temp_dir() -> TempDir {
        TempDir::new().expect("Should be able to create a temp dir.")
    }
}
