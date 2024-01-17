# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.14.7](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.14.6...sn_transfers-v0.14.7) - 2023-10-26

### Fixed
- typos

## [0.14.6](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.14.5...sn_transfers-v0.14.6) - 2023-10-24

### Fixed
- *(tests)* nodes rewards tests to account for repayments amounts

## [0.14.5](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.14.4...sn_transfers-v0.14.5) - 2023-10-24

### Added
- *(payments)* adding unencrypted CashNotes for network royalties and verifying correct payment
- *(payments)* network royalties payment made when storing content

### Other
- *(api)* wallet APIs to account for network royalties fees when returning total cost paid for storage

## [0.14.4](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.14.3...sn_transfers-v0.14.4) - 2023-10-24

### Fixed
- *(networking)* only validate _our_ transfers at nodes

## [0.14.3](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.14.2...sn_transfers-v0.14.3) - 2023-10-18

### Other
- Revert "feat: keep transfers in mem instead of mem and i/o heavy cashnotes"

## [0.14.2](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.14.1...sn_transfers-v0.14.2) - 2023-10-18

### Added
- keep transfers in mem instead of mem and i/o heavy cashnotes

## [0.14.1](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.14.0...sn_transfers-v0.14.1) - 2023-10-17

### Fixed
- *(transfers)* dont overwrite existing payment transactions when we top up

### Other
- adding comments and cleanup around quorum / payment fixes

## [0.14.0](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.13.12...sn_transfers-v0.14.0) - 2023-10-12

### Added
- *(sn_transfers)* dont load Cns from disk, store value along w/ pubkey in wallet
- include protection for deposits

### Fixed
- remove uneeded hideous key Clone trait
- deadlock
- place lock on another file to prevent windows lock issue
- lock wallet file instead of dir
- wallet concurrent access bugs

### Other
- more detailed logging when client creating store cash_note

## [0.13.12](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.13.11...sn_transfers-v0.13.12) - 2023-10-11

### Fixed
- expose RecordMismatch errors and cleanup wallet if we hit that

### Other
- *(transfers)* add somre more clarity around DoubleSpendAttemptedForCashNotes
- *(docs)* cleanup comments and docs
- *(transfers)* remove pointless api

## [0.13.11](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.13.10...sn_transfers-v0.13.11) - 2023-10-10

### Added
- *(transfer)* special event for transfer notifs over gossipsub

## [0.13.10](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.13.9...sn_transfers-v0.13.10) - 2023-10-10

### Other
- *(sn_transfers)* improve transaction build mem perf

## [0.13.9](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.13.8...sn_transfers-v0.13.9) - 2023-10-06

### Added
- feat!(sn_transfers): unify store api for wallet

### Fixed
- readd api to load cash_notes from disk, update tests

### Other
- update comments around RecordNotFound
- remove deposit vs received cashnote disctinction

## [0.13.8](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.13.7...sn_transfers-v0.13.8) - 2023-10-06

### Other
- fix new clippy errors

## [0.13.7](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.13.6...sn_transfers-v0.13.7) - 2023-10-05

### Added
- *(metrics)* enable node monitoring through dockerized grafana instance

## [0.13.6](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.13.5...sn_transfers-v0.13.6) - 2023-10-05

### Fixed
- *(client)* remove concurrency limitations

## [0.13.5](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.13.4...sn_transfers-v0.13.5) - 2023-10-05

### Fixed
- *(sn_transfers)* be sure we store CashNotes before writing the wallet file

## [0.13.4](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.13.3...sn_transfers-v0.13.4) - 2023-10-05

### Added
- use progress bars on `files upload`

## [0.13.3](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.13.2...sn_transfers-v0.13.3) - 2023-10-04

### Added
- *(sn_transfers)* impl From for NanoTokens

### Fixed
- *(sn_transfers)* reuse payment overflow fix

### Other
- *(sn_transfers)* clippy and fmt
- *(sn_transfers)* add reuse cashnote cases
- separate method and write test

## [0.13.2](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.13.1...sn_transfers-v0.13.2) - 2023-10-02

### Added
- remove unused fee output

## [0.13.1](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.13.0...sn_transfers-v0.13.1) - 2023-09-28

### Added
- client to client transfers

## [0.13.0](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.12.2...sn_transfers-v0.13.0) - 2023-09-27

### Added
- deep clean sn_transfers, reduce exposition, remove dead code

### Fixed
- benches
- uncomment benches in Cargo.toml

### Other
- optimise bench
- improve cloning
- udeps

## [0.12.2](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.12.1...sn_transfers-v0.12.2) - 2023-09-25

### Other
- *(transfers)* unused variable removal

## [0.12.1](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.12.0...sn_transfers-v0.12.1) - 2023-09-25

### Other
- udeps
- cleanup renamings in sn_transfers
- remove mostly outdated mocks

## [0.12.0](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.11.15...sn_transfers-v0.12.0) - 2023-09-21

