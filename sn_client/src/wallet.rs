// Copyright 2023 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::Error;

use super::{error::Result, Client};
use backoff::{backoff::Backoff, ExponentialBackoff};
use futures::{future::join_all, TryFutureExt};
use libp2p::PeerId;
use sn_networking::GetRecordError;
use sn_protocol::NetworkAddress;
use sn_transfers::{
    CashNote, DerivationIndex, LocalWallet, MainPubkey, NanoTokens, Payment, PaymentQuote,
    SignedSpend, SpendAddress, Transaction, Transfer, UniquePubkey, WalletError, WalletResult,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    iter::Iterator,
    time::Duration,
};
use tokio::{task::JoinSet, time::sleep};
use xor_name::XorName;
/// A wallet client can be used to send and
/// receive tokens to/from other wallets.
pub struct WalletClient {
    client: Client,
    wallet: LocalWallet,
}

impl WalletClient {
    /// Create a new wallet client.
    pub fn new(client: Client, wallet: LocalWallet) -> Self {
        Self { client, wallet }
    }

    /// Stores the wallet to disk.
    pub fn store_local_wallet(&mut self) -> WalletResult<()> {
        self.wallet.deposit_and_store_to_disk(&vec![])
    }

    /// Get the wallet balance
    pub fn balance(&self) -> NanoTokens {
        self.wallet.balance()
    }

    /// Do we have any unconfirmed transactions?
    pub fn unconfirmed_spend_requests_exist(&self) -> bool {
        self.wallet.unconfirmed_spend_requests_exist()
    }
    /// Get unconfirmed txs
    pub fn unconfirmed_spend_requests(&self) -> &BTreeSet<SignedSpend> {
        self.wallet.unconfirmed_spend_requests()
    }

    /// Get the payment for a given network address
    pub fn get_payment_for_addr(&self, address: &NetworkAddress) -> WalletResult<Payment> {
        match &address.as_xorname() {
            Some(xorname) => {
                let payment_details = self
                    .wallet
                    .get_cached_payment_for_xorname(xorname)
                    .ok_or(WalletError::NoPaymentForAddress)?;
                let payment = payment_details.to_payment();
                debug!("Payment retrieved for {xorname:?} from wallet: {payment:?}");
                info!("Payment retrieved for {xorname:?} from wallet");
                Ok(payment)
            }
            None => Err(WalletError::InvalidAddressType),
        }
    }

    /// Remove CashNote from available_cash_notes
    pub fn mark_note_as_spent(&mut self, cash_note_key: UniquePubkey) {
        self.wallet.mark_note_as_spent(cash_note_key);
    }

    /// Send tokens to another wallet.
    /// Can optionally verify the store has been successful (this will attempt to GET the Spend from the network)
    pub async fn send_cash_note(
        &mut self,
        amount: NanoTokens,
        to: MainPubkey,
        verify_store: bool,
    ) -> WalletResult<CashNote> {
        let created_cash_notes = self.wallet.local_send(vec![(amount, to)], None)?;

        // send to network
        if let Err(error) = self
            .client
            .send_spends(
                self.wallet.unconfirmed_spend_requests().iter(),
                verify_store,
            )
            .await
        {
            return Err(WalletError::CouldNotSendMoney(format!(
                "The transfer was not successfully registered in the network: {error:?}"
            )));
        } else {
            // clear unconfirmed txs
            self.wallet.clear_confirmed_spend_requests();
        }

        // return the first CashNote (assuming there is only one because we only sent to one recipient)
        match &created_cash_notes[..] {
            [cashnote] => Ok(cashnote.clone()),
            [_multiple, ..] => Err(WalletError::CouldNotSendMoney(
                "Multiple CashNotes were returned from the transaction when only one was expected. This is a BUG."
                    .into(),
            )),
            [] => Err(WalletError::CouldNotSendMoney(
                "No CashNotes were returned from the wallet.".into(),
            )),
        }
    }

