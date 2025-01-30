from autonomi_client import Client, Wallet, PaymentOption
import asyncio

async def main():
    # Initialize a wallet with a private key
    # This should be a valid Ethereum private key (64 hex chars without '0x' prefix)
    private_key = "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
    wallet = Wallet(private_key)
    print(f"Wallet address: {wallet.address()}")
    print(f"Wallet balance: {wallet.balance()}")

    # Connect to the network
    client = await Client.init()

    # Create payment option using the wallet
    payment = PaymentOption.wallet(wallet)

    # Upload some data
    data = b"Hello, Safe Network!"
    addr = await client.data_put_public(data, payment)
    print(f"Data uploaded to address: {addr}")

    # Download the data back
    downloaded = await client.data_get_public(addr)
    print(f"Downloaded data: {downloaded.decode()}")

    # You can also upload files
    with open("example.txt", "rb") as f:
        file_data = f.read()
        file_addr = client.data_put_public(file_data, payment)
        print(f"File uploaded to address: {file_addr}")


asyncio.run(main())
