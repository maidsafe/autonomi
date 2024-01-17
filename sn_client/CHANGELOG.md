# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.87.21](https://github.com/maidsafe/safe_network/compare/sn_client-v0.87.20...sn_client-v0.87.21) - 2023-08-18

### Other
- updated the following local packages: sn_protocol

## [0.87.20](https://github.com/maidsafe/safe_network/compare/sn_client-v0.87.19...sn_client-v0.87.20) - 2023-08-17

### Fixed
- *(client)* start bootstrap when we are connected to one peer

## [0.87.19](https://github.com/maidsafe/safe_network/compare/sn_client-v0.87.18...sn_client-v0.87.19) - 2023-08-17

### Other
- updated the following local packages: sn_networking

## [0.87.18](https://github.com/maidsafe/safe_network/compare/sn_client-v0.87.17...sn_client-v0.87.18) - 2023-08-17

### Fixed
- *(client)* use boostrap and fire Connecting event

## [0.87.17](https://github.com/maidsafe/safe_network/compare/sn_client-v0.87.16...sn_client-v0.87.17) - 2023-08-17

### Other
- updated the following local packages: sn_networking

## [0.87.16](https://github.com/maidsafe/safe_network/compare/sn_client-v0.87.15...sn_client-v0.87.16) - 2023-08-16

### Added
- *(client)* do not use cached proofs

## [0.87.15](https://github.com/maidsafe/safe_network/compare/sn_client-v0.87.14...sn_client-v0.87.15) - 2023-08-16

### Added
- overpay by default to allow margin

## [0.87.14](https://github.com/maidsafe/safe_network/compare/sn_client-v0.87.13...sn_client-v0.87.14) - 2023-08-15

### Other
- updated the following local packages: sn_networking

## [0.87.13](https://github.com/maidsafe/safe_network/compare/sn_client-v0.87.12...sn_client-v0.87.13) - 2023-08-11

### Added
- *(transfers)* add resend loop for unconfirmed txs
- *(networking)* ensure we always use the highest price we find
- *(networking)* enable returning less than majority for store_cost
- *(client)* use store cost queries to pre populate cost and RT

### Fixed
- *(client)* only_store_cost_if_higher missing else added

### Other
- remove client inactivity random storage query
- *(node)* resend unconfirmed txs before asserting
- *(cli)* print cost info
- *(networking)* remove logs, fix typos and clippy issues
- overpay in advance to avoid storage cost calculation inconsistent

## [0.87.12](https://github.com/maidsafe/safe_network/compare/sn_client-v0.87.11...sn_client-v0.87.12) - 2023-08-10

### Other
- updated the following local packages: sn_networking, sn_protocol

## [0.87.11](https://github.com/maidsafe/safe_network/compare/sn_client-v0.87.10...sn_client-v0.87.11) - 2023-08-10

### Other
- updated the following local packages: sn_networking

## [0.87.10](https://github.com/maidsafe/safe_network/compare/sn_client-v0.87.9...sn_client-v0.87.10) - 2023-08-08

### Added
- *(transfers)* add get largest dbc for spending

### Fixed
- *(node)* prevent panic in storage calcs

### Other
- tidy store cost code

## [0.87.9](https://github.com/maidsafe/safe_network/compare/sn_client-v0.87.8...sn_client-v0.87.9) - 2023-08-07

### Other
- updated the following local packages: sn_networking

## [0.87.8](https://github.com/maidsafe/safe_network/compare/sn_client-v0.87.7...sn_client-v0.87.8) - 2023-08-07

### Added
- rework register addresses to include pk

### Other
- rename network addresses confusing name method to xorname

## [0.87.7](https://github.com/maidsafe/safe_network/compare/sn_client-v0.87.6...sn_client-v0.87.7) - 2023-08-04

### Other
- updated the following local packages: sn_networking

