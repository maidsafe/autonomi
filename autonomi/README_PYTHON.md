# Autonomi Python Bindings

The Autonomi client library provides Python bindings for easy integration with Python applications.

## Installation

We recommend using `uv` for Python environment management:

Make sure you have installed:
- `Python`
- `uv`

## Quick Start

```bash
# make sure you are in the autonomi directory
cd autonomi/

# make a virtual environment
uv venv
source .venv/bin/activate
uv sync
maturin develop --uv

# Then you can test with pytest
pytest tests/python/test_bindings.py

# or you can run the examples or your own scripts!
python python/examples/autonomi_pointers.py 
```

```python
from autonomi_client import *Client, Wallet, PaymentOption*

# Initialize a wallet with a private key
wallet = Wallet.new_from_private_key(Network(True), "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
print(f"Wallet address: {wallet.address()}")
print(f"Wallet balance: {await wallet.balance()}")

# Connect to the network
client = await Client.init()

# Create payment option using the wallet
payment = PaymentOption.wallet(wallet)

# Upload some data
data = b"Hello, Safe Network!"
[cost, addr] = await client.data_put_public(data, payment)
print(f"Data uploaded to address: {addr}")

# Download the data back
downloaded = await client.data_get_public(addr)
print(f"Downloaded data: {downloaded.decode()}")
```

## API Reference

### Client

The main interface to interact with the Autonomi network.

#### Connection Methods

- `connect(peers: List[str]) -> Client`
  - Connect to network nodes
  - `peers`: List of multiaddresses for initial network nodes

#### Data Operations

- `data_put_public(data: bytes, payment: PaymentOption) -> str`
  - Upload public data to the network
  - Returns address where data is stored

- `data_get_public(addr: str) -> bytes`
  - Download public data from the network
  - `addr`: Address returned from `data_put_public`

- `data_put(data: bytes, payment: PaymentOption) -> DataMapChunk`
  - Store private (encrypted) data
  - Returns access information for later retrieval

- `data_get(access: DataMapChunk) -> bytes`
  - Retrieve private data
  - `access`: DataMapChunk from previous `data_put`

#### Pointer Operations

- `pointer_get(address: str) -> Pointer`
  - Retrieve pointer from network
  - `address`: Hex-encoded pointer address

- `pointer_put(pointer: Pointer, wallet: Wallet)`
  - Store pointer on network
  - Requires payment via wallet

- `pointer_cost(key: VaultSecretKey) -> str`
  - Calculate pointer storage cost
  - Returns cost in atto tokens

#### Vault Operations

- `vault_cost(key: VaultSecretKey) -> str`
  - Calculate vault storage cost

- `write_bytes_to_vault(data: bytes, payment: PaymentOption, key: VaultSecretKey, content_type: int) -> str`
  - Write data to vault
  - Returns vault address

- `fetch_and_decrypt_vault(key: VaultSecretKey) -> Tuple[bytes, int]`
  - Retrieve vault data
  - Returns (data, content_type)

- `get_user_data_from_vault(key: VaultSecretKey) -> UserData`
  - Get user data from vault

- `put_user_data_to_vault(key: VaultSecretKey, payment: PaymentOption, user_data: UserData) -> str`
  - Store user data in vault
  - Returns vault address

### Wallet

Ethereum wallet management for payments.

- `new(private_key: str) -> Wallet`
  - Create wallet from private key
  - `private_key`: 64-char hex string without '0x' prefix

- `address() -> str`
  - Get wallet's Ethereum address

- `balance() -> str`
  - Get wallet's token balance

- `balance_of_gas() -> str`
  - Get wallet's gas balance

### PaymentOption

Configure payment methods.

- `wallet(wallet: Wallet) -> PaymentOption`
  - Create payment option from wallet

### Pointer

Handle network pointers for referencing data.

- `new(target: str) -> Pointer`
  - Create new pointer
  - `target`: Hex-encoded target address

- `address() -> str`
  - Get pointer's network address

- `target() -> str`
  - Get pointer's target address

### VaultSecretKey

Manage vault access keys.

- `new() -> VaultSecretKey`
  - Generate new key

- `from_hex(hex: str) -> VaultSecretKey`
  - Create from hex string

- `to_hex() -> str`
  - Convert to hex string

### UserData

Manage user data in vaults.

- `new() -> UserData`
  - Create new user data

- `add_file_archive(archive: str) -> Optional[str]`
  - Add file archive
  - Returns archive ID if successful

- `add_private_file_archive(archive: str) -> Optional[str]`
  - Add private archive
  - Returns archive ID if successful

- `file_archives() -> List[Tuple[str, str]]`
  - List archives as (id, address) pairs

- `private_file_archives() -> List[Tuple[str, str]]`
  - List private archives as (id, address) pairs

### DataMapChunk

Handle private data storage references.

- `from_hex(hex: str) -> DataMapChunk`
  - Create from hex string

- `to_hex() -> str`
  - Convert to hex string

- `address() -> str`
  - Get short reference address

### Utility Functions

- `encrypt(data: bytes) -> Tuple[bytes, List[bytes]]`
  - Self-encrypt data
  - Returns (data_map, chunks)

## Examples

See the `examples/` directory for complete examples:
- `autonomi_example.py`: Basic data operations
- `autonomi_pointers.py`: Working with pointers
- `autonomi_vault.py`: Vault operations
- `autonomi_private_data.py`: Private data handling
- `autonomi_private_encryption.py`: Data encryption
- `autonomi_advanced.py`: Advanced usage scenarios

## Best Practices

1. Always handle wallet private keys securely
2. Check operation costs before executing
3. Use appropriate error handling
4. Monitor wallet balance for payments
5. Use appropriate content types for vault storage
6. Consider using pointers for updatable references
7. Properly manage and backup vault keys

For more examples and detailed usage, see the examples in the repository.