### Added
- rename utxo by CashNoteRedemption
- dusking DBCs

### Fixed
- udeps
- incompatible hardcoded value, add logs

### Other
- remove dbc dust comments
- rename Nano NanoTokens
- improve naming

## [0.11.15](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.11.14...sn_transfers-v0.11.15) - 2023-09-20

### Other
- major dep updates

## [0.11.14](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.11.13...sn_transfers-v0.11.14) - 2023-09-18

### Added
- serialisation for transfers for out of band sending
- generic transfer receipt

### Other
- add more docs
- add some docs

## [0.11.13](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.11.12...sn_transfers-v0.11.13) - 2023-09-15

### Other
- refine log levels

## [0.11.12](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.11.11...sn_transfers-v0.11.12) - 2023-09-14

### Other
- updated the following local packages: sn_protocol

## [0.11.11](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.11.10...sn_transfers-v0.11.11) - 2023-09-13

### Added
- *(register)* paying nodes for Register storage

## [0.11.10](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.11.9...sn_transfers-v0.11.10) - 2023-09-12

### Added
- add tx and parent spends verification
- chunk payments using UTXOs instead of DBCs

### Other
- use updated sn_dbc

## [0.11.9](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.11.8...sn_transfers-v0.11.9) - 2023-09-11

### Other
- *(release)* sn_cli-v0.81.29/sn_client-v0.88.16/sn_registers-v0.2.6/sn_node-v0.89.29/sn_testnet-v0.2.120/sn_protocol-v0.6.6

## [0.11.8](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.11.7...sn_transfers-v0.11.8) - 2023-09-08

### Added
- *(client)* repay for chunks if they cannot be validated

## [0.11.7](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.11.6...sn_transfers-v0.11.7) - 2023-09-05

### Other
- *(release)* sn_cli-v0.81.21/sn_client-v0.88.11/sn_registers-v0.2.5/sn_node-v0.89.21/sn_testnet-v0.2.112/sn_protocol-v0.6.5

## [0.11.6](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.11.5...sn_transfers-v0.11.6) - 2023-09-04

### Other
- updated the following local packages: sn_protocol

## [0.11.5](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.11.4...sn_transfers-v0.11.5) - 2023-09-04

### Other
- updated the following local packages: sn_protocol

## [0.11.4](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.11.3...sn_transfers-v0.11.4) - 2023-09-01

### Other
- *(transfers)* batch dbc storage
- *(transfers)* store dbcs by ref to avoid more clones
- *(transfers)* dont pass by value, this is a clone!
- *(client)* make unconfonfirmed txs btreeset, remove unnecessary cloning
- *(transfers)* improve update_local_wallet

## [0.11.3](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.11.2...sn_transfers-v0.11.3) - 2023-08-31

### Other
- remove unused async

## [0.11.2](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.11.1...sn_transfers-v0.11.2) - 2023-08-31

### Added
- *(node)* node to store rewards in a local wallet

### Fixed
- *(cli)* don't try to create wallet paths when checking balance

## [0.11.1](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.11.0...sn_transfers-v0.11.1) - 2023-08-31

### Other
- updated the following local packages: sn_protocol

## [0.11.0](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.28...sn_transfers-v0.11.0) - 2023-08-30

### Added
- one transfer per data set, mapped dbcs to content addrs
- [**breaking**] pay each chunk holder direct
- feat!(protocol): gets keys with GetStoreCost
- feat!(protocol): get price and pay for each chunk individually
- feat!(protocol): remove chunk merkletree to simplify payment

### Fixed
- *(tokio)* remove tokio fs

### Other
- *(deps)* bump tokio to 1.32.0
- *(client)* refactor client wallet to reduce dbc clones
- *(client)* pass around content payments map mut ref
- *(client)* error out early for invalid transfers

## [0.10.28](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.27...sn_transfers-v0.10.28) - 2023-08-24

### Other
- rust 1.72.0 fixes

## [0.10.27](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.26...sn_transfers-v0.10.27) - 2023-08-18

### Other
- updated the following local packages: sn_protocol

## [0.10.26](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.25...sn_transfers-v0.10.26) - 2023-08-11

### Added
- *(transfers)* add resend loop for unconfirmed txs

## [0.10.25](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.24...sn_transfers-v0.10.25) - 2023-08-10

### Other
- updated the following local packages: sn_protocol

## [0.10.24](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.23...sn_transfers-v0.10.24) - 2023-08-08

### Added
- *(transfers)* add get largest dbc for spending

### Fixed
- *(node)* prevent panic in storage calcs

### Other
- *(faucet)* provide more money
- tidy store cost code

## [0.10.23](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.22...sn_transfers-v0.10.23) - 2023-08-07

### Other
- rename network addresses confusing name method to xorname

## [0.10.22](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.21...sn_transfers-v0.10.22) - 2023-08-01