## [0.87.6](https://github.com/maidsafe/safe_network/compare/sn_client-v0.87.5...sn_client-v0.87.6) - 2023-08-03

### Other
- updated the following local packages: sn_networking

## [0.87.5](https://github.com/maidsafe/safe_network/compare/sn_client-v0.87.4...sn_client-v0.87.5) - 2023-08-03

### Other
- updated the following local packages: sn_networking

## [0.87.4](https://github.com/maidsafe/safe_network/compare/sn_client-v0.87.3...sn_client-v0.87.4) - 2023-08-02

### Fixed
- do not create genesis when facuet already funded

## [0.87.3](https://github.com/maidsafe/safe_network/compare/sn_client-v0.87.2...sn_client-v0.87.3) - 2023-08-01

### Other
- *(client)* reattempt to get_spend_from_network
- add more verificaiton for payments

## [0.87.2](https://github.com/maidsafe/safe_network/compare/sn_client-v0.87.1...sn_client-v0.87.2) - 2023-08-01

### Other
- updated the following local packages: sn_protocol

## [0.87.1](https://github.com/maidsafe/safe_network/compare/sn_client-v0.87.0...sn_client-v0.87.1) - 2023-08-01

### Added
- *(cli)* add no-verify flag to cli

### Other
- fix double spend and remove arbitrary wait
- *(node)* verify faucet transactions before continuing
- *(netowrking)* change default re-attempt behaviour

## [0.87.0](https://github.com/maidsafe/safe_network/compare/sn_client-v0.86.11...sn_client-v0.87.0) - 2023-08-01

### Other
- *(register)* [**breaking**] hashing the node of a Register to sign it instead of bincode-serialising it

## [0.86.11](https://github.com/maidsafe/safe_network/compare/sn_client-v0.86.10...sn_client-v0.86.11) - 2023-07-31

### Other
- updated the following local packages: sn_networking

## [0.86.10](https://github.com/maidsafe/safe_network/compare/sn_client-v0.86.9...sn_client-v0.86.10) - 2023-07-31

### Added
- carry out get_record re-attempts for critical record
- for put_record verification, NotEnoughCopies is acceptable

### Fixed
- *(test)* using proper wallets during data_with_churn test

### Other
- move PrettyPrintRecordKey to sn_protocol
- small refactors for failing CI
- more tracable logs regarding chunk payment prove

## [0.86.9](https://github.com/maidsafe/safe_network/compare/sn_client-v0.86.8...sn_client-v0.86.9) - 2023-07-31

### Other
- updated the following local packages: sn_networking

## [0.86.8](https://github.com/maidsafe/safe_network/compare/sn_client-v0.86.7...sn_client-v0.86.8) - 2023-07-28

### Other
- updated the following local packages: sn_networking

## [0.86.7](https://github.com/maidsafe/safe_network/compare/sn_client-v0.86.6...sn_client-v0.86.7) - 2023-07-28

### Other
- updated the following local packages: sn_networking, sn_protocol

## [0.86.6](https://github.com/maidsafe/safe_network/compare/sn_client-v0.86.5...sn_client-v0.86.6) - 2023-07-28

### Other
- adapt all logging to use pretty record key

## [0.86.5](https://github.com/maidsafe/safe_network/compare/sn_client-v0.86.4...sn_client-v0.86.5) - 2023-07-27

### Other
- updated the following local packages: sn_networking

## [0.86.4](https://github.com/maidsafe/safe_network/compare/sn_client-v0.86.3...sn_client-v0.86.4) - 2023-07-26

### Fixed
- *(register)* Registers with same name but different tags were not being stored by the network

### Other
- centralising RecordKey creation logic to make sure we always use the same for all content type

## [0.86.3](https://github.com/maidsafe/safe_network/compare/sn_client-v0.86.2...sn_client-v0.86.3) - 2023-07-26

### Other
- updated the following local packages: sn_networking

