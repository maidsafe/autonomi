use evmlib::common::Amount;
use evmlib::common::QuotePayment;
use evmlib::utils::dummy_address;
use evmlib::utils::dummy_hash;

#[allow(dead_code)]
pub fn random_quote_payment() -> QuotePayment {
    let quote_hash = dummy_hash();
    let reward_address = dummy_address();
    let amount = Amount::from(1);
    (quote_hash, reward_address, amount)
}