    /// Send signed spends to another wallet.
    /// Can optionally verify the store has been successful (this will attempt to GET the Spend from the network)
    pub async fn send_signed_spends(
        &mut self,
        signed_spends: BTreeSet<SignedSpend>,
        tx: Transaction,
        change_id: UniquePubkey,
        output_details: BTreeMap<UniquePubkey, (MainPubkey, DerivationIndex)>,
        verify_store: bool,
    ) -> WalletResult<CashNote> {
        let created_cash_notes =
            self.wallet
                .prepare_signed_transfer(signed_spends, tx, change_id, output_details)?;

        // send to network
        if let Err(error) = self
            .client
            .send_spends(
                self.wallet.unconfirmed_spend_requests().iter(),
                verify_store,
            )
            .await
        {
            return Err(WalletError::CouldNotSendMoney(format!(
                "The transfer was not successfully registered in the network: {error:?}"
            )));
        } else {
            // clear unconfirmed txs
            self.wallet.clear_confirmed_spend_requests();
        }

        // return the first CashNote (assuming there is only one because we only sent to one recipient)
        match &created_cash_notes[..] {
            [cashnote] => Ok(cashnote.clone()),
            [_multiple, ..] => Err(WalletError::CouldNotSendMoney(
                "Multiple CashNotes were returned from the transaction when only one was expected. This is a BUG."
                    .into(),
            )),
            [] => Err(WalletError::CouldNotSendMoney(
                "No CashNotes were returned from the wallet.".into(),
            )),
        }
    }

    /// Get storecost from the network
    /// Returns the MainPubkey of the node to pay and the price in NanoTokens
    pub async fn get_store_cost_at_address(
        &self,
        address: NetworkAddress,
    ) -> WalletResult<(PeerId, MainPubkey, PaymentQuote)> {
        self.client
            .network
            .get_store_costs_from_network(address)
            .await
            .map_err(|error| WalletError::CouldNotSendMoney(error.to_string()))
    }

    /// Send tokens to nodes closest to the data we want to make storage payment for.
    ///
    /// The returned result is: ((storage_cost, royalties_fees), (payee_map, skipped_chunks))
    /// Where:
    ///   `storage_cost` is the total cost for the all contents
    ///   `royalties_fees` is the total royalty fess for the all contents
    ///   `payee_map` is the payees selected for each content
    ///   `skipped_chunks` is the list of content already exists in network and no need to upload
    ///
    /// Note storage cost is _per record_, and it's zero if not required for this operation.
    /// This can optionally verify the store has been successful (this will attempt to GET the cash_note from the network)
    pub async fn pay_for_storage(
        &mut self,
        content_addrs: impl Iterator<Item = NetworkAddress>,
    ) -> WalletResult<(
        (NanoTokens, NanoTokens),
        (Vec<(XorName, PeerId)>, Vec<XorName>),
    )> {
        let verify_store = true;
        let c: Vec<_> = content_addrs.collect();
        // Using default ExponentialBackoff doesn't make sense,
        // as it will just fail after the first payment failure.
        let mut backoff = ExponentialBackoff::default();
        let mut last_err = "No retries".to_string();

        while let Some(delay) = backoff.next_backoff() {
            trace!("Paying for storage (w/backoff retries) for: {:?}", c);
            match self
                .pay_for_storage_once(c.clone().into_iter(), verify_store)
                .await
            {
                Ok(cost) => return Ok(cost),
                Err(WalletError::CouldNotSendMoney(err)) => {
                    warn!("Attempt to pay for data failed: {err:?}");
                    last_err = err;
                    sleep(delay).await;
                }
                Err(err) => return Err(err),
            }
        }
        Err(WalletError::CouldNotSendMoney(last_err))
    }

