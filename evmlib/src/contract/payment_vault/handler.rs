use crate::common::{Address, Amount, Calldata, TxHash};
use crate::contract::payment_vault::error::Error;
use crate::contract::payment_vault::interface::IPaymentVault;
use crate::contract::payment_vault::interface::IPaymentVault::IPaymentVaultInstance;
use crate::TX_TIMEOUT;
use alloy::network::{Network, TransactionBuilder};
use alloy::providers::Provider;
use alloy::transports::Transport;

pub struct PaymentVaultHandler<T: Transport + Clone, P: Provider<T, N>, N: Network> {
    pub contract: IPaymentVaultInstance<T, P, N>,
}

impl<T, P, N> PaymentVaultHandler<T, P, N>
where
    T: Transport + Clone,
    P: Provider<T, N>,
    N: Network,
{
    /// Create a new PaymentVaultHandler instance from a (proxy) contract's address
    pub fn new(contract_address: Address, provider: P) -> Self {
        let contract = IPaymentVault::new(contract_address, provider);
        Self { contract }
    }

    /// Set the provider
    pub fn set_provider(&mut self, provider: P) {
        let address = *self.contract.address();
        self.contract = IPaymentVault::new(address, provider);
    }

    /// Fetch a quote from the contract
    pub async fn get_quote<I: IntoIterator<Item: Into<IPaymentVault::QuotingMetrics>>>(
        &self,
        metrics: I,
    ) -> Result<Vec<Amount>, Error> {
        let metrics: Vec<_> = metrics.into_iter().map(|v| v.into()).collect();
        let mut amounts = self.contract.getQuote(metrics.clone()).call().await?.prices;

        // FIXME: temporary logic until the smart contract gets updated
        if amounts.len() == 1 {
            let value = amounts[0];
            amounts.resize(metrics.len(), value);
        }

        Ok(amounts)
    }

    /// Pay for quotes.
    pub async fn pay_for_quotes<I: IntoIterator<Item: Into<IPaymentVault::DataPayment>>>(
        &self,
        data_payments: I,
    ) -> Result<TxHash, Error> {
        let (calldata, to) = self.pay_for_quotes_calldata(data_payments)?;

        let transaction_request = self
            .contract
            .provider()
            .transaction_request()
            .with_to(to)
            .with_input(calldata);

        let pending_tx_builder = self
            .contract
            .provider()
            .send_transaction(transaction_request)
            .await
            .inspect_err(|err| error!("Error to send_transaction during pay_for_quotes: {err:?}"))?
            .with_timeout(Some(TX_TIMEOUT));

        let pending_tx_hash = pending_tx_builder.tx_hash();
        debug!("pay_for_quotes is pending with tx hash: {pending_tx_hash}");

        let tx_hash = pending_tx_builder.watch().await.inspect_err(|err| {
            error!("Error to watch transaction during pay_for_quotes: {err:?}")
        })?;

        Ok(tx_hash)
    }

    /// Returns the pay for quotes transaction calldata.
    pub fn pay_for_quotes_calldata<I: IntoIterator<Item: Into<IPaymentVault::DataPayment>>>(
        &self,
        data_payments: I,
    ) -> Result<(Calldata, Address), Error> {
        let data_payments: Vec<IPaymentVault::DataPayment> =
            data_payments.into_iter().map(|item| item.into()).collect();

        let calldata = self
            .contract
            .payForQuotes(data_payments)
            .calldata()
            .to_owned();

        Ok((calldata, *self.contract.address()))
    }

    /// Verify if payments are valid
    pub async fn verify_payment<I: IntoIterator<Item: Into<IPaymentVault::PaymentVerification>>>(
        &self,
        payment_verifications: I,
    ) -> Result<[IPaymentVault::PaymentVerificationResult; 3], Error> {
        let payment_verifications: Vec<IPaymentVault::PaymentVerification> = payment_verifications
            .into_iter()
            .map(|v| v.into())
            .collect();

        let results = self
            .contract
            .verifyPayment(payment_verifications)
            .call()
            .await?
            .verificationResults;

        Ok(results)
    }
}
