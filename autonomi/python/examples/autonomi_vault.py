from autonomi_client import Client, Wallet, PaymentOption, VaultSecretKey, UserData

def main():
    # Initialize
    private_key = "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
    peers = ["/ip4/127.0.0.1/tcp/12000"]
    
    try:
        # Setup
        wallet = Wallet(private_key)
        client = Client.connect(peers)
        payment = PaymentOption.wallet(wallet)
        
        # Create vault key
        vault_key = VaultSecretKey.new()
        print(f"Created vault key: {vault_key.to_hex()}")
        
        # Get vault cost
        cost = client.vault_cost(vault_key)
        print(f"Vault cost: {cost}")
        
        # Create user data
        user_data = UserData()
        
        # Store some data in vault
        data = b"Hello from vault!"
        content_type = 1  # Custom content type
        cost = client.write_bytes_to_vault(data, payment, vault_key, content_type)
        print(f"Wrote data to vault, cost: {cost}")
        
        # Read data back
        retrieved_data, retrieved_type = client.fetch_and_decrypt_vault(vault_key)
        print(f"Retrieved data: {retrieved_data.decode()}")
        print(f"Content type: {retrieved_type}")
        
        # Store user data
        cost = client.vault_put_user_data(vault_key, payment, user_data)
        print(f"Stored user data, cost: {cost}")
        
        # Get user data
        retrieved_user_data = client.vault_get_user_data(vault_key)
        print("File archives:", retrieved_user_data.file_archives())
        print("Private file archives:", retrieved_user_data.private_file_archives())
        
    except Exception as e:
        print(f"Error: {e}")
        return 1
    
    print("All vault operations completed successfully!")
    return 0

if __name__ == "__main__":
    exit(main()) 