use alloy::providers::Provider;
use evmlib::utils::http_provider;
use evmlib::Network;

#[tokio::test]
async fn test_gas_fee_limit() {
    let network = Network::ArbitrumOne;
    let provider = http_provider(network.rpc_url().clone());
    let base_gas_price = provider.get_gas_price().await.unwrap();
    let max_priority_fee_per_gas = provider.get_max_priority_fee_per_gas().await.unwrap();

    println!("Base gas price: {base_gas_price}");
    println!("Max priority fee per gas: {max_priority_fee_per_gas}");
}
