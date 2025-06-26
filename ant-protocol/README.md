# ant_protocol

## Overview

The `ant_protocol` directory contains the core protocol logic for the Autonomi Network. It includes various modules that handle different aspects of the protocol, such as error handling, messages, and storage.

## Table of Contents

- [Overview](#overview)
- [Error Handling](#error-handling)
- [Messages](#messages)
  - [Cmd Messages](#cmd-messages)
  - [Query Messages](#query-messages)
  - [Response Messages](#response-messages)
- [Storage](#storage)
- [Protobuf Definitions](#protobuf-definitions)

## Error Handling

The `error.rs` file contains the definitions for various errors that can occur within the protocol.

### Error Types

- `ChunkNotFound(ChunkAddress)`: Indicates that a chunk was not found.
  - Example: `Result::Err(Error::ChunkNotFound(chunk_address))`
- `ChunkNotStored(XorName)`: Indicates that a chunk was not stored.
  - Example: `Result::Err(Error::ChunkNotStored(xor_name))`

## Messages

The `messages` module contains different types of messages that can be sent or received within the protocol.

### Cmd Messages

#### `Cmd::Replicate`

- **Description**: Write operation to notify peer fetch a list of `NetworkAddress` from the holder.
- **Parameters**:
  - `holder: NetworkAddress`: Holder of the replication keys.
  - `keys: Vec<NetworkAddress>`: Keys of the copy that shall be replicated.

### Query Messages

#### `Query::GetStoreCost`

- **Description**: Retrieve the cost of storing a record at the given address.
- **Parameters**:
  - `address: NetworkAddress`: The address where the record will be stored.

### Response Messages

#### `QueryResponse::GetStoreCost`

- **Description**: The store cost in nanos for storing the next record.
- **Parameters**:
  - `store_cost: Result<Token>`: The cost of storing the record.
  - `payment_address: PublicAddress`: The address to pay the store cost to.

#### `CmdResponse::Replicate`

- **Description**: Response to replication cmd.
- **Parameters**:
  - `Result<()>`: The result of the replication command.

## Storage

The `storage` module handles the storage aspects of the protocol.

### API Calls

- `ChunkAddress`: Address of a chunk in the network.
- `GraphEntryAddress`: Address of a Transaction in the network.
- `Header`: Header information for storage items.

## Protobuf Definitions

The `antnode_proto` directory contains the Protocol Buffers definitions for the Autonomi Network.

### Files

- `req_resp_types.proto`: Definitions for request and response types.
- `antnode.proto`: Main Protocol Buffers definitions for the Autonomi Network.
