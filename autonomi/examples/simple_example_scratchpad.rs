use autonomi::client::payment::PaymentOption;
use autonomi::{Bytes, Client, SecretKey};
use evmlib::{Network, wallet::Wallet};

// Helper function to create a funded wallet
fn get_funded_wallet() -> Result<Wallet, Box<dyn std::error::Error>> {
    let private_key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
    // Use the same network type as the client (EvmNetwork::new(true) in init_local)
    let network = Network::new(true)?;
    let wallet = Wallet::new_from_private_key(network, private_key)?;
    Ok(wallet)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    scratchpad_example().await
}

async fn scratchpad_example() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize client and wallet
    let client = Client::init_local().await?;
    let wallet = get_funded_wallet()?;
    let payment = PaymentOption::from(&wallet);

    // Create secret key for scratchpad
    let key = SecretKey::random();
    let public_key = key.public_key();

    // Check cost
    let estimated_cost = client.scratchpad_cost(&public_key).await?;
    println!("Estimated scratchpad cost: {estimated_cost}");

    // Create scratchpad
    let content_type = 42;
    let initial_data = Bytes::from("Hello, Autonomi!");
    let (actual_cost, addr) = client
        .scratchpad_create(&key, content_type, &initial_data, payment.clone())
        .await?;
    println!("Created at {addr:?}");
    println!("Actual cost: {actual_cost}");

    // Get scratchpad
    let scratchpad = client.scratchpad_get(&addr).await?;
    assert_eq!(scratchpad.counter(), 0);
    println!(
        "Retrieved scratchpad with counter: {}",
        scratchpad.counter()
    );

    // Decrypt content
    let decrypted = scratchpad.decrypt_data(&key)?;
    assert_eq!(decrypted, initial_data);
    println!("✓ Decrypted content matches initial data");

    // Update scratchpad
    let new_data = Bytes::from("Updated content!");
    client
        .scratchpad_update(&key, content_type, &new_data)
        .await?;
    println!("✓ Scratchpad updated successfully");

    // Get updated scratchpad
    let updated = client.scratchpad_get(&addr).await?;
    assert_eq!(updated.counter(), 1);
    let updated_content = updated.decrypt_data(&key)?;
    assert_eq!(updated_content, new_data);
    println!(
        "✓ Updated scratchpad verified with counter: {}",
        updated.counter()
    );
    println!("✓ All scratchpad operations completed successfully!");

    Ok(())
}
