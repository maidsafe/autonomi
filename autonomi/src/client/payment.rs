use crate::client::data::PayError;
use crate::Client;
use ant_evm::{EvmWallet, ProofOfPayment};
use std::collections::HashMap;
use xor_name::XorName;

/// Contains the proof of payment for XOR addresses.
pub type Receipt = HashMap<XorName, ProofOfPayment>;

/// Payment options for data payments.
#[derive(Clone)]
pub enum PaymentOption {
    Wallet(EvmWallet),
    Receipt(Receipt),
}

impl From<EvmWallet> for PaymentOption {
    fn from(value: EvmWallet) -> Self {
        PaymentOption::Wallet(value)
    }
}

impl From<&EvmWallet> for PaymentOption {
    fn from(value: &EvmWallet) -> Self {
        PaymentOption::Wallet(value.clone())
    }
}

impl From<Receipt> for PaymentOption {
    fn from(value: Receipt) -> Self {
        PaymentOption::Receipt(value)
    }
}

impl Client {
    pub(crate) async fn pay_for_content_addrs(
        &self,
        content_addrs: impl Iterator<Item = XorName>,
        payment_option: PaymentOption,
    ) -> Result<Receipt, PayError> {
        match payment_option {
            PaymentOption::Wallet(wallet) => {
                let (receipt, _) = self.pay(content_addrs, &wallet).await?;
                debug!(
                    "Paid for content addresses with wallet and the receipt is {:?}",
                    receipt
                );
                Ok(receipt)
            }
            PaymentOption::Receipt(receipt) => Ok(receipt),
        }
    }
}
