// Copyright 2023 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use sn_transfers::client_transfers::TransferOutputsMap;

use super::Client;

use sn_dbc::{Dbc, PublicAddress, Token};
use sn_protocol::NetworkAddress;
use sn_transfers::{
    client_transfers::TransferOutputs,
    wallet::{Error, LocalWallet, Result},
};

use futures::future::join_all;
use std::{
    collections::BTreeMap,
    iter::Iterator,
    time::{Duration, Instant},
};
use tokio::time::sleep;

/// A wallet client can be used to send and
/// receive tokens to/from other wallets.
pub struct WalletClient {
    client: Client,
    wallet: LocalWallet,
    /// These have not yet been successfully confirmed in
    /// the network and need to be republished, to reach network validity.
    /// We maintain the order they were added in, as to republish
    /// them in the correct order, in case any later spend was
    /// dependent on an earlier spend.
    unconfirmed_txs: Vec<TransferOutputs>,
}

impl WalletClient {
    /// Create a new wallet client.
    pub fn new(client: Client, wallet: LocalWallet) -> Self {
        Self {
            client,
            wallet,
            unconfirmed_txs: vec![],
        }
    }

    /// Do we have any unconfirmed transactions?
    pub fn unconfirmed_txs_exist(&self) -> bool {
        !self.unconfirmed_txs.is_empty()
    }

    /// Send tokens to another wallet.
    /// Can optionally verify the store has been successful (this will attempt to GET the dbc from the network)
    pub async fn send(
        &mut self,
        amount: Token,
        to: PublicAddress,
        verify_store: bool,
    ) -> Result<Dbc> {
        // retry previous failures
        self.resend_pending_txs(verify_store).await;

        // offline transfer
        let transfer = self.wallet.local_send(vec![(amount, to)], None).await?;
        let dbcs = transfer.created_dbcs.clone();

        // send to network
        trace!("Sending transfer to the network: {transfer:#?}");
        if let Err(error) = self.client.send(transfer.clone(), verify_store).await {
            warn!("The transfer was not successfully registered in the network: {error:?}. It will be retried later.");
            self.unconfirmed_txs.push(transfer);
        }

        // return created DBCs even if network part failed???
        match &dbcs[..] {
            [info, ..] => Ok(info.clone()),
            [] => Err(Error::CouldNotSendTokens(
                "No DBCs were returned from the wallet.".into(),
            )),
        }
    }

    /// Get storecost from the network
    /// Returns a Vec of proofs
    pub async fn get_store_cost_at_address(
        &mut self,
        address: &NetworkAddress,
    ) -> Result<Vec<(PublicAddress, Token)>> {
        self.client
            .get_store_costs_at_address(address)
            .await
            .map_err(|error| Error::CouldNotSendTokens(error.to_string()))
    }

    /// Send tokens to nodes closest to the data we want to make storage payment for.
    ///
    /// Returns (Proofs and an Option around Storage Cost), storage cost is _per record_, and only returned if required for this operation
    ///
    /// This can optionally verify the store has been successful (this will attempt to GET the dbc from the network)
    pub async fn pay_for_storage(
        &mut self,
        content_addrs: impl Iterator<Item = NetworkAddress>,
        verify_store: bool,
    ) -> Result<(TransferOutputsMap, Token)> {
        let mut total_cost = Token::zero();

        let mut payment_map = BTreeMap::default();

        // we can collate all the payments together into one transfer
        for content_addr in content_addrs {
            // TODO: parallelise this
            let amounts_to_pay = self.get_store_cost_at_address(&content_addr).await?;

            // TODO: We put an empty vec for now as we're not validating.
            // We'll need to map dbcs to content addrs though.
            payment_map.insert(content_addr, amounts_to_pay);
        }

        // TODO: This needs to map returned Dbcs to target addrs
        let (payments, cost) = self.pay_for_records(payment_map, verify_store).await?;

        if let Some(cost) = total_cost.checked_add(cost) {
            total_cost = cost;
        }

        Ok((payments, total_cost))
    }

    /// Send tokens to nodes closest to the data we want to make storage payment for.
    ///
    /// Returns DbcIds created for the payment and the total amount paid
    ///
    /// This can optionally verify the store has been successful (this will attempt to GET the dbc from the network)
    pub async fn pay_for_records(
        &mut self,
        all_data_payments: BTreeMap<NetworkAddress, Vec<(PublicAddress, Token)>>,
        verify_store: bool,
    ) -> Result<(BTreeMap<NetworkAddress, TransferOutputs>, Token)> {
        let now = Instant::now();

        let mut total_cost = Token::zero();
        for (_data, costs) in all_data_payments.iter() {
            for (_target, cost) in costs {
                if let Some(new_cost) = total_cost.checked_add(*cost) {
                    total_cost = new_cost;
                } else {
                    return Err(Error::TotalPriceTooHigh);
                }
            }
        }
        let transfer_map = self
            .wallet
            .local_send_storage_payment(all_data_payments, None)
            .await?;

        // send to network
        trace!("Sending storage payment transfer to the network");

        for transfer in transfer_map.values() {
            let mut tasks = Vec::new();

            tasks.push(self.client.send(transfer.clone(), verify_store));

            for spend_attempt_result in join_all(tasks).await {
                debug!("Spend has completed: {:?}", spend_attempt_result);
                if let Err(error) = spend_attempt_result {
                    warn!("The storage payment transfer was not successfully registered in the network: {error:?}. It will be retried later.");
                    self.unconfirmed_txs.push(transfer.clone());
                    return Err(error);
                }
            }
        }

        let elapsed = now.elapsed();
        println!("After {elapsed:?}, All transfers made for total payment of {total_cost:?} nano tokens. ");

        Ok((transfer_map, total_cost))
    }

