# Autonomi CLI (`ant`)

> A command-line interface for the Autonomi Network - store data permanently with lifetime storage, private by design.

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](http://www.gnu.org/licenses/gpl-3.0)

The Autonomi CLI (`ant`) is your gateway to the Autonomi Network, enabling you to upload files, create encrypted vaults, manage registers, and interact with the world's first truly autonomous data network from your terminal.

## Table of Contents

- [Quick Start](#quick-start)
- [Features](#features)
- [Prerequisites](#prerequisites)
- [Installation](#installation)
- [Usage Overview](#usage-overview)
- [Command Reference](#command-reference)
  - [File Operations](#file-operations)
  - [Register Operations](#register-operations)
  - [Vault Operations](#vault-operations)
  - [Scratchpad Operations](#scratchpad-operations)
  - [Pointer Operations](#pointer-operations)
  - [Wallet Operations](#wallet-operations)
  - [Analyze Operations](#analyze-operations)
- [Global Options](#global-options)
- [Examples & Workflows](#examples--workflows)
- [Configuration](#configuration)
- [Troubleshooting](#troubleshooting)
- [FAQ](#faq)
- [Performance & Limits](#performance--limits)
- [Security Best Practices](#security-best-practices)
- [Contributing](#contributing)
- [License](#license)

## Quick Start

Get up and running in 5 minutes:

### 1. Install the CLI
```bash
# Download from releases or build from source (see Installation section)
ant --version
```

### 2. Create a Wallet
```bash
ant wallet create
# Save your private key securely!
```

### 3. Upload Your First File
```bash
ant file upload myfile.txt
# Returns: File uploaded to: <network-address>
```

### 4. Download It Back
```bash
ant file download <network-address> downloaded.txt
# Your file is retrieved from the network!
```

That's it! You've just stored data permanently on the Autonomi Network.

## Features

- **Lifetime Storage**: Pay once, store forever - no recurring fees
- **Private by Design**: End-to-end encryption with self-encryption technology
- **Permanent Availability**: Data stored across a decentralized network
- **No Blockchain Bloat**: Fast, efficient storage without traditional consensus overhead
- **Vault System**: Personal encrypted storage for organizing your files
- **Mutable Data**: Registers and scratchpads for data that needs to change
- **Public & Private Files**: Share publicly or keep files encrypted
- **Gas-Optimized Uploads**: Smart retry mechanisms for cost-effective storage

## Prerequisites

Before using the Autonomi CLI, ensure you have:

### For All Users
- **Network Access**: Connection to the Autonomi Network (mainnet or local testnet)
- **Wallet**: An EVM-compatible wallet with tokens for storage payments
  - Create with: `ant wallet create`
  - Or import existing: `ant wallet import <private_key>`

### For Local Development
- **EVM Testnet**: Run local test network for development
  ```bash
  cargo run --bin evm-testnet
  ```
- **Local Network**: Set up nodes for testing
  ```bash
  cargo run --bin antctl -- local run --build --clean --rewards-address <YOUR_ETHEREUM_ADDRESS>
  ```
- **Environment Variables**:
  - `SECRET_KEY`: Your EVM wallet private key (for non-interactive operations)
  - `ANT_PEERS`: Bootstrap peers (optional)

### For Building from Source
- **Rust**: Version 1.70 or later ([rust-lang.org](https://rust-lang.org))
- **Cargo**: Comes with Rust installation
- **Git**: For cloning the repository

## Usage Overview

The Autonomi CLI follows a simple pattern:

```bash
ant [GLOBAL_OPTIONS] <COMMAND> [COMMAND_OPTIONS] [ARGUMENTS]
```

### Core Concepts

**Data Flow**: Your data goes through these stages:
1. **Upload** → Files are encrypted and stored on the network
2. **Vault** → Organize and track your uploads in a personal vault
3. **Download** → Retrieve data using network addresses

**Command Categories**:

| Category | Purpose | When to Use |
|----------|---------|-------------|
| **file** | Store and retrieve files | Permanent data storage |
| **vault** | Organize your files | Managing multiple uploads |
| **register** | Mutable key-value data | Data that changes over time |
| **scratchpad** | Shared mutable data (up to 4MB) | Collaborative data editing |
| **pointer** | Named references to other data | Create updatable links to graphs/scratchpads/chunks |
| **wallet** | Manage your payment wallet | Fund storage operations |
| **analyze** | Inspect network addresses | Debug or explore data |

### Common Workflows

1. **One-time file upload**: `file upload` → save address → done
2. **Organized storage**: `vault create` → `file upload` → `vault sync`
3. **Mutable data**: `register create` → `register edit` (update as needed)
4. **Collaboration**: `scratchpad create` → `scratchpad share` → others edit with shared key

### Quick Command Reference

**File Operations**:
```bash
ant file cost <file>                    # Estimate storage cost
ant file upload <file> [--public]       # Upload to network
ant file download <addr> <dest>         # Retrieve file
ant file list                           # List vault files
```

**Wallet Operations**:
```bash
ant wallet create                       # Create new wallet
ant wallet import <private_key>         # Import existing wallet
ant wallet balance                      # Check funds
ant wallet export                       # View wallet details
```

**Register Operations** (Mutable Data):
```bash
ant register generate-key               # Create register key
ant register cost <name>                # Estimate register cost
ant register create <name> <value>      # Create named register
ant register edit <address> <value>     # Update register
ant register get <address>              # Read register
ant register history <address>          # View register history
ant register list                       # List all registers
```

**Scratchpad Operations** (Shared Mutable Data):
```bash
ant scratchpad generate-key             # Create scratchpad key
ant scratchpad cost <name>              # Estimate scratchpad cost
ant scratchpad create <name> <data>     # Create scratchpad (up to 4MB)
ant scratchpad share <name>             # Get shareable secret key
ant scratchpad get <name>               # Read scratchpad
ant scratchpad edit <name> <data>       # Update scratchpad
ant scratchpad list                     # List all scratchpads
```

**Vault Operations** (Organization):
```bash
ant vault cost                          # Estimate vault cost
ant vault create                        # Initialize vault
ant vault sync                          # Upload local metadata
ant vault load                          # Download vault data
```

**Pointer Operations** (Named References):
```bash
ant pointer generate-key                # Create pointer key
ant pointer cost <name>                 # Estimate pointer cost
ant pointer create <name> <target>      # Create pointer to data
ant pointer share <name>                # Get shareable secret key
ant pointer get <name>                  # Resolve pointer
ant pointer edit <name> <target>        # Update pointer target
ant pointer list                        # List all pointers
```

**Analyze Operations**:
```bash
ant analyze <address>                   # Analyze and visualize network address
```

For detailed command documentation, see the [Command Reference](#command-reference) section below.


## Installation

You can install the Autonomi CLI in two ways: by downloading pre-built binaries or building from source.

### Option 1: Download Pre-Built Binary (Recommended)

1. Visit the [Releases](https://github.com/maidsafe/autonomi/releases) page on GitHub
2. Download the latest release for your operating system:
   - Windows: `ant-<version>-x86_64-pc-windows-msvc.zip`
   - macOS (Intel): `ant-<version>-x86_64-apple-darwin.tar.gz`
   - macOS (Apple Silicon): `ant-<version>-aarch64-apple-darwin.tar.gz`
   - Linux: `ant-<version>-x86_64-unknown-linux-musl.tar.gz`
3. Extract the downloaded archive
4. Move the `ant` binary to a directory in your system's PATH

**Verify installation:**
```bash
ant --version
```

### Option 2: Build from Source

**Prerequisites**: Rust 1.70+ and Cargo ([install from rust-lang.org](https://rust-lang.org))

1. **Clone the repository:**
```bash
git clone https://github.com/maidsafe/autonomi.git
cd autonomi
```

2. **Build the CLI:**
```bash
cargo build --release --bin=ant
```

The binary will be created at: `target/release/ant` (or `target/release/ant.exe` on Windows)

3. **Add to your PATH:**

#### Windows (PowerShell)
```powershell
# Temporary (current session only)
$env:PATH += ";C:\path\to\autonomi\target\release"

# Permanent
[System.Environment]::SetEnvironmentVariable("PATH", $env:PATH + ";C:\path\to\autonomi\target\release", [System.EnvironmentVariableTarget]::User)
```

#### macOS and Linux (Bash)
```bash
# Temporary (current session only)
export PATH=$PATH:/path/to/autonomi/target/release

# Permanent
echo 'export PATH=$PATH:/path/to/autonomi/target/release' >> ~/.bashrc
source ~/.bashrc
```

4. **Verify installation:**
```bash
ant --version
```

### Troubleshooting Installation

If `ant --version` doesn't work:
- Ensure the binary is executable: `chmod +x /path/to/ant` (macOS/Linux)
- Verify PATH is set correctly: `echo $PATH` (macOS/Linux) or `echo $env:PATH` (Windows)
- Try using the full path: `/path/to/ant --version`
- See the [Troubleshooting](#troubleshooting) section for more help

## Command Reference

This section provides detailed documentation for all available commands and options.

## Global Options

These options can be used with any command:

### Network Selection
```
--alpha
```
Connect to the alpha network instead of mainnet.

```
--network-id <ID>
```
Specify the network ID to use. This allows you to run the CLI on different networks.

Valid values:
- `0`: Local Network
- `1`: Mainnet (default)
- `2`: Alpha Network
- `3-255`: Custom Networks (configured via environment variables and other network config flags)

### Version Information
```
--version
```
Print version information.

```
--crate-version
```
Print the crate version.

```
--package-version
```
Print the package version.

```
--protocol-version
```
Print the network protocol version.

### Specify the logging output destination.
```
--log-output-dest <LOG_OUTPUT_DEST>
```

Default value: `data-dir`\
Valid values: [`stdout` , `data-dir` , <custom path\>]

The data directory location is platform specific:
| OS  | Path |
| ------------- |:-------------:|
| Linux | $HOME/.local/share/autonomi/client/logs |
| macOS | $HOME/Library/Application Support/autonomi/client/logs |
| Windows | %AppData%\autonomi\client\logs |

### Specify the logging format.
```
--log-format <LOG_FORMAT>
```   
Valid values [`default` , `json`]

If the argument is not used, the default format will be applied.

### Specify the Connection Timeout
```
--timeout <CONNECTION_TIMEOUT>
```  

Default value: `120`\
Valid values: [`0 - 999`]

The maximum duration to wait for a connection to the network before timing out.\
This value is expressed in seconds.

### Prevent verification of data storage on the network.
```
-x, --no-verify
```
This may increase operation speed, but offers no guarantees that operations were successful.

---

### File Operations

#### Get a cost estimate for storing a file
```
file cost <file> [--merkle] [--disable-single-node-payment]
```

Gets a cost estimate for uploading a file to the network.
This returns both the storage costs and gas fees for the file.

Expected value: 
- `<file>`: File path (accessible by current user)

The following flags can be applied:
- `--merkle` (Optional) Use Merkle batch payment mode instead of standard payment. Merkle mode pays for all chunks in a single transaction, saving gas fees.
- `--disable-single-node-payment` (Optional) Use standard payment mode instead of single-node payment. Standard mode pays 3 nodes individually, which costs more in gas fees. Single-node payment (default) pays only one node with 3x that amount, saving gas fees. This flag only applies to standard payment mode (not Merkle).


#### Upload a file
```
file upload <file> [--public] [--no-archive] [--retry-failed <N>] [--merkle] [--disable-single-node-payment] [--max-fee-per-gas <value>]
```
Uploads a file to the network.

Expected value: 
- `<file>`: File path (accessible by current user)

The following flags can be added:
- `--public` (Optional) Specifying this will make this file publicly available to anyone on the network
- `--no-archive` (Optional) Skip creating local archive after upload. Only upload files without saving archive information. Note that --no-archive is the default behaviour for single file uploads (folk can still upload a single file as an archive by putting it in a directory)
- `--retry-failed <N>` (Optional) Automatically retry failed uploads. This is particularly useful for handling gas fee errors when the network base fee exceeds your --max-fee-per-gas setting. The retry mechanism works at the batch level, so only failed chunks are retried, not the entire file upload process. Default is `0` for no retry.
- `--merkle` (Optional) Use Merkle batch payment mode instead of standard payment. Merkle mode pays for all chunks in a single transaction, saving gas fees.
- `--disable-single-node-payment` (Optional) Use standard payment mode instead of single-node payment. Standard mode pays 3 nodes individually, which costs more gas. Single-node payment (default) pays only one node with 3x that amount. Data is stored on 5 nodes regardless of payment mode. This flag only applies to standard payment mode (not Merkle).
- `--max-fee-per-gas <value>` (Optional) Maximum fee per gas / gas price bid. Options: `low`, `market` (default), `auto`, `limited-auto:<WEI>`, `unlimited`, or a specific `<WEI AMOUNT>`.

Example usage with retry functionality:
```
ant file upload myfile.txt --public --retry-failed 3 --max-fee-per-gas 10000000
```
This will upload the file publicly and automatically retry if the base fee is higher than arbitrums minimum gas fee, showing detailed error messages with current gas prices. Using these settings ensures your data goes up at minimum cost (but depending on current blockchain fees and the amount of data this might take a while)

#### Download a file
```
file download <addr> <dest_path> [-q, --quorum <QUORUM>] [-r, --retries <N>] [--disable-cache] [--cache-dir <PATH>]
```
Download a file from network address to output path

Expected values: 
- `<addr>`: The network address of a file
- `<dest_path>`: The output path to download the file to

The following flags can be applied:
- `-q, --quorum <QUORUM>` (Optional, Experimental) Specify the quorum for the download (ensures we have n copies for each chunk). Possible values: `one`, `majority`, `all`, or a number greater than 0.
- `-r, --retries <N>` (Optional, Experimental) Specify the number of retries for the download.
- `--disable-cache` (Optional) Disable chunk caching. By default, chunks are cached to enable resuming downloads.
- `--cache-dir <PATH>` (Optional) Custom cache directory for chunk caching. If not specified, uses the default Autonomi client data directory. Only applies when cache is enabled (default).

#### List the files in a vault
```
file list [-v, --verbose]
```
Lists all files (both public and private) in a vault.

The following flag can be applied:
- `-v, --verbose` (Optional) List files with network details (requires network connection).


### Register Operations

#### Generate a key for a register
```
register generate-key [--overwrite]
```
Generate a new register key

The following flag can be applied:
`--overwrite` (Optional) Adding this flag will overwrite any existing key, and result in loss of access to any existing registers created using that key


#### Get a cost estimate for storing a register on the network
```
register cost <name>
```
Gets a cost estimate for storing a register on the network.
This returns both the storage costs and gas fees.

#### Create a new register and upload to the network
```
register create <name> <value> [--hex] [--max-fee-per-gas <value>]
```
Create a new register with the given name and value.
Note: that anyone with the register address can read its value.

Expected values: 
- `<name>`: The name of the register
- `<value>`: The value to store in the register

The following flags can be applied:
- `--hex` (Optional) Treat the value as a hex string and convert it to binary before storing.
- `--max-fee-per-gas <value>` (Optional) Maximum fee per gas / gas price bid.

#### Edit an existing register
```
register edit [--name] <address> <value> [--hex] [--max-fee-per-gas <value>]
```
Edit an existing register

Expected values: 
- `<address>`: The address of the register to edit
- `<value>`: The new value to store in the register

The following flags can be applied:
- `--name` (Optional) Use the name of the register instead of the address. Note: only the owner of the register can use this shorthand as the address can be generated from the name and register key.
- `--hex` (Optional) Treat the value as a hex string and convert it to binary before storing.
- `--max-fee-per-gas <value>` (Optional) Maximum fee per gas / gas price bid.

#### Get a register
```
register get [--name] <address> [--hex]
```
Get a register from the network

Expected values: 
- `<address>`: The address of the register

The following flags can be applied:
- `--name` (Optional) Use the name of the register instead of the address. Note: only the owner of the register can use this shorthand as the address can be generated from the name and register key.
- `--hex` (Optional) Display the value as a hex string instead of raw bytes.

#### Get register history
```
register history <address> [-n, --name] [--hex]
```
Show the history of all values that have been stored in a register.

Expected values:
- `<address>`: The address of the register

The following flags can be applied:
- `-n, --name` (Optional) Use the name of the register instead of the address
- `--hex` (Optional) Display values as hex strings instead of raw bytes

Note: Only the owner of the register can use the `--name` shorthand as the address can be generated from the name and register key.

#### List registers
```
register list
```
List local registers


### Vault Operations

#### Get a cost estimate for storing a vault on the network
```
vault cost [expected_max_size]
```
Gets a cost estimate for uploading a vault to the network.
This returns both the storage costs and gas fees for the vault.

Expected value:
- `[expected_max_size]` (Optional) Expected maximum size of a vault, only for cost estimation. Default: `3145728` (3MB).

#### Create a new vault and upload to the network
```
vault create [--max-fee-per-gas <value>]
```
Creates a new vault and uploads it to the network.
This will initialise a new vault in the local storage and then upload it to the network.

The following flag can be applied:
- `--max-fee-per-gas <value>` (Optional) Maximum fee per gas / gas price bid.

#### Load vault from the network
```
vault load
```
Retrieves data from the network and writes it to local storage.
This will download the vault data from the network and synchronise it with the local storage.

#### Sync local data with the network
```
vault sync [--force]
```
Sync the users local data with the network vault data.

The following flag can be applied:
`--force` (Optional) Add this flag to overwrite data in the vault with local user data

### Wallet Operations
#### Create a new wallet
```
wallet create [--no-password] 
```

You will be prompted for an optional password, ignoring this will not encrypt the wallet.
This will output the private key for the wallet, the public key for the wallet, and the stored location on device.

The following flags can be used to explictly include or exclude encryption of the created wallet

`--no-password` (Optional) Add this flag to skip the password prompt and encryption step. \
`--password <password>` (Optional) Add this flag to encrypt the create wallet

Note on wallet security
Encrypted wallets provide an additional layer of security, requiring a password to read the private key and perform transactions. However, ensure you remember your password; losing it may result in the inability to access your encrypted wallet.

#### Imports an existing wallet from a private key
```
wallet import <private_key>
```

The following flags can be used to explictly include or exclude encryption of the imported wallet

`--no-password` (Optional) Add this flag to skip the password prompt and encryption step. \
`--password <password>` (Optional) Add this flag to encrypt the create wallet


#### Displays the wallet balance
```
wallet balance
```
This will display both the token and gas balances.

#### Display the wallet details
```
wallet export
```
This will display both the address and private key of the wallet.

### Analyze Operations

Analyze an address to get the address type, and visualize the content.

```
analyze <address> [--closest-nodes] [--holders] [--nodes-health] [--repair] [--addr-range <HEX>] [-r, --recursive] [-v, --verbose] [--json <PATH>]
```

Expected value:
- `<address>`: The address of the data to analyse

The following flags can be applied:
- `--closest-nodes` (Optional) Show closest nodes to this address instead of analyzing it.
- `--holders` (Optional) Show all holders of the record at this address.
- `--nodes-health` (Optional) Check health of closest nodes by requesting storage proofs for the target chunk address.
- `--repair` (Optional) Repair records with insufficient copies in closest group. When analyzing with --closest-nodes, automatically re-upload records that have less than 3 holders among the closest 7 nodes.
- `--addr-range <HEX>` (Optional) Filter network scan to only target addresses starting with this hex character (0-9, a-f). Only applies when using --nodes-health with a number of rounds.
- `-r, --recursive` (Optional) Recursively analyze all discovered addresses (chunks, pointers, etc.)
- `-v, --verbose` (Optional) Verbose output with detailed description of the analysis.
- `--json <PATH>` (Optional) Output results as JSON to a file with append-only writing. If path is a file, appends to that file. If path is a directory, enables file rotations (50MB max per file, 10 files max).

### Scratchpad Operations

#### Generate a new scratchpad key
```
scratchpad generate-key [--overwrite]
```
Generate a new general scratchpad key from which all your scratchpad keys can be derived.

The following flag can be applied:
`--overwrite` (Optional) Warning: overwriting the existing key will result in loss of access to any existing scratchpads

#### Get a cost estimate for creating a scratchpad
```
scratchpad cost <name>
```
Gets a cost estimate for creating a scratchpad on the network.

Expected values:
- `<name>`: The name of the scratchpad

#### Create a new scratchpad
```
scratchpad create <name> <data> [--max-fee-per-gas <value>]
```
Create a new scratchpad with the given name and data.

Expected values:
- `<name>`: The name of the scratchpad
- `<data>`: The data to store in the scratchpad (Up to 4MB)

The following flag can be applied:
`--max-fee-per-gas <value>` (Optional) Specify the maximum fee per gas

#### Share a scratchpad
```
scratchpad share <name>
```
Share a scratchpad secret key with someone else. Sharing this key means that the other party will have permanent read and write access to the scratchpad.

Expected values:
- `<name>`: The name of the scratchpad

#### Get a scratchpad
```
scratchpad get <name> [--secret-key] [--hex]
```
Get the contents of an existing scratchpad from the network.

Expected values:
- `<name>`: The name of the scratchpad

The following flags can be applied:
`--secret-key` (Optional) Indicate that this is an external scratchpad secret key (Use when interacting with a shared scratchpad)
`--hex` (Optional) Display the data as a hex string instead of raw bytes

#### Edit a scratchpad
```
scratchpad edit <name> <data> [--secret-key]
```
Edit the contents of an existing scratchpad.

Expected values:
- `<name>`: The name of the scratchpad
- `<data>`: The new data to store in the scratchpad (Up to 4MB)

The following flag can be applied:
`--secret-key` (Optional) Indicate that this is an external scratchpad secret key (Use when interacting with a shared scratchpad)

#### List scratchpads
```
scratchpad list [-v, --verbose]
```
List owned scratchpads.

The following flag can be applied:
`-v, --verbose` (Optional) Show counter and data size for each scratchpad

### Pointer Operations

Pointers are named references that can point to other data on the network (graphs, scratchpads, other pointers, or chunks). They provide a way to create updatable links to data, allowing you to change what the pointer references without changing the pointer's name or address.

#### Generate a new pointer key
```
pointer generate-key [--overwrite]
```
Generate a new pointer signing key.

The following flag can be applied:
`--overwrite` (Optional) Warning: overwriting the existing key will result in loss of access to any existing pointers

#### Get a cost estimate for creating a pointer
```
pointer cost <name>
```
Gets a cost estimate for creating a pointer on the network.

Expected values:
- `<name>`: The name of the pointer

#### Create a new pointer
```
pointer create <name> <target> [-t, --target-data-type <type>] [--max-fee-per-gas <value>]
```
Create a new pointer with the given name that points to a target address.

Expected values:
- `<name>`: The name of the pointer
- `<target>`: The network address to point to

The following flags can be applied:
- `-t, --target-data-type <type>` (Optional) Specify the type of data being pointed to. Valid values: `auto`, `graph`, `scratchpad`, `pointer`, `chunk`. Default: `auto` (auto-detect by fetching from network)
- `--max-fee-per-gas <value>` (Optional) Specify the maximum fee per gas

#### Share a pointer
```
pointer share <name>
```
Share a pointer secret key with someone else. Sharing this key means that the other party will have permanent read and write access to update the pointer.

Expected values:
- `<name>`: The name of the pointer

#### Get a pointer
```
pointer get <name> [--secret-key]
```
Retrieve the target address that a pointer is pointing to.

Expected values:
- `<name>`: The name of the pointer (or the secret key if using `--secret-key` flag)

The following flag can be applied:
`--secret-key` (Optional) Indicate that this is an external pointer secret key (Use when interacting with a shared pointer)

#### Edit a pointer
```
pointer edit <name> <target> [-t, --target-data-type <type>] [--secret-key]
```
Update the target address that an existing pointer points to.

Expected values:
- `<name>`: The name of the pointer (or the secret key if using `--secret-key` flag)
- `<target>`: The new network address to point to

The following flags can be applied:
- `-t, --target-data-type <type>` (Optional) Specify the type of data being pointed to. Valid values: `auto`, `graph`, `scratchpad`, `pointer`, `chunk`. Default: `auto`
- `--secret-key` (Optional) Indicate that this is an external pointer secret key

#### List pointers
```
pointer list [-v, --verbose]
```
List all owned pointers.

The following flag can be applied:
`-v, --verbose` (Optional) Show counter and target details for each pointer

## Examples & Workflows

This section demonstrates complete workflows for common use cases.

### Example 1: Upload and Share a Public File

```bash
# 1. Create a wallet if you don't have one
ant wallet create
# Save your private key!

# 2. Check the cost estimate
ant file cost myfile.pdf

# 3. Upload the file publicly
ant file upload myfile.pdf --public

# Output: File uploaded to: 502f7b794a2022c3ff1a2ce3fbf2d...
# Anyone can now download this file using this address

# 4. Share the address with others
# They can download with:
ant file download 502f7b794a2022c3ff1a2ce3fbf2d downloaded.pdf
```

### Example 2: Private File Upload with Vault

```bash
# 1. Create a vault to organize your files
ant vault create

# 2. Upload a private file (default)
ant file upload confidential.txt

# Output: File uploaded to: 6a8f5b3c9d1e7f4a8b2c5d9e3f1a7b4...
# File is encrypted and only you can access it

# 3. Sync vault to save file metadata
ant vault sync

# 4. List all files in your vault
ant file list

# 5. Download your file later
ant file download 6a8f5b3c9d1e7f4a8b2c5d9e3f1a7b4 restored.txt
```

### Example 3: Use Registers for Mutable Data

```bash
# 1. Generate a register key (once)
ant register generate-key

# 2. Create a register with initial value
ant register create my-counter "0"

# Output: Register created at: 7c9e2f5a8b3d6e1f4a7b9c2d5e8f1a3...

# 3. Update the register value
ant register edit 7c9e2f5a8b3d6e1f4a7b9c2d5e8f1a3 "42"

# Or use the name (if you own the register)
ant register edit --name my-counter "100"

# 4. Read the current value
ant register get --name my-counter
# Output: 100
```

### Example 4: Share a Scratchpad for Collaboration

```bash
# 1. Generate scratchpad key (once)
ant scratchpad generate-key

# 2. Create a scratchpad with initial data
ant scratchpad create meeting-notes "Team meeting - Jan 2025"

# 3. Get the shareable secret key
ant scratchpad share meeting-notes

# Output: Secret key: 8d1f3a5b7c9e2f4a6b8c1d3e5f7a9b2...
# Share this key with collaborators

# 4. You or collaborators can edit using the secret key
ant scratchpad edit meeting-notes "Updated notes" --secret-key

# 5. Anyone with the key can read
ant scratchpad get meeting-notes --secret-key
```

### Example 5: Upload with Gas Fee Retry

```bash
# Upload with automatic retry if gas fees are too high
ant file upload large-file.zip --public --retry-failed 3 --max-fee-per-gas 10000000

# This will:
# - Attempt upload with max gas fee of 10000000
# - Retry up to 3 times if base fee exceeds your limit
# - Only retry failed chunks, not the entire file
# - Ensure cost-effective upload (may take longer)
```

### Example 6: Use Pointers for Updatable References

```bash
# 1. Generate a pointer key (once)
ant pointer generate-key

# 2. Create a scratchpad with some data
ant scratchpad create config "version=1.0.0"

# Output: Scratchpad created at: 9a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5...

# 3. Create a pointer that references the scratchpad
ant pointer create my-config 9a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5 --target-data-type scratchpad

# Output: Pointer created at: 1f2e3d4c5b6a7b8c9d0e1f2a3b4c5d6...

# 4. Later, get the pointer to find the current config
ant pointer get my-config

# Output: 9a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5... (points to scratchpad)

# 5. Update the scratchpad with new config
ant scratchpad edit config "version=2.0.0"

# 6. Create a new scratchpad with different config
ant scratchpad create config-v2 "version=2.0.0,feature=enabled"

# Output: 7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2...

# 7. Update the pointer to reference the new scratchpad
ant pointer edit my-config 7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2 --target-data-type scratchpad

# Now "my-config" points to the new scratchpad!
# Anyone using the pointer name always gets the latest config

# 8. List all your pointers
ant pointer list --verbose
```

**Use case**: Pointers allow you to create stable names (like "my-config") that can point to different data over time. This is useful for versioning, configuration management, or any scenario where you need an updatable reference without changing the name.

## Configuration

### Environment Variables

The CLI recognizes these environment variables:

| Variable | Purpose | Example |
|----------|---------|---------|
| `ANT_PEERS` | Bootstrap peers (multiaddr format) | `/ip4/127.0.0.1/tcp/12000/p2p/12D3Koo...` |
| `SECRET_KEY` | EVM wallet private key (for automation) | `0x1234567890abcdef...` |

**Using environment variables:**

```bash
# Linux/macOS
export ANT_PEERS="/ip4/127.0.0.1/tcp/12000/p2p/12D3Koo..."
export SECRET_KEY="0x..."
ant file upload myfile.txt

# Windows PowerShell
$env:ANT_PEERS="/ip4/127.0.0.1/tcp/12000/p2p/12D3Koo..."
$env:SECRET_KEY="0x..."
ant file upload myfile.txt
```

### Data Directories

The CLI stores data in platform-specific locations:

| OS | Logs | Wallet | Vault |
|----|------|--------|-------|
| **Linux** | `$HOME/.local/share/autonomi/client/logs` | `$HOME/.local/share/autonomi/client` | `$HOME/.local/share/autonomi/client` |
| **macOS** | `$HOME/Library/Application Support/autonomi/client/logs` | `$HOME/Library/Application Support/autonomi/client` | `$HOME/Library/Application Support/autonomi/client` |
| **Windows** | `%AppData%\autonomi\client\logs` | `%AppData%\autonomi\client` | `%AppData%\autonomi\client` |

### Persistent Configuration

You can set default options using command-line flags on each invocation, or use environment variables for automation:

```bash
# Always use specific peers
export ANT_PEERS="/ip4/142.93.37.4/tcp/40184/p2p/12D3Koo..."

# Always log to stdout
alias ant='ant --log-output-dest stdout'
```

## Troubleshooting

### Common Issues and Solutions

#### "Failed to connect to network"

**Symptoms**: CLI cannot connect to any peers

**Solutions**:
1. Check your internet connection
2. Verify bootstrap peers are correct: `--peer <multiaddr>`
3. Increase timeout: `--timeout 300`
4. Check firewall settings (allow outbound connections)
5. Try using explicit peers via `ANT_PEERS` environment variable

#### "Insufficient funds" or "Gas fee too high"

**Symptoms**: Upload fails with payment errors

**Solutions**:
1. Check wallet balance: `ant wallet balance`
2. Fund your wallet with tokens
3. Use `--max-fee-per-gas` to set a limit
4. Enable retry: `--retry-failed 3`
5. Wait for lower network congestion

#### "Wallet decryption failed"

**Symptoms**: Cannot access encrypted wallet

**Solutions**:
1. Verify you're entering the correct password
2. Check wallet file hasn't been corrupted
3. Restore from backup if available
4. Import using private key: `ant wallet import <key>`

#### "File not found" when downloading

**Symptoms**: Download fails with "chunk not found" errors

**Solutions**:
1. Verify the address is correct
2. Check network connectivity (`--timeout 300`)
3. The file may not have fully replicated yet (wait and retry)
4. If upload didn't complete, the file may be incomplete

#### "Register/Scratchpad key not found"

**Symptoms**: Cannot access previously created registers/scratchpads

**Solutions**:
1. Ensure you generated a key: `ant register generate-key`
2. Key file may have been deleted - regeneration creates a NEW key
3. Check data directory for key files
4. Cannot recover old registers/scratchpads without the original key

#### Upload fails midway

**Symptoms**: Large file upload stops or errors

**Solutions**:
1. Use `--retry-failed` to automatically retry failed chunks
2. Check wallet balance (may have run out of funds)
3. Increase timeout for large files: `--timeout 600`
4. Check network stability
5. Failed uploads are retried at the chunk level, not full file

### Debugging with Logs

Enable detailed logging to diagnose issues:

```bash
# Log to stdout with JSON format
ant --log-output-dest stdout --log-format json file upload test.txt

# Log to custom directory
ant --log-output-dest /path/to/logs file upload test.txt

# View existing logs
# Linux/macOS
cat ~/.local/share/autonomi/client/logs/ant.log

# Windows
type %AppData%\autonomi\client\logs\ant.log
```

## FAQ

### General Questions

**Q: How much does storage cost?**

A: Storage costs vary based on network conditions and file size. Use `ant file cost <file>` to get an estimate before uploading. Costs include storage payment and gas fees.

**Q: How long does data persist on the network?**

A: Data is stored permanently with a one-time payment. There are no recurring fees or expiration.

**Q: Can I delete data after uploading?**

A: No, uploaded data is permanent and cannot be deleted. Only upload data you're comfortable storing indefinitely.

**Q: What's the difference between public and private files?**

A:
- **Private** (default): Encrypted, only you can access with your credentials
- **Public** (`--public`): Anyone with the network address can download

**Q: Can I use the same wallet across multiple devices?**

A: Yes! Export your wallet (`ant wallet export`), then import the private key on another device (`ant wallet import <key>`). Keep your private key secure.

**Q: What happens if my upload fails midway?**

A: The CLI uploads files in chunks. Use `--retry-failed` to automatically retry only the failed chunks, not the entire file.

### Technical Questions

**Q: What's the maximum file size?**

A: Files are chunked for upload, so there's no practical size limit. Larger files take longer and cost more.

**Q: What's the scratchpad size limit?**

A: Scratchpads can store up to 4MB of data.

**Q: How do I backup my vault?**

A: Vaults are stored on the network. Use `ant vault load` to download vault metadata to any device with your wallet.

**Q: What's the difference between registers and scratchpads?**

A:
- **Registers**: Mutable key-value storage, versioned, good for small frequently-changing data
- **Scratchpads**: Up to 4MB mutable storage, shared via secret keys, good for collaborative editing

**Q: What are pointers and when should I use them?**

A: Pointers are named references that can point to other data on the network (graphs, scratchpads, other pointers, or chunks). Use pointers when you need an updatable reference - the pointer name stays the same but you can change what it points to. This is useful for:
- Configuration files that need updating
- Versioning systems (pointer always points to latest version)
- Creating stable endpoints that can redirect to different data
- Building data structures with mutable references

**Q: What's the difference between pointers, registers, and scratchpads?**

A:
- **Pointers**: Named references to other data addresses (can point to graphs, scratchpads, chunks, or other pointers). The pointer itself is cheap and updatable.
- **Registers**: Store actual data values, versioned, small size recommended
- **Scratchpads**: Store up to 4MB of actual data, encrypted, updatable

Think of pointers as "shortcuts" or "symbolic links" to other data, while registers and scratchpads store the actual data.

**Q: Can I see what's in my wallet without the password?**

A: No, encrypted wallets require the password to access. Use `--no-password` when creating if you don't want encryption (not recommended for production).

**Q: Why is `--no-verify` faster but risky?**

A: It skips verification that data was actually stored on the network. Use only when speed matters more than guarantees (not recommended for important data).

**Q: What happens if two people edit a scratchpad simultaneously?**

A: Each edit includes a counter. The network accepts the highest counter value. If both use the same counter, one edit may be rejected. Coordinate edits or implement conflict resolution in your application.

## Performance & Limits

### File Upload Performance

**Factors affecting upload speed:**
- File size (larger = longer)
- Number of chunks (based on file size)
- Network congestion
- Gas fees (higher fees = faster processing)
- Connection timeout settings

**Optimization tips:**
- Use `--retry-failed` for large files
- Set appropriate `--max-fee-per-gas` based on urgency
- Increase `--timeout` for very large files
- Upload during off-peak hours for lower gas fees

### Size Limits

| Data Type | Limit | Notes |
|-----------|-------|-------|
| Files | No practical limit | Uploaded in chunks |
| Scratchpads | 4 MB | Hard limit |
| Registers | Small values recommended | Designed for mutable pointers/metadata |
| Pointers | N/A | Store only references to other data addresses |
| Wallet | N/A | Standard EVM wallet |

### Concurrency

- Multiple CLI commands can run simultaneously
- Each command maintains its own network connection
- Vault sync operations should not be run concurrently

### Network Timeouts

Default timeout: 120 seconds

Recommended timeouts:
- Small files (<10 MB): 120s (default)
- Medium files (10-100 MB): 300s
- Large files (>100 MB): 600s+
- Slow connections: Increase as needed

## Security Best Practices

### Wallet Security

**Critical Guidelines:**

1. **Always encrypt your wallet in production**
   ```bash
   ant wallet create  # Will prompt for password
   # NOT: ant wallet create --no-password (only for testing)
   ```

2. **Store private keys securely**
   - Never commit private keys to version control
   - Use password managers or hardware security modules
   - Backup private keys in secure, offline storage
   - Consider using `--password` flag programmatically only in secure environments

3. **Use environment variables carefully**
   ```bash
   # DANGEROUS: Exposed in shell history
   ant wallet import 0x1234567890abcdef...

   # BETTER: Use from file or secure input
   read -s SECRET_KEY
   export SECRET_KEY
   ant file upload file.txt
   ```

### File Privacy

1. **Default is private** - files are encrypted by default
2. **Public files are permanent** - anyone with the address can access forever
3. **Network addresses are public** - treat file addresses like public URLs
4. **No deletion** - only upload what you're comfortable storing permanently

### Register, Scratchpad & Pointer Security

**Registers:**
- Anyone with the address can **read** the value
- Only the owner with the register key can **write**
- Don't store sensitive data in registers (they're publicly readable)

**Scratchpads:**
- Secret keys grant **both read and write** access
- Anyone with the secret key has **permanent** access
- Once shared, a key cannot be revoked
- Only share scratchpad keys with trusted parties
- Consider using time-limited or application-specific scratchpads

**Pointers:**
- Anyone with the pointer address can **read** what it points to
- Only the owner with the pointer key can **update** the target
- Pointers reveal the target address they're pointing to
- Secret keys grant **both read and write** access to update the pointer
- Once a pointer key is shared, recipients can permanently change the target
- Be cautious: updating a pointer affects everyone who uses that pointer

### Network Security

1. **Verify your peers** - only connect to trusted bootstrap peers
2. **Use official releases** - download binaries from official GitHub releases
3. **Check signatures** - verify binary integrity when available
4. **Audit logs** - review logs for unexpected behavior
5. **Network isolation** - use separate wallets for testing vs production

### Best Practices Summary

- ✅ Use encrypted wallets
- ✅ Backup private keys securely
- ✅ Understand public vs private data
- ✅ Verify uploads completed successfully
- ✅ Use `--retry-failed` for important data
- ✅ Consider pointer impact before updating (affects all users)
- ❌ Never commit private keys
- ❌ Never use `--no-password` in production
- ❌ Never share scratchpad/pointer keys publicly
- ❌ Never assume uploaded data can be deleted

## License
This Autonomi Network repository is licensed under the General Public License (GPL), version 3 ([LICENSE](http://www.gnu.org/licenses/gpl-3.0.en.html)).

## Contributing
Contributions are welcome! Please read the [CONTRIBUTING.md](https://github.com/maidsafe/autonomi/blob/main/CONTRIBUTING.md) file for guidelines on how to contribute to this project.
