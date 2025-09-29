# AntNode Python Bindings

This document describes the Python bindings for the AntNode Rust implementation.

## Installation

The AntNode Python package is built using [maturin](https://github.com/PyO3/maturin) and requires Python 3.8 or later. We recommend using `uv` for Python environment management:

```bash
uv venv
uv pip install maturin
maturin develop
```

## Usage

```python
from antnode import AntNode

# Create a new node instance
node = AntNode()

# Start the node with configuration
node.run(
    rewards_address="0x1234567890123456789012345678901234567890",
    evm_network="arbitrum_sepolia",  # or "arbitrum_one"
    ip="0.0.0.0",
    port=12000,
    initial_peers=[],  # List of multiaddresses for initial peers
    local=True,       # Run in local mode
    root_dir=None,    # Custom root directory (optional)
)
```

## API Reference

### Constructor

#### `AntNode()`
Creates a new instance of the AntNode.

### Node Operations

#### `run(rewards_address: str, evm_network: str, ip: str = "0.0.0.0", port: int = 0, initial_peers: List[str] = [], local: bool = False, root_dir: Optional[str] = None) -> None`
Start the node with the given configuration.

- **Parameters:**
  - `rewards_address`: Ethereum address for rewards (hex string starting with "0x")
  - `evm_network`: Either "arbitrum_one" or "arbitrum_sepolia"
  - `ip`: IP address to bind to (default: "0.0.0.0")
  - `port`: Port number to use (default: 0 for random port)
  - `initial_peers`: List of multiaddresses for initial peers
  - `local`: Run in local mode
  - `root_dir`: Custom root directory path (optional)

#### `peer_id() -> str`
Get the node's PeerId as a string.

#### `get_rewards_address() -> str`
Get the node's rewards/wallet address as a hex string.

#### `set_rewards_address(address: str) -> None`
Set a new rewards/wallet address for the node.
- `address`: Hex string starting with "0x"

### Storage Operations

#### `store_record(key: str, value: bytes, record_type: str) -> None`
Store a record in the node's storage.
- `key`: Record key
- `value`: Record data as bytes
- `record_type`: Type of record

#### `get_record(key: str) -> Optional[bytes]`
Get a record from the node's storage.
- Returns `None` if record not found

#### `delete_record(key: str) -> bool`
Delete a record from the node's storage.
- Returns `True` if record was deleted

#### `get_stored_records_size() -> int`
Get the total size of stored records in bytes.

#### `get_all_record_addresses() -> List[str]`
Get all record addresses stored by the node.

### Network Operations

#### `get_kbuckets() -> List[Tuple[int, List[str]]]`
Get the node's kbuckets information.
- Returns list of tuples containing (distance, list of peer IDs)

### Directory Management

#### `get_root_dir() -> str`
Get the current root directory path for node data.

#### `get_default_root_dir(peer_id: Optional[str] = None) -> str`
Get the default root directory path for the given peer ID.
- Platform specific paths:
  - Linux: `$HOME/.local/share/autonomi/node/<peer-id>`
  - macOS: `$HOME/Library/Application Support/autonomi/node/<peer-id>`
  - Windows: `C:\Users\<username>\AppData\Roaming\autonomi\node\<peer-id>`

#### `get_logs_dir() -> str`
Get the logs directory path.

#### `get_data_dir() -> str`
Get the data directory path where records are stored.

## Error Handling

The bindings use Python exceptions to handle errors:
- `ValueError`: For invalid input parameters
- `RuntimeError`: For operational errors

## Example

See [example.py](../example.py) for a complete example of using the AntNode Python bindings.