    /// Resend failed txs
    /// This can optionally verify the store has been successful (this will attempt to GET the dbc from the network)
    pub async fn resend_pending_txs(&mut self, verify_store: bool) {
        for (index, transfer) in self.unconfirmed_txs.clone().into_iter().enumerate() {
            let tx_hash = transfer.tx.hash();
            println!("Trying to republish pending tx: {tx_hash:?}..");
            if self
                .client
                .send(transfer.clone(), verify_store)
                .await
                .is_ok()
            {
                println!("Tx {tx_hash:?} was successfully republished!");
                let _ = self.unconfirmed_txs.remove(index);
                // We might want to be _really_ sure and do the below
                // as well, but it's not necessary.
                // use crate::domain::wallet::VerifyingClient;
                // client.verify(tx_hash).await.ok();
            }
        }
    }

    /// Return the wallet.
    pub fn into_wallet(self) -> LocalWallet {
        self.wallet
    }
}

impl Client {
    /// Send a spend request to the network.
    /// This can optionally verify the spend has been correctly stored before returning
    pub async fn send(&self, transfer: TransferOutputs, verify_store: bool) -> Result<()> {
        let mut tasks = Vec::new();
        for spend_request in &transfer.all_spend_requests {
            trace!(
                "sending spend request to the network: {:?}: {spend_request:#?}",
                spend_request.signed_spend.dbc_id()
            );
            tasks.push(self.network_store_spend(spend_request.clone(), verify_store));
        }

        for spend_attempt_result in join_all(tasks).await {
            spend_attempt_result.map_err(|err| Error::CouldNotSendTokens(err.to_string()))?;
        }

        Ok(())
    }

    /// Send a spend request to the network.
    /// This does _not_ verify the spend has been put to the network correctly
    pub async fn send_without_verify(&self, transfer: TransferOutputs) -> Result<()> {
        let mut tasks = Vec::new();
        for spend_request in &transfer.all_spend_requests {
            trace!("sending spend request to the network: {spend_request:#?}");
            tasks.push(self.network_store_spend(spend_request.clone(), false));
        }

        for spend_attempt_result in join_all(tasks).await {
            spend_attempt_result.map_err(|err| Error::CouldNotSendTokens(err.to_string()))?;
        }

        Ok(())
    }

    pub async fn verify(&self, dbc: &Dbc) -> Result<()> {
        // We need to get all the spends in the dbc from the network,
        // and compare them to the spends in the dbc, to know if the
        // transfer is considered valid in the network.
        let mut tasks = Vec::new();
        for spend in &dbc.signed_spends {
            tasks.push(self.get_spend_from_network(spend.dbc_id()));
        }

        let mut received_spends = std::collections::BTreeSet::new();
        for result in join_all(tasks).await {
            let network_valid_spend =
                result.map_err(|err| Error::CouldNotVerifyTransfer(err.to_string()))?;
            let _ = received_spends.insert(network_valid_spend);
        }

        // If all the spends in the dbc are the same as the ones in the network,
        // we have successfully verified that the dbc is globally recognised and therefor valid.
        if received_spends == dbc.signed_spends {
            return Ok(());
        }
        Err(Error::CouldNotVerifyTransfer(
            "The spends in network were not the same as the ones in the DBC. The parents of this DBC are probably double spends.".into(),
        ))
    }
}

/// Use the client to send a DBC from a local wallet to an address.
/// This marks the spent DBC as spent in the Network
pub async fn send(
    from: LocalWallet,
    amount: Token,
    to: PublicAddress,
    client: &Client,
    verify_store: bool,
) -> Result<Dbc> {
    if amount.as_nano() == 0 {
        panic!("Amount must be more than zero.");
    }

    let mut wallet_client = WalletClient::new(client.clone(), from);
    let new_dbc = wallet_client
        .send(amount, to, verify_store)
        .await
        .expect("Tokens shall be successfully sent.");

    if verify_store {
        let mut attempts = 0;
        while wallet_client.unconfirmed_txs_exist() {
            info!("Unconfirmed txs exist, sending again after 1 second...");
            sleep(Duration::from_secs(1)).await;
            wallet_client.resend_pending_txs(verify_store).await;

            if attempts > 10 {
                return Err(Error::UnconfirmedTxAfterRetries);
            }

            attempts += 1;
        }
    }

    let mut wallet = wallet_client.into_wallet();
    wallet
        .store()
        .await
        .expect("Wallet shall be successfully stored.");
    wallet
        .store_created_dbc(new_dbc.clone())
        .await
        .expect("Created dbc shall be successfully stored.");

    Ok(new_dbc)
}
