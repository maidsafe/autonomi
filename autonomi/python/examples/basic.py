from autonomi_client import Client, Wallet, VaultSecretKey, UserData

def external_signer_example(client: Client, data: bytes):
    # Get quotes for storing data
    quotes, payments, free_chunks = client.get_quotes_for_data(data)
    print(f"Got {len(quotes)} quotes for storing data")
    print(f"Need to make {len(payments)} payments")
    print(f"{len(free_chunks)} chunks are free")
    
    # Get raw quotes for specific addresses
    addr = "0123456789abcdef"  # Example address
    quotes, payments, free = client.get_quotes_for_content_addresses([addr])
    print(f"Got quotes for address {addr}")

def main():
    # Connect to network
    client = Client(["/ip4/127.0.0.1/tcp/12000"])
    
    # Create wallet
    wallet = Wallet()
    print(f"Wallet address: {wallet.address()}")
    
    # Upload public data
    data = b"Hello World!"
    addr = client.data_put_public(data, wallet)
    print(f"Uploaded public data to: {addr}")
    retrieved = client.data_get_public(addr)
    print(f"Retrieved public data: {retrieved}")
    
    # Upload private data
    private_access = client.data_put(b"Secret message", wallet)
    print(f"Private data access: {private_access}")
    private_data = client.data_get(private_access)
    print(f"Retrieved private data: {private_data}")
    
    # Upload file/directory
    file_addr = client.file_upload_public("./test_data", wallet)
    print(f"Uploaded files to: {file_addr}")
    client.file_download(file_addr, "./downloaded_data")
    print("Downloaded files")
    
    # Vault operations
    vault_key = VaultSecretKey.generate()
    vault_cost = client.vault_cost(vault_key)
    print(f"Vault creation cost: {vault_cost}")

    user_data = UserData()
    cost = client.vault_put_user_data(vault_key, wallet, user_data)
    print(f"Stored user data, cost: {cost}")

    retrieved_data = client.vault_get_user_data(vault_key)
    print(f"Retrieved user data: {retrieved_data}")

    # Private directory operations
    private_dir_access = client.dir_upload("./test_data", wallet)
    print(f"Uploaded private directory, access: {private_dir_access}")
    client.dir_download(private_dir_access, "./downloaded_private")
    print("Downloaded private directory")

    # External signer example
    external_signer_example(client, b"Test data")

if __name__ == "__main__":
    main() 