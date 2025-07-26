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
from autonomi_client import *

Client, Wallet, PaymentOption *

# Initialize a wallet with a private key
wallet = Wallet.new_from_private_key(Network(True),
                                     "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
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

#### Scratchpad

Manage mutable encrypted data on the network.

#### Scratchpad Class

- `Scratchpad(owner: SecretKey, data_encoding: int, unencrypted_data: bytes, counter: int) -> Scratchpad`
    - Create a new scratchpad instance
    - `owner`: Secret key for encrypting and signing
    - `data_encoding`: Custom value to identify data type (app-defined)
    - `unencrypted_data`: Raw data to be encrypted
    - `counter`: Version counter for tracking updates

- `address() -> ScratchpadAddress`
    - Get the address of the scratchpad

- `decrypt_data(sk: SecretKey) -> bytes`
    - Decrypt the data using the given secret key

#### Client Methods for Scratchpad

- `scratchpad_get_from_public_key(public_key: PublicKey) -> Scratchpad`
    - Retrieve a scratchpad using owner's public key

- `scratchpad_get(addr: ScratchpadAddress) -> Scratchpad`
    - Retrieve a scratchpad by its address

- `scratchpad_check_existance(addr: ScratchpadAddress) -> bool`
    - Check if a scratchpad exists on the network

- `scratchpad_put(scratchpad: Scratchpad, payment: PaymentOption) -> Tuple[str, ScratchpadAddress]`
    - Store a scratchpad on the network
    - Returns (cost, address)

-

`scratchpad_create(owner: SecretKey, content_type: int, initial_data: bytes, payment: PaymentOption) -> Tuple[str, ScratchpadAddress]`

- Create a new scratchpad with a counter of 0
- Returns (cost, address)

- `scratchpad_update(owner: SecretKey, content_type: int, data: bytes) -> None`
    - Update an existing scratchpad
    - **Note**: Counter is automatically incremented by 1 during update
    - The scratchpad must exist before updating

- `scratchpad_cost(public_key: PublicKey) -> str`
    - Calculate the cost to store a scratchpad
    - Returns cost in atto tokens

#### Important Notes on Scratchpad Counter

1. When creating a new scratchpad with `scratchpad_create`, the counter starts at 0
2. When updating with `scratchpad_update`, the counter is automatically incremented
3. If you need to set a specific counter value, create a new Scratchpad instance and use `scratchpad_put`
4. Only the scratchpad with the highest counter is kept on the network when there are conflicts

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

- `set_transaction_config(config: TransactionConfig)`
    - Set the transaction config for the wallet (see `TransactionConfig`)

### TransactionConfig

Transaction configuration for wallets.

```python
config = TransactionConfig(max_fee_per_gas=MaxFeePerGas.limited_auto(200000000))
```

#### MaxFeePerGas

Control the maximum fee per gas (gas price bid) for transactions:

- `MaxFeePerGas.auto()`: Use current network gas price. No gas price limit. Use with caution.
  ```python
  MaxFeePerGas.auto()
  ```

- `MaxFeePerGas.limited_auto(limit)`: Use current gas price with an upper limit. This is the recommended preset.
  ```python
  # Limit to 0.2 gwei
  MaxFeePerGas.limited_auto(200000000)
  ```

- `MaxFeePerGas.unlimited()`: No gas price limit. Use with caution.
  ```python
  MaxFeePerGas.unlimited()
  ```

- `MaxFeePerGas.custom(wei_amount)`: Set exact gas price in wei.
  ```python
  # Set to exactly 0.2 gwei
  MaxFeePerGas.custom(200000000)
  ```

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
- `autonomi_scratchpad.py`: Scratchpad creation and updates (with counter management)
- `autonomi_advanced.py`: Advanced usage scenarios

## Best Practices

1. Always handle wallet private keys securely
2. Check operation costs before executing
3. Use appropriate error handling
4. Monitor wallet balance for payments
5. Use appropriate content types for vault storage
6. Consider using pointers for updatable references
7. Properly manage and backup vault keys

#### Register Operations

Registers are mutable data structures on the Autonomi network that allow you to create updateable content with a versioned history. Each register is owned by a specific key and can only be updated by the owner.

##### Register Class

Registers are accessed through the Client and require a unique key for each register:

```python
from autonomi_client import *

# Initialize client and wallet
client = await Client.init()
wallet = Wallet.new_from_private_key(Network(True), "your_private_key")
payment = PaymentOption.wallet(wallet)

# Create a register key from a name
main_key = SecretKey.random()
register_key = Client.register_key_from_name(main_key, "my-register-1")
```

##### Key Register Methods

- `register_cost(public_key: PublicKey) -> str`
    - Calculate the cost to create a register
    - `public_key`: The public key for the register
    - Returns cost in atto tokens

- `register_create(key: SecretKey, content: RegisterValue, payment: PaymentOption) -> Tuple[str, RegisterAddress]`
    - Create a new register on the network
    - `key`: Secret key that owns this register
    - `content`: Initial content as RegisterValue
    - `payment`: Payment method
    - Returns (cost, address) tuple

- `register_get(address: RegisterAddress) -> RegisterValue`
    - Retrieve current value of a register
    - `address`: RegisterAddress to retrieve from

- `register_update(key: SecretKey, content: RegisterValue, payment: PaymentOption) -> str`
    - Update an existing register with new content
    - `key`: Secret key that owns the register
    - `content`: New content as RegisterValue
    - Returns cost in atto tokens

- `register_history(address: RegisterAddress) -> RegisterHistory`
    - Get the complete version history of a register
    - Returns iterator over all versions

##### Utility Methods

- `Client.register_key_from_name(main_key: SecretKey, name: str) -> SecretKey`
    - Generate a deterministic register key from a main key and name
    - Allows you to recreate the same register key later

- `Client.register_value_from_bytes(data: bytes) -> RegisterValue`
    - Create RegisterValue from byte data (limited to 32 bytes)

##### RegisterAddress Class

- `RegisterAddress(public_key: PublicKey) -> RegisterAddress`
    - Create register address from owner's public key

- `from_hex(hex: str) -> RegisterAddress`
    - Create register address from hex string

- `to_hex() -> str`
    - Convert register address to hex string

- `owner() -> PublicKey`
    - Get the public key that owns this register

##### Complete Register Example

```python
from autonomi_client import *

async def register_example():
    # Initialize client and wallet
    client = await Client.init()
    wallet = Wallet.new_from_private_key(Network(True), 
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
    payment = PaymentOption.wallet(wallet)
    
    # Generate keys
    main_key = SecretKey.random()
    register_key = Client.register_key_from_name(main_key, "my-counter")
    
    # Create register content (max 32 bytes)
    initial_content = Client.register_value_from_bytes(b"counter: 0")
    
    # Check cost before creating
    cost = await client.register_cost(register_key.public_key())
    print(f"Register creation cost: {cost} AttoTokens")
    
    # Create the register
    creation_cost, address = await client.register_create(
        register_key, initial_content, payment)
    print(f"Register created at: {address.to_hex()}")
    
    # Wait for network replication
    await asyncio.sleep(5)
    
    # Read current value
    current_value = await client.register_get(address)
    print(f"Current value: {current_value}")
    
    # Update the register
    new_content = Client.register_value_from_bytes(b"counter: 1")
    update_cost = await client.register_update(register_key, new_content, payment)
    print(f"Update cost: {update_cost} AttoTokens")
    
    # Wait for replication
    await asyncio.sleep(5)
    
    # Get updated value
    updated_value = await client.register_get(address)
    print(f"Updated value: {updated_value}")
    
    # Get complete history
    history = client.register_history(address)
    all_versions = await history.collect()
    print(f"History has {len(all_versions)} versions:")
    for i, version in enumerate(all_versions):
        print(f"  Version {i}: {version}")

# Run the example
asyncio.run(register_example())
```

##### Register Use Cases

1. **Mutable Configuration**: Store application settings that need periodic updates
2. **Status Updates**: Maintain current status or state information
3. **Version Control**: Track document or data versions with full history
4. **Counters**: Implement distributed counters or sequence numbers
5. **Metadata**: Store changeable metadata for files or applications

##### Important Register Notes

1. **Size Limit**: Register values are limited to 32 bytes maximum
2. **Ownership**: Only the key holder can update a register
3. **Network Delays**: Allow time for network replication after operations
4. **Deterministic Keys**: Use `register_key_from_name()` for consistent key generation
5. **Payment Required**: Both creation and updates require payment
6. **History Preserved**: All versions are permanently stored and accessible

For more examples and detailed usage, see the examples in the repository.