    /// Existing chunks will have a store cost to be Zero.
    /// The payement procedure shall be skipped, and the chunk upload as well.
    /// Hence the list of existing chunks will be returned.
    async fn pay_for_storage_once(
        &mut self,
        content_addrs: impl Iterator<Item = NetworkAddress>,
        verify_store: bool,
    ) -> WalletResult<(
        (NanoTokens, NanoTokens),
        (Vec<(XorName, PeerId)>, Vec<XorName>),
    )> {
        // get store cost from network in parrallel
        let mut tasks = JoinSet::new();
        for content_addr in content_addrs {
            let client = self.client.clone();
            tasks.spawn(async move {
                let cost = client
                    .network
                    .get_store_costs_from_network(content_addr.clone())
                    .await
                    .map_err(|error| WalletError::CouldNotSendMoney(error.to_string()));

                debug!("Storecosts retrieved for {content_addr:?} {cost:?}");
                (content_addr, cost)
            });
        }
        debug!("Pending store cost tasks: {:?}", tasks.len());

        // collect store costs
        let mut cost_map = BTreeMap::default();
        let mut skipped_chunks = vec![];
        let mut payee_map = vec![];
        while let Some(res) = tasks.join_next().await {
            match res {
                Ok((content_addr, Ok(cost))) => {
                    if let Some(xorname) = content_addr.as_xorname() {
                        if cost.2.cost == NanoTokens::zero() {
                            skipped_chunks.push(xorname);
                            debug!("Skipped existing chunk {content_addr:?}");
                        } else {
                            let _ = cost_map.insert(xorname, (cost.1, cost.2));
                            payee_map.push((xorname, cost.0));
                            debug!("Storecost inserted into payment map for {content_addr:?}");
                        }
                    } else {
                        warn!("Cannot get store cost for a content that is not a data type: {content_addr:?}");
                    }
                }
                Ok((content_addr, Err(err))) => {
                    warn!("Cannot get store cost for {content_addr:?} with error {err:?}");
                    return Err(err);
                }
                Err(e) => {
                    return Err(WalletError::CouldNotSendMoney(format!(
                        "Storecost get task failed: {e:?}"
                    )));
                }
            }
        }
        info!("Storecosts retrieved");

        // pay for records
        Ok((
            self.pay_for_records(&cost_map, verify_store).await?,
            (payee_map, skipped_chunks),
        ))
    }

    /// Send tokens to nodes closest to the data we want to make storage payment for.
    ///
    /// Returns the amount paid for storage, including the network royalties fee paid.
    ///
    /// This can optionally verify the store has been successful (this will attempt to GET the cash_note from the network)
    pub async fn pay_for_records(
        &mut self,
        cost_map: &BTreeMap<XorName, (MainPubkey, PaymentQuote)>,
        verify_store: bool,
    ) -> WalletResult<(NanoTokens, NanoTokens)> {
        // Before wallet progress, there shall be no `unconfirmed_spend_requests`
        // Here, just re-upload again. The caller shall carry out a re-try later on.
        if self.wallet.unconfirmed_spend_requests_exist() {
            info!("Pre-Unconfirmed txs exist, sending again after 1 second...");
            sleep(Duration::from_secs(1)).await;
            self.resend_pending_txs(verify_store).await;

            return Err(WalletError::CouldNotSendMoney(
                "Wallet has pre-unconfirmed tx, resent them and try later.".to_string(),
            ));
        }

        let total_cost = self.wallet.local_send_storage_payment(cost_map)?;

        // send to network
        trace!("Sending storage payment transfer to the network");
        let spend_attempt_result = self
            .client
            .send_spends(
                self.wallet.unconfirmed_spend_requests().iter(),
                verify_store,
            )
            .await;
        // Here is bit risky that for the whole bunch of spends to the chunks' store_costs and royalty_fee
        // they will get re-paid again for ALL, if any one of the payment failed to be put.
        if let Err(error) = spend_attempt_result {
            warn!("The storage payment transfer was not successfully registered in the network: {error:?}. It will be retried later.");

            // if we have a DoubleSpend error, lets remove the CashNote from the wallet
            if let WalletError::DoubleSpendAttemptedForCashNotes(spent_cash_notes) = &error {
                for cash_note_key in spent_cash_notes {
                    warn!("Removing double spends CashNote from wallet: {cash_note_key:?}");
                    self.wallet.mark_note_as_spent(*cash_note_key);
                    self.wallet.clear_specific_spend_request(*cash_note_key);
                }
            }

            self.wallet.store_unconfirmed_spend_requests()?;

            return Err(WalletError::CouldNotSendMoney(format!(
                "The storage payment transfer was not successfully registered in the network: {error:?}"
            )));
        } else {
            info!("Spend has completed: {:?}", spend_attempt_result);
            self.wallet.clear_confirmed_spend_requests();
        }

        Ok(total_cost)
    }

    /// Resend failed txs
    /// This can optionally verify the store has been successful (this will attempt to GET the cash_note from the network)
    pub async fn resend_pending_txs(&mut self, verify_store: bool) {
        if self
            .client
            .send_spends(
                self.wallet.unconfirmed_spend_requests().iter(),
                verify_store,
            )
            .await
            .is_ok()
        {
            self.wallet.clear_confirmed_spend_requests();
            // We might want to be _really_ sure and do the below
            // as well, but it's not necessary.
            // use crate::domain::wallet::VerifyingClient;
            // client.verify(tx_hash).await.ok();
        }
    }

