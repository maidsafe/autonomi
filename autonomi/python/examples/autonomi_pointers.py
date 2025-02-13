"""
Example demonstrating the use of pointers in the Autonomi network.
Pointers allow for creating references to data that can be updated.
"""

from autonomi_client import Client, Network, Wallet, PaymentOption, SecretKey, PointerTarget, ChunkAddress, Pointer
import asyncio

async def main():
    # Initialize a wallet with a private key
    # This should be a valid Ethereum private key (64 hex chars)
    private_key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
    network = Network(True)
    wallet = Wallet.new_from_private_key(network, private_key)
    print(f"Wallet address: {wallet.address()}")
    print(f"Wallet balance: {await wallet.balance()}")

    # Connect to the network
    client = await Client.init_local()

    # First, let's upload some data that we want to point to
    target_data = b"Hello, I'm the target data!"
    [cost, target_addr] = await client.data_put_public(target_data, PaymentOption.wallet(wallet))
    print(f"Target data uploaded to: {target_addr}")

    # Create a pointer target from the address
    target = PointerTarget.new_chunk(ChunkAddress(target_addr))
    
    # Create owner key pair
    key = SecretKey()

    # Estimate the cost of the pointer
    cost = await client.pointer_cost(key.public_key())
    print(f"pointer cost: {cost}")

    # Create the pointer
    pointer = Pointer(key, 0, target)
    payment_option = PaymentOption.wallet(wallet)
    
    # Create and store the pointer
    pointer_addr = await client.pointer_put(pointer, payment_option)
    print("Pointer stored successfully")

    # Wait for the pointer to be stored by the network
    await asyncio.sleep(1)

    # Later, we can retrieve the pointer
    pointer = await client.pointer_get(pointer_addr)
    print(f"Retrieved pointer target: {pointer}")

    # We can then use the target address to get the original data
    retrieved_data = await client.data_get_public(pointer.target.hex)
    print(f"Retrieved target data: {retrieved_data.decode()}")


asyncio.run(main())
