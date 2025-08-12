import asyncio
from autonomi_client import Client, SecretKey, Wallet, PaymentOption, EVMNetwork

async def scratchpad_example():
    # Initialize client and wallet
    client = await Client.init_local()
    network = EVMNetwork(True)  # Use testnet for local development
    # For mainnet use: EVMNetwork(False)
    private_key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
    wallet = Wallet.new_from_private_key(network, private_key)
    payment = PaymentOption.wallet(wallet)

    # Create secret key for scratchpad
    key = SecretKey()
    public_key = key.public_key()

    try:
        # Check cost
        estimated_cost = await client.scratchpad_cost(public_key)
        print(f"Estimated scratchpad cost: {estimated_cost}")

        # Create scratchpad
        content_type = 42
        initial_data = b"Hello, Autonomi!"
        actual_cost, addr = await client.scratchpad_create(key, content_type, initial_data, payment)
        print(f"Created at {addr.hex}")
        print(f"Actual cost: {actual_cost}")

        # Get scratchpad
        scratchpad = await client.scratchpad_get(addr)
        assert scratchpad.counter() == 0
        print(f"Retrieved scratchpad with counter: {scratchpad.counter()}")
        
        # Decrypt content
        decrypted = scratchpad.decrypt_data(key)
        assert decrypted == initial_data
        print("✓ Decrypted content matches initial data")

        # Update scratchpad (free)
        new_data = b"Updated content!"
        await client.scratchpad_update(key, content_type, new_data)
        print("✓ Scratchpad updated successfully")

        # Get updated scratchpad
        updated = await client.scratchpad_get(addr)
        assert updated.counter() == 1
        updated_content = updated.decrypt_data(key)
        assert updated_content == new_data
        print(f"✓ Updated scratchpad verified with counter: {updated.counter()}")
        print("✓ All scratchpad operations completed successfully!")

    except Exception as e:
        print(f"Error: {e}")

asyncio.run(scratchpad_example())