## [0.86.2](https://github.com/maidsafe/safe_network/compare/sn_client-v0.86.1...sn_client-v0.86.2) - 2023-07-26

### Other
- updated the following local packages: sn_networking

## [0.86.1](https://github.com/maidsafe/safe_network/compare/sn_client-v0.86.0...sn_client-v0.86.1) - 2023-07-25

### Added
- *(replication)* replicate when our close group changes

### Fixed
- *(client)* keep an active `ClientEvent` receiver

### Other
- *(client)* get k_value from const fn

## [0.86.0](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.55...sn_client-v0.86.0) - 2023-07-21

### Added
- *(protocol)* [**breaking**] make Chunks storage payment required

### Other
- tokens transfers task in data_with_churn tests to use client apis instead of faucet helpers

## [0.85.55](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.54...sn_client-v0.85.55) - 2023-07-20

### Other
- cleanup error types

## [0.85.54](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.53...sn_client-v0.85.54) - 2023-07-19

### Added
- using kad::record for dbc spend ops
- *(CI)* dbc verfication during network churning test

## [0.85.53](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.52...sn_client-v0.85.53) - 2023-07-19

### Other
- updated the following local packages: sn_protocol

## [0.85.52](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.51...sn_client-v0.85.52) - 2023-07-18

### Other
- updated the following local packages: sn_networking

## [0.85.51](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.50...sn_client-v0.85.51) - 2023-07-18

### Added
- safer registers requiring signatures
- *(networking)* remove LostRecordEvent

### Fixed
- address PR comments
- client

## [0.85.50](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.49...sn_client-v0.85.50) - 2023-07-18

### Other
- updated the following local packages: sn_networking

## [0.85.49](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.48...sn_client-v0.85.49) - 2023-07-17

### Other
- updated the following local packages: sn_networking

## [0.85.48](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.47...sn_client-v0.85.48) - 2023-07-17

### Added
- *(networking)* upgrade to libp2p 0.52.0

### Other
- *(networking)* log all connected peer count

## [0.85.47](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.46...sn_client-v0.85.47) - 2023-07-17

### Added
- *(client)* keep storage payment proofs in local wallet

## [0.85.46](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.45...sn_client-v0.85.46) - 2023-07-12

### Other
- client to upload paid chunks in batches

## [0.85.45](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.44...sn_client-v0.85.45) - 2023-07-11

### Other
- updated the following local packages: sn_networking

## [0.85.44](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.43...sn_client-v0.85.44) - 2023-07-11

### Fixed
- *(client)* publish register on creation

## [0.85.43](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.42...sn_client-v0.85.43) - 2023-07-11

### Other
- updated the following local packages: sn_networking

## [0.85.42](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.41...sn_client-v0.85.42) - 2023-07-10

### Other
- updated the following local packages: sn_networking

## [0.85.41](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.40...sn_client-v0.85.41) - 2023-07-10

### Added
- client query register via get_record
- client upload Register via put_record

## [0.85.40](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.39...sn_client-v0.85.40) - 2023-07-06

### Other
- updated the following local packages: sn_networking

## [0.85.39](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.38...sn_client-v0.85.39) - 2023-07-06

### Added
- PutRecord response during client upload
- client upload chunk using kad::put_record

### Other
- *(release)* sn_cli-v0.79.0/sn_logging-v0.2.0/sn_node-v0.86.0/sn_testnet-v0.1.76/sn_networking-v0.3.11

## [0.85.38](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.37...sn_client-v0.85.38) - 2023-07-05

### Added
- carry out validation for record_store::put

## [0.85.37](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.36...sn_client-v0.85.37) - 2023-07-04

### Other
- demystify permissions

## [0.85.36](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.35...sn_client-v0.85.36) - 2023-07-03

### Added
- append SAFE_PEERS to initial_peers after restart

### Fixed
- *(text)* data_churn_test creates clients parsing SAFE_PEERS env