    /// Return the wallet.
    pub fn into_wallet(self) -> LocalWallet {
        self.wallet
    }

    /// Return ref to inner waller
    pub fn mut_wallet(&mut self) -> &mut LocalWallet {
        &mut self.wallet
    }
}

impl Client {
    /// Send spend requests to the network.
    /// This can optionally verify the spends have been correctly stored before returning
    pub async fn send_spends(
        &self,
        spend_requests: impl Iterator<Item = &SignedSpend>,
        verify_store: bool,
    ) -> WalletResult<()> {
        let mut tasks = Vec::new();

        for spend_request in spend_requests {
            debug!(
                "sending spend request to the network: {:?}: {spend_request:#?}",
                spend_request.unique_pubkey()
            );

            let the_task = async move {
                let cash_note_key = spend_request.unique_pubkey();
                let result = self
                    .network_store_spend(spend_request.clone(), verify_store)
                    .await;

                (cash_note_key, result)
            };
            tasks.push(the_task);
        }

        let mut spent_cash_notes = BTreeSet::default();
        for (cash_note_key, spend_attempt_result) in join_all(tasks).await {
            // This is a record mismatch on spend, we need to clean up and remove the spent CashNote from the wallet
            // This only happens if we're verifying the store
            if let Err(Error::Network(sn_networking::Error::GetRecordError(
                GetRecordError::RecordDoesNotMatch(record_key),
            ))) = spend_attempt_result
            {
                warn!("Record mismatch on spend, removing CashNote from wallet: {record_key:?}");
                spent_cash_notes.insert(*cash_note_key);
            } else {
                return spend_attempt_result
                    .map_err(|err| WalletError::CouldNotSendMoney(err.to_string()));
            }
        }

        if spent_cash_notes.is_empty() {
            Ok(())
        } else {
            Err(WalletError::DoubleSpendAttemptedForCashNotes(
                spent_cash_notes,
            ))
        }
    }

    /// Receive a Transfer, verify and redeem CashNotes from the Network.
    pub async fn receive(
        &self,
        transfer: &Transfer,
        wallet: &LocalWallet,
    ) -> WalletResult<Vec<CashNote>> {
        let cashnotes = self
            .network
            .verify_and_unpack_transfer(transfer, wallet)
            .map_err(|e| WalletError::CouldNotReceiveMoney(format!("{e:?}")))
            .await?;
        Ok(cashnotes)
    }

    /// Verify that the spends refered to in the CashNote exist on the network.
    pub async fn verify_cashnote(&self, cash_note: &CashNote) -> WalletResult<()> {
        // We need to get all the spends in the cash_note from the network,
        // and compare them to the spends in the cash_note, to know if the
        // transfer is considered valid in the network.
        let mut tasks = Vec::new();
        for spend in &cash_note.signed_spends {
            let address = SpendAddress::from_unique_pubkey(spend.unique_pubkey());
            debug!(
                "Getting spend for pubkey {:?} from network at {address:?}",
                spend.unique_pubkey()
            );
            tasks.push(self.get_spend_from_network(address));
        }

        let mut received_spends = std::collections::BTreeSet::new();
        for result in join_all(tasks).await {
            let network_valid_spend =
                result.map_err(|err| WalletError::CouldNotVerifyTransfer(err.to_string()))?;
            let _ = received_spends.insert(network_valid_spend);
        }

        // If all the spends in the cash_note are the same as the ones in the network,
        // we have successfully verified that the cash_note is globally recognised and therefor valid.
        if received_spends == cash_note.signed_spends {
            return Ok(());
        }
        Err(WalletError::CouldNotVerifyTransfer(
            "The spends in network were not the same as the ones in the CashNote. The parents of this CashNote are probably double spends.".into(),
        ))
    }
}

