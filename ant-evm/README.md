# Autonomi Network EVM data payments

This crate contains the logic for data payments on the Autonomi Network using the Ethereum protocol. 

This crate provides a set of types and utilities for interacting with EVM-based networks. It offers abstraction over common tasks such as handling addresses, wallets, payments, and network configurations. Below is an overview of the main types exposed by the crate.

## Exposed Types

### RewardsAddress
Alias for `evmlib::common::Address`. Represents an EVM-compatible address used for handling rewards.

### QuoteHash
Represents a unique hash identifying a quote. Useful for referencing and verifying `PaymentQuote`.

### TxHash
Represents the transaction hash. Useful for identifying and tracking transactions on the blockchain.

### EvmWallet
Alias for `evmlib::wallet::Wallet`. A wallet used to interact with EVM-compatible networks, providing key management and signing functionality.

### EvmNetworkCustom
A custom network type that allows for interaction with custom EVM-based networks. 

### EvmNetwork
A standard network type for EVM-based networks such as Ethereum or ArbitrumOne.

### PaymentQuote
Represents a quote for a payment transaction. Contains relevant data for processing payments through EVM-based networks.

### QuotingMetrics
Represents metrics associated with generating a payment quote. Useful for performance measurement and optimization.

### ProofOfPayment
Contains proof of a successful payment on an EVM-based network. Includes data like transaction hash and confirmation details.

### Amount
Represents a general amount of tokens. Can be used to define any token value in a flexible way.

### AttoTokens
Represents an amount in the smallest token unit, commonly "atto" (10^-18). Useful for working with precise amounts in smart contracts.

### EvmError
A custom error type used for handling EVM-related errors within the library.

### Result
A specialized `Result` type that wraps around `EvmError`. Standardizes error handling across operations.