### Other
- reduce SAMPLE_SIZE for the data_with_churn test
- some client log tidy up

## [0.85.35](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.34...sn_client-v0.85.35) - 2023-06-29

### Other
- updated the following local packages: sn_networking

## [0.85.34](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.33...sn_client-v0.85.34) - 2023-06-28

### Other
- updated the following local packages: sn_networking

## [0.85.33](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.32...sn_client-v0.85.33) - 2023-06-28

### Added
- make the example work, fix sync when reg doesnt exist
- rework permissions, implement register cmd handlers
- register refactor, kad reg without cmds

### Fixed
- rename UserRights to UserPermissions

## [0.85.32](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.31...sn_client-v0.85.32) - 2023-06-28

### Other
- updated the following local packages: sn_networking

## [0.85.31](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.30...sn_client-v0.85.31) - 2023-06-28

### Added
- *(node)* dial without PeerId

## [0.85.30](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.29...sn_client-v0.85.30) - 2023-06-27

### Other
- updated the following local packages: sn_networking

## [0.85.29](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.28...sn_client-v0.85.29) - 2023-06-27

### Other
- updated the following local packages: sn_networking

## [0.85.28](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.27...sn_client-v0.85.28) - 2023-06-26

### Other
- updated the following local packages: sn_networking

## [0.85.27](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.26...sn_client-v0.85.27) - 2023-06-26

### Other
- updated the following local packages: sn_networking

## [0.85.26](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.25...sn_client-v0.85.26) - 2023-06-26

### Other
- *(release)* sn_cli-v0.78.9/sn_logging-v0.1.4/sn_node-v0.83.55/sn_testnet-v0.1.59/sn_networking-v0.1.24

## [0.85.25](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.24...sn_client-v0.85.25) - 2023-06-26

### Other
- payment proof map to use xorname as index instead of merkletree nodes type

## [0.85.24](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.23...sn_client-v0.85.24) - 2023-06-24

### Other
- updated the following local packages: sn_networking

## [0.85.23](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.22...sn_client-v0.85.23) - 2023-06-23

### Other
- updated the following local packages: sn_networking

## [0.85.22](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.21...sn_client-v0.85.22) - 2023-06-23

### Added
- forward chunk when not being the closest
- repliate to peers lost record

### Fixed
- client upload to peers closer to chunk

## [0.85.21](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.20...sn_client-v0.85.21) - 2023-06-23

### Other
- updated the following local packages: sn_networking

## [0.85.20](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.19...sn_client-v0.85.20) - 2023-06-22

### Other
- *(client)* initial refactor around uploads

## [0.85.19](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.18...sn_client-v0.85.19) - 2023-06-22

### Fixed
- improve client upload speed

## [0.85.18](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.17...sn_client-v0.85.18) - 2023-06-21

### Other
- updated the following local packages: sn_networking, sn_protocol

## [0.85.17](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.16...sn_client-v0.85.17) - 2023-06-21

### Other
- *(network)* remove `NetworkEvent::PutRecord` dead code

## [0.85.16](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.15...sn_client-v0.85.16) - 2023-06-21

### Other
- remove unused error variants
- *(node)* obtain parent_tx from SignedSpend
- *(release)* sn_cli-v0.77.46/sn_logging-v0.1.3/sn_node-v0.83.42/sn_testnet-v0.1.46/sn_networking-v0.1.15

## [0.85.15](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.14...sn_client-v0.85.15) - 2023-06-20

### Added
- *(network)* validate `Record` on GET
- *(network)* validate and store `ReplicatedData`
- *(node)* perform proper validations on PUT
- *(network)* validate and store `Record`

### Fixed
- *(node)* store parent tx along with `SignedSpend`

### Other
- *(docs)* add more docs and comments

## [0.85.14](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.13...sn_client-v0.85.14) - 2023-06-20

### Other
- updated the following local packages: sn_networking

