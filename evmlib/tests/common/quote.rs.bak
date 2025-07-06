use evmlib::common::{Amount, QuotePayment};
use evmlib::utils::{dummy_address, dummy_hash};

#[allow(dead_code)]
pub fn random_quote_payment() -> QuotePayment {
    let quote_hash = dummy_hash();
    let reward_address = dummy_address();
    let amount = Amount::from(1);
    (quote_hash, reward_address, amount)
}