### Other
- *(networking)* use TOTAL_SUPPLY from sn_transfers

## [0.10.21](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.20...sn_transfers-v0.10.21) - 2023-08-01

### Other
- updated the following local packages: sn_protocol

## [0.10.20](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.19...sn_transfers-v0.10.20) - 2023-08-01

### Other
- *(release)* sn_cli-v0.80.17/sn_client-v0.87.0/sn_registers-v0.2.0/sn_node-v0.88.6/sn_testnet-v0.2.44/sn_protocol-v0.4.2

## [0.10.19](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.18...sn_transfers-v0.10.19) - 2023-07-31

### Fixed
- *(test)* using proper wallets during data_with_churn test

## [0.10.18](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.17...sn_transfers-v0.10.18) - 2023-07-28

### Other
- updated the following local packages: sn_protocol

## [0.10.17](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.16...sn_transfers-v0.10.17) - 2023-07-26

### Other
- updated the following local packages: sn_protocol

## [0.10.16](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.15...sn_transfers-v0.10.16) - 2023-07-25

### Other
- updated the following local packages: sn_protocol

## [0.10.15](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.14...sn_transfers-v0.10.15) - 2023-07-21

### Other
- updated the following local packages: sn_protocol

## [0.10.14](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.13...sn_transfers-v0.10.14) - 2023-07-20

### Other
- updated the following local packages: sn_protocol

## [0.10.13](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.12...sn_transfers-v0.10.13) - 2023-07-19

### Added
- *(CI)* dbc verfication during network churning test

## [0.10.12](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.11...sn_transfers-v0.10.12) - 2023-07-19

### Other
- updated the following local packages: sn_protocol

## [0.10.11](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.10...sn_transfers-v0.10.11) - 2023-07-18

### Other
- updated the following local packages: sn_protocol

## [0.10.10](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.9...sn_transfers-v0.10.10) - 2023-07-17

### Other
- updated the following local packages: sn_protocol

## [0.10.9](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.8...sn_transfers-v0.10.9) - 2023-07-17

### Added
- *(client)* keep storage payment proofs in local wallet

## [0.10.8](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.7...sn_transfers-v0.10.8) - 2023-07-12

### Other
- updated the following local packages: sn_protocol

## [0.10.7](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.6...sn_transfers-v0.10.7) - 2023-07-11

### Other
- updated the following local packages: sn_protocol

## [0.10.6](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.5...sn_transfers-v0.10.6) - 2023-07-10

### Other
- updated the following local packages: sn_protocol

## [0.10.5](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.4...sn_transfers-v0.10.5) - 2023-07-06

### Other
- updated the following local packages: sn_protocol

## [0.10.4](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.3...sn_transfers-v0.10.4) - 2023-07-05

### Other
- updated the following local packages: sn_protocol

## [0.10.3](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.2...sn_transfers-v0.10.3) - 2023-07-04

### Other
- updated the following local packages: sn_protocol

## [0.10.2](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.1...sn_transfers-v0.10.2) - 2023-06-28

### Other
- updated the following local packages: sn_protocol

## [0.10.1](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.10.0...sn_transfers-v0.10.1) - 2023-06-26

### Added
- display path when no deposits were found upon wallet deposit failure

### Other
- adding proptests for payment proofs merkletree utilities
- payment proof map to use xorname as index instead of merkletree nodes type
- having the payment proof validation util to return the item's leaf index

## [0.10.0](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.9.8...sn_transfers-v0.10.0) - 2023-06-22

### Added
- use standarised directories for files/wallet commands

## [0.9.8](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.9.7...sn_transfers-v0.9.8) - 2023-06-21

### Other
- updated the following local packages: sn_protocol

## [0.9.7](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.9.6...sn_transfers-v0.9.7) - 2023-06-21

### Fixed
- *(sn_transfers)* hardcode new genesis DBC for tests

### Other
- *(node)* obtain parent_tx from SignedSpend

## [0.9.6](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.9.5...sn_transfers-v0.9.6) - 2023-06-20

### Other
- updated the following local packages: sn_protocol

## [0.9.5](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.9.4...sn_transfers-v0.9.5) - 2023-06-20

### Other
- specific error types for different payment proof verification scenarios

## [0.9.4](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.9.3...sn_transfers-v0.9.4) - 2023-06-15

### Added
- add double spend test

### Fixed
- parent spend checks
- parent spend issue

## [0.9.3](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.9.2...sn_transfers-v0.9.3) - 2023-06-14

### Added
- include output DBC within payment proof for Chunks storage

## [0.9.2](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.9.1...sn_transfers-v0.9.2) - 2023-06-12

### Added
- remove spendbook rw locks, improve logging

## [0.9.1](https://github.com/maidsafe/safe_network/compare/sn_transfers-v0.9.0...sn_transfers-v0.9.1) - 2023-06-09

### Other
- manually change crate version
