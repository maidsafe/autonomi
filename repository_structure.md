# Safe Network Repository Structure and Capabilities

## Core Components

### Client Side
1. **autonomi** - Main client implementation
   - Primary interface for users to interact with the Safe Network
   - Multiple language bindings support (Rust, Python, WASM)
   - Features:
     - Data operations (chunks, registers)
     - Vault operations
     - File system operations
     - EVM integration
   - Components:
     - `src/client/` - Core client implementation
     - `src/self_encryption.rs` - Data encryption handling
     - `src/python.rs` - Python language bindings
     - `src/utils.rs` - Utility functions
   - Build Features:
     - `data` - Basic data operations
     - `vault` - Vault operations (includes data and registers)
     - `registers` - Register operations
     - `fs` - File system operations
     - `local` - Local network testing
     - `external-signer` - External transaction signing
   - Testing:
     - `tests/` - Rust integration tests
     - `tests-js/` - JavaScript tests
     - `examples/` - Usage examples

2. **autonomi-cli** - Command-line interface
   - CLI tool for network interaction
   - Components:
     - `src/commands/` - CLI command implementations
     - `src/access/` - Network access management
     - `src/actions/` - Core action implementations
     - `src/wallet/` - Wallet management functionality
     - `src/commands.rs` - Command routing
     - `src/opt.rs` - Command-line options parsing
     - `src/utils.rs` - Utility functions
   - Features:
     - Network access management
     - Wallet operations
     - Data operations (chunks, registers)
     - Command-line parsing and routing

### Network Node Components
1. **sn_node** - Network Node Implementation
   - Core Components:
     - `src/node.rs` - Main node implementation
     - `src/put_validation.rs` - Data validation logic
     - `src/replication.rs` - Data replication handling
     - `src/metrics.rs` - Performance monitoring
     - `src/python.rs` - Python language bindings
   - Features:
     - Data validation and storage
     - Network message handling
     - Metrics collection
     - Error handling
     - Event processing
   - Binary Components:
     - `src/bin/` - Executable implementations

2. **sn_protocol** - Core Protocol Implementation
   - Components:
     - `src/messages/` - Network message definitions
     - `src/storage/` - Storage implementations
     - `src/safenode_proto/` - Protocol definitions
     - `src/node_rpc.rs` - RPC interface definitions
   - Features:
     - Message protocol definitions
     - Storage protocol
     - Node communication protocols
     - Version management

3. **sn_transfers** - Transfer System
   - Components:
     - `src/cashnotes/` - Digital cash implementation
     - `src/transfers/` - Transfer logic
     - `src/wallet/` - Wallet implementation
     - `src/genesis.rs` - Genesis block handling
   - Features:
     - Digital cash management
     - Transfer operations
     - Wallet operations
     - Genesis configuration
     - Error handling

### Data Types and Protocol
1. **sn_registers** - Register implementation
   - CRDT-based data structures
   - Conflict resolution mechanisms
   - Concurrent operations handling

### Network Management and Communication
1. **sn_networking** - Network Communication Layer
   - Core Components:
     - `src/cmd.rs` - Network command handling
     - `src/driver.rs` - Network driver implementation
     - `src/record_store.rs` - Data record management
     - `src/bootstrap.rs` - Network bootstrap process
     - `src/transport/` - Transport layer implementations
   - Features:
     - Network discovery and bootstrapping
     - External address handling
     - Relay management
     - Replication fetching
     - Record store management
     - Transfer handling
     - Metrics collection
   - Event System:
     - `src/event/` - Event handling implementation
     - Network event processing
     - Event-driven architecture

2. **sn_node_manager** - Node Management System
   - Core Components:
     - `src/cmd/` - Management commands
     - `src/add_services/` - Service management
     - `src/config.rs` - Configuration handling
     - `src/rpc.rs` - RPC interface
   - Features:
     - Node deployment and configuration
     - Service management
     - Local node handling
     - RPC client implementation
     - Error handling
   - Management Tools:
     - Binary implementations
     - Helper utilities
     - Configuration management

### Networking and Communication
1. **sn_networking** - Network communication
   - P2P networking implementation
   - Connection management
   - Message routing

2. **sn_peers_acquisition** - Peer discovery
   - Bootstrap mechanisms
   - Peer management
   - Network topology

### Infrastructure Components
1. **node-launchpad** - Node Deployment System
   - Core Components:
     - `src/app.rs` - Main application logic
     - `src/components/` - UI components
     - `src/node_mgmt.rs` - Node management
     - `src/node_stats.rs` - Statistics tracking
     - `src/config.rs` - Configuration handling
   - Features:
     - Node deployment and management
     - System monitoring
     - Configuration management
     - Terminal UI interface
     - Connection mode handling
   - UI Components:
     - Custom widgets
     - Styling system
     - Terminal UI implementation

2. **nat-detection** - Network Detection System
   - Core Components:
     - `src/behaviour/` - NAT behavior implementations
     - `src/main.rs` - Main detection logic
   - Features:
     - NAT type detection
     - Network connectivity testing
     - Behavior analysis
     - Connection management

### Payment and EVM Integration
1. **sn_evm** - EVM Integration System
   - Core Components:
     - `src/data_payments.rs` - Payment handling for data operations
     - `src/amount.rs` - Amount calculations and management
   - Features:
     - Data payment processing
     - Amount handling
     - Error management
     - Integration with EVM

2. **evmlib** - EVM Library
   - Core Components:
     - `src/contract/` - Smart contract handling
     - `src/wallet.rs` - Wallet implementation
     - `src/transaction.rs` - Transaction processing
     - `src/cryptography.rs` - Cryptographic operations
   - Features:
     - Smart contract management
     - Wallet operations
     - Transaction handling
     - External signer support
     - Test network support
     - Event handling
     - Utility functions

3. **evm_testnet** - EVM Test Environment
   - Features:
     - Test network setup
     - Development environment
     - Testing utilities

### Utilities and Support
1. **sn_logging** - Logging System
   - Core Components:
     - `src/appender.rs` - Log appender implementation
     - `src/layers.rs` - Logging layers
     - `src/metrics.rs` - Metrics integration
   - Features:
     - Structured logging
     - Custom appenders
     - Metrics integration
     - Error handling

2. **sn_metrics** - Metrics System
   - Features:
     - Performance monitoring
     - System metrics collection
     - Metrics reporting

3. **sn_build_info** - Build Information
   - Features:
     - Version management
     - Build configuration
     - Build information tracking

4. **test_utils** - Testing Utilities
   - Components:
     - `src/evm.rs` - EVM testing utilities
     - `src/testnet.rs` - Test network utilities
   - Features:
     - EVM test helpers
     - Test network setup
     - Common test functions

5. **sn_auditor** - Network Auditing
   - Features:
     - Network health monitoring
     - Security auditing
     - Performance tracking

## Development Tools
- **adr** - Architecture Decision Records
- **resources** - Additional resources and documentation
- **token_supplies** - Token management utilities

## Documentation
- **CHANGELOG.md** - Version history
- **CONTRIBUTING.md** - Contribution guidelines
- **README.md** - Project overview
- **prd.md** - Product Requirements Document

## Build and Configuration
- **Cargo.toml** - Main project configuration
- **Justfile** - Task automation
- **release-plz.toml** - Release configuration
- **reviewpad.yml** - Code review configuration

## Next Steps
1. Review and validate this structure
2. Identify any missing components or capabilities
3. Begin implementation of refactoring steps as outlined in refactoring_steps.md
4. Focus on client API refactoring as the first priority