## [0.85.13](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.12...sn_client-v0.85.13) - 2023-06-20

### Added
- pay 1 nano per Chunk as temporary approach till net-invoices are implemented
- committing storage payment SignedSpends to the network
- nodes to verify input DBCs of Chunk payment proof were spent

### Other
- specific error types for different payment proof verification scenarios
- include the Tx instead of output DBCs as part of storage payment proofs

## [0.85.12](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.11...sn_client-v0.85.12) - 2023-06-20

### Other
- updated the following local packages: sn_networking

## [0.85.11](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.10...sn_client-v0.85.11) - 2023-06-16

### Fixed
- reduce client mem usage during uploading

## [0.85.10](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.9...sn_client-v0.85.10) - 2023-06-15

### Added
- add double spend test

### Fixed
- parent spend issue

## [0.85.9](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.8...sn_client-v0.85.9) - 2023-06-14

### Added
- include output DBC within payment proof for Chunks storage

## [0.85.8](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.7...sn_client-v0.85.8) - 2023-06-14

### Other
- updated the following local packages: sn_networking

## [0.85.7](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.6...sn_client-v0.85.7) - 2023-06-14

### Added
- *(client)* expose req/resp timeout to client cli

## [0.85.6](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.5...sn_client-v0.85.6) - 2023-06-13

### Other
- *(release)* sn_cli-v0.77.12/sn_logging-v0.1.2/sn_node-v0.83.10/sn_testnet-v0.1.14/sn_networking-v0.1.6

## [0.85.5](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.4...sn_client-v0.85.5) - 2023-06-12

### Added
- remove spendbook rw locks, improve logging

### Other
- remove uneeded printlns
- *(release)* sn_cli-v0.77.10/sn_record_store-v0.1.3/sn_node-v0.83.8/sn_testnet-v0.1.12/sn_networking-v0.1.4

## [0.85.4](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.3...sn_client-v0.85.4) - 2023-06-09

### Other
- manually change crate version

## [0.85.3](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.2...sn_client-v0.85.3) - 2023-06-09

### Other
- more replication flow statistics during mem_check test

## [0.85.2](https://github.com/maidsafe/safe_network/compare/sn_client-v0.85.1...sn_client-v0.85.2) - 2023-06-07

### Added
- bail out if empty list of addreses is provided for payment proof generation
- *(client)* add progress indicator for initial network connections
- attach payment proof when uploading Chunks
- collect payment proofs and make sure merkletree always has pow-of-2 leaves
- node side payment proof validation from a given Chunk, audit trail, and reason-hash
- use all Chunks of a file to generate payment the payment proof tree
- Chunk storage payment and building payment proofs

### Fixed
- remove progress bar after it's finished.

### Other
- Revert "chore(release): sn_cli-v0.77.1/sn_client-v0.85.2/sn_networking-v0.1.2/sn_node-v0.83.1"
- *(release)* sn_cli-v0.77.1/sn_client-v0.85.2/sn_networking-v0.1.2/sn_node-v0.83.1
- Revert "chore(release): sn_cli-v0.77.1/sn_client-v0.85.2/sn_networking-v0.1.2/sn_protocol-v0.1.2/sn_node-v0.83.1/sn_record_store-v0.1.2/sn_registers-v0.1.2"
- *(release)* sn_cli-v0.77.1/sn_client-v0.85.2/sn_networking-v0.1.2/sn_protocol-v0.1.2/sn_node-v0.83.1/sn_record_store-v0.1.2/sn_registers-v0.1.2
- small log wording updates
- exposing definition of merkletree nodes data type and additional doc in code
- making Chunk payment proof optional for now
- moving all payment proofs utilities into sn_transfers crate

## [0.85.1](https://github.com/jacderida/safe_network/compare/sn_client-v0.85.0...sn_client-v0.85.1) - 2023-06-06

### Added
- refactor replication flow to using pull model