/// Use the client to send a CashNote from a local wallet to an address.
/// This marks the spent CashNote as spent in the Network
pub async fn send(
    from: LocalWallet,
    amount: NanoTokens,
    to: MainPubkey,
    client: &Client,
    verify_store: bool,
) -> Result<CashNote> {
    if amount.is_zero() {
        return Err(Error::AmountIsZero);
    }

    let mut wallet_client = WalletClient::new(client.clone(), from);

    let mut did_error = false;
    // Wallet shall be all clear to progress forward.
    let mut attempts = 0;
    while wallet_client.unconfirmed_spend_requests_exist() {
        info!("Pre-Unconfirmed txs exist, sending again after 1 second...");
        sleep(Duration::from_secs(1)).await;
        wallet_client.resend_pending_txs(verify_store).await;

        if attempts > 10 {
            // save the error state, but break out of the loop so we can save
            did_error = true;
            break;
        }

        attempts += 1;
    }

    if did_error {
        error!("Wallet has pre-unconfirmed txs, cann't progress further.");
        println!("Wallet has pre-unconfirmed txs, cann't progress further.");
        return Err(WalletError::UnconfirmedTxAfterRetries.into());
    }

    let new_cash_note = wallet_client
        .send_cash_note(amount, to, verify_store)
        .await
        .map_err(|err| {
            error!("Could not send cash note, err: {err:?}");
            err
        })?;

    if verify_store {
        attempts = 0;
        while wallet_client.unconfirmed_spend_requests_exist() {
            info!("Unconfirmed txs exist, sending again after 1 second...");
            sleep(Duration::from_secs(1)).await;
            wallet_client.resend_pending_txs(verify_store).await;

            if attempts > 10 {
                // save the error state, but break out of the loop so we can save
                did_error = true;
                break;
            }

            attempts += 1;
        }
    }

    if did_error {
        wallet_client
            .into_wallet()
            .store_unconfirmed_spend_requests()?;
        return Err(WalletError::UnconfirmedTxAfterRetries.into());
    }

    wallet_client
        .into_wallet()
        .deposit_and_store_to_disk(&vec![new_cash_note.clone()])?;

    Ok(new_cash_note)
}

/// Send tokens to another wallet.
/// Can optionally verify the store has been successful (this will attempt to GET the Spend from the network)
pub async fn broadcast_signed_spends(
    from: LocalWallet,
    client: &Client,
    signed_spends: BTreeSet<SignedSpend>,
    tx: Transaction,
    change_id: UniquePubkey,
    output_details: BTreeMap<UniquePubkey, (MainPubkey, DerivationIndex)>,
    verify_store: bool,
) -> WalletResult<CashNote> {
    let mut wallet_client = WalletClient::new(client.clone(), from);

    let mut did_error = false;
    // Wallet shall be all clear to progress forward.
    let mut attempts = 0;
    while wallet_client.unconfirmed_spend_requests_exist() {
        info!("Pre-Unconfirmed txs exist, sending again after 1 second...");
        sleep(Duration::from_secs(1)).await;
        wallet_client.resend_pending_txs(verify_store).await;

        if attempts > 10 {
            // save the error state, but break out of the loop so we can save
            did_error = true;
            break;
        }

        attempts += 1;
    }

    if did_error {
        error!("Wallet has pre-unconfirmed txs, cann't progress further.");
        println!("Wallet has pre-unconfirmed txs, cann't progress further.");
        return Err(WalletError::UnconfirmedTxAfterRetries);
    }

    let new_cash_note = wallet_client
        .send_signed_spends(signed_spends, tx, change_id, output_details, verify_store)
        .await
        .map_err(|err| {
            error!("Could not send signed spends, err: {err:?}");
            err
        })?;

    if verify_store {
        attempts = 0;
        while wallet_client.unconfirmed_spend_requests_exist() {
            info!("Unconfirmed txs exist, sending again after 1 second...");
            sleep(Duration::from_secs(1)).await;
            wallet_client.resend_pending_txs(verify_store).await;

            if attempts > 10 {
                // save the error state, but break out of the loop so we can save
                did_error = true;
                break;
            }

            attempts += 1;
        }
    }

    if did_error {
        wallet_client
            .into_wallet()
            .store_unconfirmed_spend_requests()?;
        return Err(WalletError::UnconfirmedTxAfterRetries);
    }

    wallet_client
        .into_wallet()
        .deposit_and_store_to_disk(&vec![new_cash_note.clone()])?;

    Ok(new_cash_note)
}
