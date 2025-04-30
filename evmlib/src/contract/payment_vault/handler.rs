use crate::common::{Address, Amount, Calldata, TxHash};
use crate::contract::payment_vault::error::Error;
use crate::contract::payment_vault::interface::IPaymentVault;
use crate::contract::payment_vault::interface::IPaymentVault::IPaymentVaultInstance;
use crate::retry::{retry, send_transaction_with_retries};
use crate::transaction_config::TransactionConfig;
use alloy::network::Network;
use alloy::providers::Provider;

pub struct PaymentVaultHandler<P: Provider<N>, N: Network> {
    pub contract: IPaymentVaultInstance<P, N>,
}

impl<P, N> PaymentVaultHandler<P, N>
where
    P: Provider<N>,
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

        debug!("Getting quotes for metrics: {metrics:?}");

        let mut amounts = retry(
            || async { self.contract.getQuote(metrics.clone()).call().await },
            "getQuote",
            None,
        )
        .await?;

        // FIXME: temporary logic until the local smart contract gets updated
        if amounts.len() == 1 {
            let value = amounts[0];
            amounts.resize(metrics.len(), value);
        }

        debug!("Returned quotes are: {:?}", amounts);

        Ok(amounts)
    }

    /// Pay for quotes.
    pub async fn pay_for_quotes<I: IntoIterator<Item: Into<IPaymentVault::DataPayment>>>(
        &self,
        data_payments: I,
        transaction_config: &TransactionConfig,
    ) -> Result<TxHash, Error> {
        debug!("Paying for quotes.");
        let (calldata, to) = self.pay_for_quotes_calldata(data_payments)?;
        send_transaction_with_retries(
            self.contract.provider(),
            calldata,
            to,
            "pay for quotes",
            transaction_config,
        )
        .await
        .map_err(Error::from)
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

        debug!("Verifying payments: {payment_verifications:?}");

        let results = retry(
            || async {
                self.contract
                    .verifyPayment(payment_verifications.clone())
                    .call()
                    .await
            },
            "verifyPayment",
            None,
        )
        .await?;

        debug!("Payment verification results: {:?}", results);

        Ok(results)
    }
}
