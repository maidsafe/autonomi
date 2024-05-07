# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.91.0-alpha.6](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.91.0-alpha.5...sn_cli-v0.91.0-alpha.6) - 2024-05-07

### Added
- *(client)* dump spends creation_reason statistics
- *(node)* make spend and cash_note reason field configurable
- *(cli)* readd wallet helper address for dist feat
- *(cli)* generate a mnemonic as wallet basis if no wallet found
- *(cli)* eip2333 helpers for accounts
- [**breaking**] renamings in CashNote
- [**breaking**] rename token to amount in Spend
- *(cli)* implement FilesUploadStatusNotifier trait for lib code
- *(cli)* return the files upload summary after a successful files upload
- unit testing dag, double spend poisoning tweaks
- report protocol mismatch error
- *(cli)* track spend creation reasons during audit
- *(client)* speed up register checks when paying
- double spend fork detection, fix invalid edges issue
- dag faults unit tests, sn_auditor offline mode
- *(faucet)* log from sn_client
- *(network)* add --upnp flag to node
- *(networking)* feature gate 'upnp'
- *(networking)* add UPnP behavior to open port
- *(relay)* remove autonat and enable hole punching manually
- *(relay)* remove old listen addr if we are using a relayed connection
- *(relay)* update the relay manager if the listen addr has been closed
- *(relay)* remove the dial flow
- *(relay)* impl RelayManager to perform circuit relay when behind NAT
- *(networking)* add in autonat server basics
- *(neetworking)* initial tcp use by default
- *(networking)* clear  record on valid put
- *(node)* restrict replication fetch range when node is full
- *(store)* load existing records in parallel
- *(node)* notify peer it is now considered as BAD
- *(node)* restore historic quoting metrics to allow restart
- *(networking)* shift to use ilog2 bucket distance for close data calcs
- spend shows the purposes of outputs created for
- *(transfers)* do not genereate wallet by default
- *(tui)* adding services
- *(network)* network contacts url should point to the correct network version

### Fixed
- *(cli)* acct_packet tests updated
- more test and cli fixes
- update calls to HotWallet::load
- *(client)* move acct_packet mnemonic into client layer
- *(client)* ensure we have a wallet or generate one via mnemonic
- create faucet via account load or generation
- *(client)* set uploader to use mnemonic wallet loader
- *(client)* calm down broadcast error logs if we've no listeners
- spend dag double spend links
- orphan test
- orphan parent bug, improve fault detection and logging
- *(networking)* allow wasm32 compilation
- *(network)* remove all external addresses related to a relay server
- *(relay_manager)* remove external addr on connection close
- relay server should not close connections made to a reserved peer
- short circuit identify if the peer is already present in the routitng table
- update outdated connection removal flow
- do not remove outdated connections
- increase relay server capacity
- keep idle connections forever
- pass peer id while crafting relay address
- *(relay)* crafted multi address should contain the P2PCircuit protocol
- do not add reported external addressese if we are behind home network
- *(networking)* do not add to dialed peers
- *(network)* do not strip out relay's PeerId
- *(relay)* craft the correctly formatted relay address
- *(network)* do not perform AutoNat for clients
- *(relay_manager)* do not dial with P2PCircuit protocol
- *(test)* quoting metrics might have live_time field changed along time
- *(node)* avoid false alert on FailedLocalRecord
- *(record_store)* prune only one record at a time
- *(node)* notify replication_fetcher of early completion
- *(node)* fetcher completes on_going_fetch entry on record_key only
- *(node)* not send out replication when failed read from local
- *(networking)* increase the local responsible range of nodes to K_VALUE peers away
- *(network)* clients should not perform farthest relevant record check
- *(node)* replication_fetch keep distance_range sync with record_store
- *(node)* replication_list in range filter
- transfer tests for HotWallet creation
- typo
- *(manager)* do not print to stdout on low verbosity level
- *(protocol)* evaluate NETWORK_VERSION_MODE at compile time

### Other
- *(versions)* sync versions with latest crates.io vs
- address review comments
- refactor CASH_NOTE_REASON strings to consts
- addres review comments
- *(cli)* update mnemonic wallet seed phrase wording
- *(CI)* upload faucet log during CI
- remove deprecated wallet deposit cmd
- fix typo for issue 1494
- *(cli)* make FilesUploadSummary public
- *(deps)* bump dependencies
- check DAG crawling performance
- store owner info inside node instead of network
- small cleanup of dead code
- improve naming and typo fix
- clarify client documentation
- clarify client::new description
- clarify client documentation
- clarify client::new description
- cargo fmt
- rename output reason to purpose for clarity
- *(network)* move event handling to its own module
- cleanup network events
- *(network)* remove nat detection via incoming connections check
- enable connection keepalive timeout
- remove non relayed listener id from relay manager
- enable multiple relay connections
- return early if peer is not a node
- *(tryout)* do not add new relay candidates
- add debug lines while adding potential relay candidates
- do not remove old non-relayed listeners
- clippy fix
- *(networking)* remove empty file
- *(networking)* re-add global_only
- use quic again
- log listner id
- *(relay)* add candidate even if we are dialing
- remove quic
- cleanup, add in relay server behaviour, and todo
- *(node)* lower some log levels to reduce log size
- *(node)* optimise record_store farthest record calculation
- *(node)* do not reset farthest_acceptance_distance
- *(node)* remove duplicated record_store fullness check
- *(networking)* notify network event on failed put due to prune
- *(networking)* ensure pruned data is indeed further away than kept
- *(CI)* confirm there is no failed replication fetch
- *(networking)* remove circular vec error
- *(node)* unit test for recover historic quoting metrics
- *(node)* pass entire QuotingMetrics into calculate_cost_for_records
- *(node)* extend distance range
- *(transfers)* reduce error size
- *(transfer)* unit tests for PaymentQuote
- *(release)* sn_auditor-v0.1.7/sn_client-v0.105.3/sn_networking-v0.14.4/sn_protocol-v0.16.3/sn_build_info-v0.1.7/sn_transfers-v0.17.2/sn_peers_acquisition-v0.2.10/sn_cli-v0.90.4/sn_faucet-v0.4.9/sn_metrics-v0.1.4/sn_node-v0.105.6/sn_service_management-v0.2.4/sn-node-manager-v0.7.4/sn_node_rpc_client-v0.6.8/token_supplies-v0.1.47
- *(release)* sn_auditor-v0.1.3-alpha.0/sn_client-v0.105.3-alpha.0/sn_networking-v0.14.2-alpha.0/sn_protocol-v0.16.2-alpha.0/sn_build_info-v0.1.7-alpha.0/sn_transfers-v0.17.2-alpha.0/sn_peers_acquisition-v0.2.9-alpha.0/sn_cli-v0.90.3-alpha.0/sn_node-v0.105.4-alpha.0/sn-node-manager-v0.7.3-alpha.0/sn_faucet-v0.4.4-alpha.0/sn_service_management-v0.2.2-alpha.0/sn_node_rpc_client-v0.6.4-alpha.0
- *(release)* sn_auditor-v0.1.7/sn_client-v0.105.3/sn_networking-v0.14.4/sn_protocol-v0.16.3/sn_build_info-v0.1.7/sn_transfers-v0.17.2/sn_peers_acquisition-v0.2.10/sn_cli-v0.90.4/sn_faucet-v0.4.9/sn_metrics-v0.1.4/sn_node-v0.105.6/sn_service_management-v0.2.4/sn-node-manager-v0.7.4/sn_node_rpc_client-v0.6.8/token_supplies-v0.1.47
- *(release)* sn_client-v0.105.3-alpha.5/sn_protocol-v0.16.3-alpha.2/sn_cli-v0.90.4-alpha.5/sn_node-v0.105.6-alpha.4/sn-node-manager-v0.7.4-alpha.1/sn_auditor-v0.1.7-alpha.0/sn_networking-v0.14.4-alpha.0/sn_peers_acquisition-v0.2.10-alpha.0/sn_faucet-v0.4.9-alpha.0/sn_service_management-v0.2.4-alpha.0/sn_node_rpc_client-v0.6.8-alpha.0
- *(release)* sn_client-v0.105.3-alpha.3/sn_protocol-v0.16.3-alpha.1/sn_peers_acquisition-v0.2.9-alpha.2/sn_cli-v0.90.4-alpha.3/sn_node-v0.105.6-alpha.1/sn_auditor-v0.1.5-alpha.0/sn_networking-v0.14.3-alpha.0/sn_faucet-v0.4.7-alpha.0/sn_service_management-v0.2.3-alpha.0/sn-node-manager-v0.7.4-alpha.0/sn_node_rpc_client-v0.6.6-alpha.0
- *(release)* sn_auditor-v0.1.3-alpha.1/sn_client-v0.105.3-alpha.1/sn_networking-v0.14.2-alpha.1/sn_peers_acquisition-v0.2.9-alpha.1/sn_cli-v0.90.4-alpha.1/sn_metrics-v0.1.4-alpha.0/sn_node-v0.105.5-alpha.1/sn_service_management-v0.2.2-alpha.1/sn-node-manager-v0.7.3-alpha.1/sn_node_rpc_client-v0.6.4-alpha.1/token_supplies-v0.1.47-alpha.0
- *(release)* sn_build_info-v0.1.7-alpha.1/sn_protocol-v0.16.3-alpha.0/sn_cli-v0.90.4-alpha.0/sn_faucet-v0.4.5-alpha.0/sn_node-v0.105.5-alpha.0

## [0.90.2](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.90.1...sn_cli-v0.90.2) - 2024-03-28

### Fixed
- *(cli)* read from cache during initial chunking process
- *(uploader)* do not error out on quote expiry during get store cost

## [0.90.1](https://github.com/joshuef/safe_network/compare/sn_cli-v0.90.0...sn_cli-v0.90.1) - 2024-03-28

### Added
- *(uploader)* error out if the quote has expired during get store_cost
- *(uploader)* use WalletApi to prevent loading client wallet during each operation
- *(transfers)* implement WalletApi to expose common methods

### Fixed
- *(uploader)* clarify the use of root and wallet dirs

### Other
- *(uploader)* update docs

## [0.90.0](https://github.com/joshuef/safe_network/compare/sn_cli-v0.89.85...sn_cli-v0.90.0) - 2024-03-27

### Added
- *(cli)* expose AccountPacket APIs from a lib so it can be used by other apps
- *(uploader)* collect all the uploaded registers
- *(uploader)* allow either chunk or chunk path to be used
- *(uploader)* register existence should be checked before going with payment flow
- *(client)* use the new Uploader insetead of FilesUpload
- make logging simpler to use
- [**breaking**] remove gossip code
- svg caching, fault tolerance during DAG collection
- *(uploader)* repay immediately if the quote has expired
- *(uploader)* use ClientRegister instead of Registers
- *(client)* implement a generic uploader with repay ability
- *(transfers)* enable client to check if a quote has expired
- *(client)* make publish register as an associated function
- *(network)* filter out peers when returning store cost
- *(transfers)* [**breaking**] support multiple payments for the same xorname
- use Arc inside Client, Network to reduce clone cost
- *(networking)* add NodeIssue for tracking bad node shunning
- *(faucet)* rate limit based upon wallet locks

### Fixed
- *(cli)* files should be chunked before checking if the chunks are empty
- *(test)* use tempfile lib instead of stdlib to create temp dirs
- *(clippy)* allow too many arguments as it is a private function
- *(uploader)* remove unused error tracking and allow retries for new payee
- *(uploader)* make the internals more clean
- *(uploader)* update force make payment logic
- *(register)* permissions verification was not being made by some Register APIs
- *(node)* fetching new data shall not cause timed_out immediately
- *(test)* generate unique temp dir to avoid read outdated data
- *(register)* shortcut permissions check when anyone can write to Register

### Other
- *(cli)* moving binary target related files onto src/bin dir
- *(uploader)* remove FilesApi dependency
- *(uploader)* implement UploaderInterface for easier testing
- rename of function to be more descriptive
- remove counter run through several functions and replace with simple counter
- *(register)* minor simplification in Register Permissions implementation
- *(uploader)* remove unused code path when store cost is 0
- *(uploader)* implement tests to test the basic pipeline logic
- *(uploader)* initial test setup for uploader
- *(uploader)* remove failed_to states
- *(node)* refactor pricing metrics
- lower some networking log levels
- *(node)* loose bad node detection criteria
- *(node)* optimization to reduce logging

## [0.89.85](https://github.com/joshuef/safe_network/compare/sn_cli-v0.89.84...sn_cli-v0.89.85) - 2024-03-21

### Added
- *(cli)* have CLI folders cmds to act on current directory by default
- *(folders)* folders APIs to accept an encryption key for metadata chunks
- *(log)* set log levels on the fly
- improve parallelisation with buffered streams
- refactor DAG, improve error management and security
- dag error recording
- *(protocol)* add rpc to set node log level on the fly

### Other
- *(cli)* adding automated test for metadata chunk encryption
- *(cli)* adding some high-level doc to acc-packet codebase
- *(node)* reduce bad_nodes check resource usage

## [0.89.84](https://github.com/joshuef/safe_network/compare/sn_cli-v0.89.83...sn_cli-v0.89.84) - 2024-03-18

### Other
- *(acc-packet)* adding test for acc-packet moved to a different location on disk
- *(acc-packet)* adding unit test for acc-packet changes scanning logic
- *(acc-packet)* adding unit test to private methods/helpers
- *(cli)* breaking up acc-packet logic within its own mod
- name change to spawn events handler
- increase of text length
- iterate upload code rearranged for clear readability

## [0.89.83](https://github.com/joshuef/safe_network/compare/sn_cli-v0.89.82...sn_cli-v0.89.83) - 2024-03-14

### Added
- self in import change
- moved param to outside calc
- refactor spend validation

### Fixed
- *(cli)* allow to upload chunks from acc-packet using chunked files local cache
- *(cli)* use chunk-mgr with iterator skipping tracking info files

### Other
- *(acc-packet)* adding verifications to compare tracking info generated on acc-packets cloned
- *(acc-packet)* adding verifications to compare the files/dirs stored on acc-packets cloned
- *(acc-packet)* testing sync empty root dirs
- *(acc-packet)* testing mutations syncing across clones of an acc-packet
- *(acc-packet)* adding automated tests to sn_cli::AccountPacket
- *(cli)* chunk-mgr to report files chunked/uploaded rather than bailing out
- improve code quality
- new `sn_service_management` crate

## [0.89.82-alpha.1](https://github.com/joshuef/safe_network/compare/sn_cli-v0.89.82-alpha.0...sn_cli-v0.89.82-alpha.1) - 2024-03-08

### Added
- reference checks
- reference checks
- builder added to estimate
- removal of unnecessary code in upload rs
- remove all use of client in iter uploader

### Other
- *(folders)* adding automated tests to sn_client::FoldersApi

## [0.89.81](https://github.com/joshuef/safe_network/compare/sn_cli-v0.89.80...sn_cli-v0.89.81) - 2024-03-06

### Added
- *(cli)* cmd to initialise a directory as root Folder for storing and syncing on/with network
- *(cli)* pull any Folders changes from network when syncing and merge them to local version
- make sn_cli use sn_clients reeports
- *(cli)* files download respects filename path
- *(folders)* make payments for local mutations detected before syncing
- *(folders)* build mutations report to be used by status and sync apis
- *(folders)* sync up logic and CLI cmd
- impl iterate uploader self to extract spawn theads
- impl iterate uploader self to extract spawn theads
- elevate files api and cm
- refactor upload with iter
- a more clear param for a message function
- split upload and upload with iter
- removal of some messages from vody body
- batch royalties redemption
- collect royalties through DAG
- *(folders)* avoid chunking files when retrieving them with Folders from the network
- *(folders)* store files data-map within Folders metadata chunk
- file to download
- *(folders)* regenerate tracking info when downloading Folders fm the network
- *(folders)* realise local changes made to folders/files
- *(folders)* keep track of local changes to Folders

### Fixed
- *(folders)* set correct change state to folders when scanning
- *(folders)* keep track of root folder sync status

### Other
- clean swarm commands errs and spend errors
- also add deps features in sn_client
- *(release)* sn_transfers-v0.16.1
- *(release)* sn_protocol-v0.15.0/sn-node-manager-v0.4.0
- *(cli)* removing some redundant logic from acc-packet codebase
- *(cli)* minor improvements to acc-packet codebase comments
- rename to iterative upload
- rename to iterative upload
- *(folders)* some simplifications to acc-packet codebase
- *(folders)* minor improvements to folders status report

## [0.89.80](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.79...sn_cli-v0.89.80) - 2024-02-23

### Added
- file to upload
- estimate refactor

## [0.89.79](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.78...sn_cli-v0.89.79) - 2024-02-21

### Other
- update Cargo.lock dependencies

## [0.89.78](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.77...sn_cli-v0.89.78) - 2024-02-20

### Other
- updated the following local packages: sn_protocol, sn_protocol

## [0.89.77](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.76...sn_cli-v0.89.77) - 2024-02-20

### Added
- dependency reconfiguration
- nano to snt
- concurrent estimate without error messages
- make data public bool
- removal of the retry strategy
- estimate feature with ci and balance after with fn docs

## [0.89.76](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.75...sn_cli-v0.89.76) - 2024-02-20

### Other
- *(release)* sn_networking-v0.13.26/sn-node-manager-v0.3.6/sn_client-v0.104.23/sn_node-v0.104.31

## [0.89.75](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.74...sn_cli-v0.89.75) - 2024-02-20

### Added
- spend and DAG utilities

## [0.89.74](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.73...sn_cli-v0.89.74) - 2024-02-20

### Added
- *(folders)* move folders/files metadata out of Folders entries

## [0.89.73](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.72...sn_cli-v0.89.73) - 2024-02-20

### Other
- updated the following local packages: sn_client, sn_registers

## [0.89.72](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.71...sn_cli-v0.89.72) - 2024-02-20

### Other
- *(release)* sn_networking-v0.13.23/sn_node-v0.104.26/sn_client-v0.104.18/sn_node_rpc_client-v0.4.57

## [0.89.71](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.70...sn_cli-v0.89.71) - 2024-02-19

### Other
- *(release)* sn_networking-v0.13.21/sn_client-v0.104.16/sn_node-v0.104.24

## [0.89.70](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.69...sn_cli-v0.89.70) - 2024-02-19

### Other
- *(cli)* allow to pass files iterator to chunk-mgr and files-upload tools

## [0.89.69](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.68...sn_cli-v0.89.69) - 2024-02-15

### Added
- *(client)* keep payee as part of storage payment cache

### Other
- *(release)* sn_networking-v0.13.19/sn_faucet-v0.3.67/sn_client-v0.104.14/sn_node-v0.104.22

## [0.89.68](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.67...sn_cli-v0.89.68) - 2024-02-15

### Other
- updated the following local packages: sn_protocol, sn_protocol

## [0.89.67](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.66...sn_cli-v0.89.67) - 2024-02-14

### Other
- updated the following local packages: sn_protocol, sn_protocol

## [0.89.66](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.65...sn_cli-v0.89.66) - 2024-02-14

### Other
- *(refactor)* move mod.rs files the modern way

## [0.89.65](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.64...sn_cli-v0.89.65) - 2024-02-13

### Other
- updated the following local packages: sn_protocol, sn_protocol

## [0.89.64](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.63...sn_cli-v0.89.64) - 2024-02-13

### Added
- identify orphans and inconsistencies in the DAG

### Fixed
- manage the genesis spend case

## [0.89.63](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.62...sn_cli-v0.89.63) - 2024-02-12

### Other
- *(release)* sn_networking-v0.13.12/sn_node-v0.104.12/sn-node-manager-v0.1.59/sn_client-v0.104.7/sn_node_rpc_client-v0.4.46

## [0.89.62](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.61...sn_cli-v0.89.62) - 2024-02-12

### Added
- *(cli)* single payment for all folders being synced
- *(cli)* adding Folders download CLI cmd
- *(client)* adding Folders sync API and CLI cmd

### Other
- *(cli)* improvements based on peer review
- *(cli)* adding simple example doc for using Folders cmd
- *(cli)* moving some Folder logic to a private helper function

## [0.89.61](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.60...sn_cli-v0.89.61) - 2024-02-12

### Other
- update Cargo.lock dependencies

## [0.89.60](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.59...sn_cli-v0.89.60) - 2024-02-09

### Other
- *(release)* sn_networking-v0.13.10/sn_client-v0.104.4/sn_node-v0.104.8

## [0.89.59](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.58...sn_cli-v0.89.59) - 2024-02-09

### Other
- update dependencies

## [0.89.58](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.57...sn_cli-v0.89.58) - 2024-02-08

### Other
- copyright update to current year

## [0.89.57](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.56...sn_cli-v0.89.57) - 2024-02-08

### Other
- update dependencies

## [0.89.56](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.55...sn_cli-v0.89.56) - 2024-02-08

### Added
- move the RetryStrategy into protocol and use that during cli upload/download

### Fixed
- *(bench)* update retry strategy args

### Other
- *(network)* rename re-attempts to retry strategy

## [0.89.55](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.54...sn_cli-v0.89.55) - 2024-02-08

### Other
- update dependencies

## [0.89.54](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.53...sn_cli-v0.89.54) - 2024-02-08

### Other
- update dependencies

## [0.89.53](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.52...sn_cli-v0.89.53) - 2024-02-08

### Other
- update dependencies

## [0.89.52](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.51...sn_cli-v0.89.52) - 2024-02-08

### Other
- update dependencies

## [0.89.51](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.50...sn_cli-v0.89.51) - 2024-02-07

### Other
- update dependencies

## [0.89.50](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.49...sn_cli-v0.89.50) - 2024-02-07

### Added
- extendable local state DAG in cli

## [0.89.49](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.48...sn_cli-v0.89.49) - 2024-02-06

### Other
- update dependencies

## [0.89.48](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.47...sn_cli-v0.89.48) - 2024-02-06

### Other
- update dependencies

## [0.89.47](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.46...sn_cli-v0.89.47) - 2024-02-06

### Other
- update dependencies

## [0.89.46](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.45...sn_cli-v0.89.46) - 2024-02-05

### Other
- update dependencies

## [0.89.45](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.44...sn_cli-v0.89.45) - 2024-02-05

### Other
- update dependencies

## [0.89.44](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.43...sn_cli-v0.89.44) - 2024-02-05

### Other
- update dependencies

## [0.89.43](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.42...sn_cli-v0.89.43) - 2024-02-05

### Other
- update dependencies

## [0.89.42](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.41...sn_cli-v0.89.42) - 2024-02-05

### Other
- update dependencies

## [0.89.41](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.40...sn_cli-v0.89.41) - 2024-02-05

### Other
- update dependencies

## [0.89.40](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.39...sn_cli-v0.89.40) - 2024-02-02

### Other
- update dependencies

## [0.89.39](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.38...sn_cli-v0.89.39) - 2024-02-02

### Other
- update dependencies

## [0.89.38](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.37...sn_cli-v0.89.38) - 2024-02-02

### Other
- update dependencies

## [0.89.37](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.36...sn_cli-v0.89.37) - 2024-02-01

### Other
- update dependencies

## [0.89.36](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.35...sn_cli-v0.89.36) - 2024-02-01

### Fixed
- *(cli)* move UploadedFiles creation logic from ChunkManager
- *(cli)* chunk manager to return error if fs operation fails

### Other
- *(cli)* use 'completed' files everywhere

## [0.89.35](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.34...sn_cli-v0.89.35) - 2024-02-01

### Other
- update dependencies

## [0.89.34](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.33...sn_cli-v0.89.34) - 2024-01-31

### Other
- update dependencies

## [0.89.33](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.32...sn_cli-v0.89.33) - 2024-01-31

### Other
- update dependencies

## [0.89.32](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.31...sn_cli-v0.89.32) - 2024-01-31

### Other
- update dependencies

## [0.89.31](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.30...sn_cli-v0.89.31) - 2024-01-30

### Other
- update dependencies

## [0.89.30](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.29...sn_cli-v0.89.30) - 2024-01-30

### Other
- update dependencies

## [0.89.29](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.28...sn_cli-v0.89.29) - 2024-01-30

### Other
- update dependencies

## [0.89.28](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.27...sn_cli-v0.89.28) - 2024-01-30

### Other
- update dependencies

## [0.89.27](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.26...sn_cli-v0.89.27) - 2024-01-30

### Other
- update dependencies

## [0.89.26](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.25...sn_cli-v0.89.26) - 2024-01-29

### Other
- update dependencies

## [0.89.25](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.24...sn_cli-v0.89.25) - 2024-01-29

### Other
- update dependencies

## [0.89.24](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.23...sn_cli-v0.89.24) - 2024-01-29

### Other
- update dependencies

## [0.89.23](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.22...sn_cli-v0.89.23) - 2024-01-29

### Other
- *(cli)* moving wallet mod into its own mod folder

## [0.89.22](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.21...sn_cli-v0.89.22) - 2024-01-29

### Other
- update dependencies

## [0.89.21](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.20...sn_cli-v0.89.21) - 2024-01-26

### Other
- update dependencies

## [0.89.20](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.19...sn_cli-v0.89.20) - 2024-01-25

### Other
- update dependencies

## [0.89.19](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.18...sn_cli-v0.89.19) - 2024-01-25

### Other
- update dependencies

## [0.89.18](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.17...sn_cli-v0.89.18) - 2024-01-25

### Other
- update dependencies

## [0.89.17](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.16...sn_cli-v0.89.17) - 2024-01-25

### Other
- update dependencies

## [0.89.16](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.15...sn_cli-v0.89.16) - 2024-01-25

### Other
- update dependencies

## [0.89.15](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.14...sn_cli-v0.89.15) - 2024-01-25

### Other
- update dependencies

## [0.89.14](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.13...sn_cli-v0.89.14) - 2024-01-24

### Other
- update dependencies

## [0.89.13](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.12...sn_cli-v0.89.13) - 2024-01-24

### Other
- update dependencies

## [0.89.12](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.11...sn_cli-v0.89.12) - 2024-01-24

### Other
- update dependencies

## [0.89.11](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.10...sn_cli-v0.89.11) - 2024-01-23

### Other
- update dependencies

## [0.89.10](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.9...sn_cli-v0.89.10) - 2024-01-23

### Other
- update dependencies

## [0.89.9](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.8...sn_cli-v0.89.9) - 2024-01-23

### Other
- *(release)* sn_protocol-v0.10.14/sn_networking-v0.12.35

## [0.89.8](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.7...sn_cli-v0.89.8) - 2024-01-22

### Other
- update dependencies

## [0.89.7](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.6...sn_cli-v0.89.7) - 2024-01-22

### Other
- update dependencies

## [0.89.6](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.5...sn_cli-v0.89.6) - 2024-01-21

### Other
- update dependencies

## [0.89.5](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.4...sn_cli-v0.89.5) - 2024-01-18

### Other
- update dependencies

## [0.89.4](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.3...sn_cli-v0.89.4) - 2024-01-18

### Other
- update dependencies

## [0.89.3](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.2...sn_cli-v0.89.3) - 2024-01-18

### Other
- update dependencies

## [0.89.2](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.1...sn_cli-v0.89.2) - 2024-01-18

### Other
- update dependencies

## [0.89.1](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.89.0...sn_cli-v0.89.1) - 2024-01-17

### Other
- update dependencies

## [0.89.0](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.22...sn_cli-v0.89.0) - 2024-01-17

### Other
- *(client)* [**breaking**] move out client connection progress bar

## [0.88.22](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.21...sn_cli-v0.88.22) - 2024-01-17

### Other
- update dependencies

## [0.88.21](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.20...sn_cli-v0.88.21) - 2024-01-16

### Other
- update dependencies

## [0.88.20](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.19...sn_cli-v0.88.20) - 2024-01-16

### Other
- update dependencies

## [0.88.19](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.18...sn_cli-v0.88.19) - 2024-01-16

### Other
- update dependencies

## [0.88.18](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.17...sn_cli-v0.88.18) - 2024-01-16

### Other
- update dependencies

## [0.88.17](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.16...sn_cli-v0.88.17) - 2024-01-15

### Other
- update dependencies

## [0.88.16](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.15...sn_cli-v0.88.16) - 2024-01-15

### Other
- update dependencies

## [0.88.15](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.14...sn_cli-v0.88.15) - 2024-01-15

### Other
- update dependencies

## [0.88.14](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.13...sn_cli-v0.88.14) - 2024-01-15

### Other
- update dependencies

## [0.88.13](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.12...sn_cli-v0.88.13) - 2024-01-12

### Other
- update dependencies

## [0.88.12](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.11...sn_cli-v0.88.12) - 2024-01-12

### Other
- update dependencies

## [0.88.11](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.10...sn_cli-v0.88.11) - 2024-01-11

### Other
- update dependencies

## [0.88.10](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.9...sn_cli-v0.88.10) - 2024-01-11

### Other
- update dependencies

## [0.88.9](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.8...sn_cli-v0.88.9) - 2024-01-11

### Other
- update dependencies

## [0.88.8](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.7...sn_cli-v0.88.8) - 2024-01-11

### Other
- update dependencies

## [0.88.7](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.6...sn_cli-v0.88.7) - 2024-01-10

### Added
- *(client)* client APIs and CLI cmd to broadcast a transaction signed offline
- *(cli)* new cmd to sign a transaction offline
- *(cli)* new wallet cmd to create a unsigned transaction to be used for offline signing

### Other
- *(transfers)* solving clippy issues about complex fn args

## [0.88.6](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.5...sn_cli-v0.88.6) - 2024-01-10

### Other
- update dependencies

## [0.88.5](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.4...sn_cli-v0.88.5) - 2024-01-10

### Added
- allow register CLI to create a public register writable to anyone

## [0.88.4](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.3...sn_cli-v0.88.4) - 2024-01-09

### Other
- update dependencies

## [0.88.3](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.2...sn_cli-v0.88.3) - 2024-01-09

### Other
- update dependencies

## [0.88.2](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.1...sn_cli-v0.88.2) - 2024-01-09

### Other
- update dependencies

## [0.88.1](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.88.0...sn_cli-v0.88.1) - 2024-01-09

### Added
- *(cli)* safe wallet create saves new key

## [0.88.0](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.87.0...sn_cli-v0.88.0) - 2024-01-08

### Added
- provide `--first` argument for `safenode`

## [0.87.0](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.103...sn_cli-v0.87.0) - 2024-01-08

### Added
- *(cli)* intergrate FilesDownload with cli

### Other
- *(client)* [**breaking**] refactor `Files` into `FilesUpload`

## [0.86.103](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.102...sn_cli-v0.86.103) - 2024-01-08

### Other
- update dependencies

## [0.86.102](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.101...sn_cli-v0.86.102) - 2024-01-08

### Other
- more doc updates to readme files

## [0.86.101](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.100...sn_cli-v0.86.101) - 2024-01-08

### Other
- update dependencies

## [0.86.100](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.99...sn_cli-v0.86.100) - 2024-01-08

### Other
- update dependencies

## [0.86.99](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.98...sn_cli-v0.86.99) - 2024-01-06

### Fixed
- *(cli)* read datamap when the xor addr of the file is provided

## [0.86.98](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.97...sn_cli-v0.86.98) - 2024-01-05

### Other
- update dependencies

## [0.86.97](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.96...sn_cli-v0.86.97) - 2024-01-05

### Other
- add clippy unwrap lint to workspace

## [0.86.96](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.95...sn_cli-v0.86.96) - 2024-01-05

### Other
- update dependencies

## [0.86.95](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.94...sn_cli-v0.86.95) - 2024-01-05

### Added
- *(cli)* store uploaded file metadata

## [0.86.94](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.93...sn_cli-v0.86.94) - 2024-01-05

### Other
- *(cli)* error if there is no file to upload

## [0.86.93](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.92...sn_cli-v0.86.93) - 2024-01-05

### Other
- update dependencies

## [0.86.92](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.91...sn_cli-v0.86.92) - 2024-01-04

### Other
- update dependencies

## [0.86.91](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.90...sn_cli-v0.86.91) - 2024-01-04

### Other
- *(cli)* print private data warning
- *(cli)* print the datamap's entire hex addr during first attempt

## [0.86.90](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.89...sn_cli-v0.86.90) - 2024-01-03

### Other
- *(cli)* print the datamap's entire hex addr

## [0.86.89](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.88...sn_cli-v0.86.89) - 2024-01-03

### Added
- *(cli)* keep downloaded files in a safe subdir
- *(client)* clients no longer upload data_map by default

### Fixed
- *(cli)* write datamap to metadata

### Other
- clippy test fixes and updates
- *(cli)* add not to non-public uploaded files
- refactor for clarity around head_chunk_address
- *(cli)* do not write datamap chunk if non-public

## [0.86.88](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.87...sn_cli-v0.86.88) - 2024-01-03

### Other
- update dependencies

## [0.86.87](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.86...sn_cli-v0.86.87) - 2024-01-02

### Other
- update dependencies

## [0.86.86](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.85...sn_cli-v0.86.86) - 2024-01-02

### Other
- update dependencies

## [0.86.85](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.84...sn_cli-v0.86.85) - 2023-12-29

### Other
- update dependencies

## [0.86.84](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.83...sn_cli-v0.86.84) - 2023-12-29

### Other
- update dependencies

## [0.86.83](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.82...sn_cli-v0.86.83) - 2023-12-29

### Other
- update dependencies

## [0.86.82](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.81...sn_cli-v0.86.82) - 2023-12-26

### Other
- update dependencies

## [0.86.81](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.80...sn_cli-v0.86.81) - 2023-12-22

### Other
- update dependencies

## [0.86.80](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.79...sn_cli-v0.86.80) - 2023-12-22

### Fixed
- printout un-verified files to alert user

## [0.86.79](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.78...sn_cli-v0.86.79) - 2023-12-21

### Other
- log full Register address when created in cli and example app

## [0.86.78](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.77...sn_cli-v0.86.78) - 2023-12-21

### Other
- *(client)* emit chunk Uploaded event if a chunk was verified during repayment

## [0.86.77](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.76...sn_cli-v0.86.77) - 2023-12-20

### Other
- reduce default batch size

## [0.86.76](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.75...sn_cli-v0.86.76) - 2023-12-19

### Added
- network royalties through audit POC

## [0.86.75](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.74...sn_cli-v0.86.75) - 2023-12-19

### Other
- update dependencies

## [0.86.74](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.73...sn_cli-v0.86.74) - 2023-12-19

### Other
- update dependencies

## [0.86.73](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.72...sn_cli-v0.86.73) - 2023-12-19

### Other
- update dependencies

## [0.86.72](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.71...sn_cli-v0.86.72) - 2023-12-19

### Fixed
- *(cli)* mark chunk completion as soon as we upload each chunk

## [0.86.71](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.70...sn_cli-v0.86.71) - 2023-12-18

### Other
- update dependencies

## [0.86.70](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.69...sn_cli-v0.86.70) - 2023-12-18

### Added
- *(cli)* random shuffle upload chunks to allow clients co-operation

## [0.86.69](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.68...sn_cli-v0.86.69) - 2023-12-18

### Other
- update dependencies

## [0.86.68](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.67...sn_cli-v0.86.68) - 2023-12-18

### Added
- *(client)* update the Files config via setters
- *(client)* track the upload stats inside Files
- *(client)* move upload retry logic from CLI to client

### Other
- *(client)* add docs to the Files struct
- *(cli)* use the new client Files api to upload chunks

## [0.86.67](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.66...sn_cli-v0.86.67) - 2023-12-14

### Other
- update dependencies

## [0.86.66](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.65...sn_cli-v0.86.66) - 2023-12-14

### Other
- update dependencies

## [0.86.65](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.64...sn_cli-v0.86.65) - 2023-12-14

### Other
- *(cli)* make upload summary printout clearer

## [0.86.64](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.63...sn_cli-v0.86.64) - 2023-12-14

### Other
- *(cli)* make sequential payment fail limit a const

## [0.86.63](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.62...sn_cli-v0.86.63) - 2023-12-14

### Other
- *(cli)* make wallet address easy to copy
- *(cli)* peer list is not printed to stdout

## [0.86.62](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.61...sn_cli-v0.86.62) - 2023-12-14

### Added
- *(cli)* cli arg for controlling chunk retries
- *(cli)* simple retry mechanism for remaining chunks

### Other
- prevent retries on ci runs w/ '-r 0'

## [0.86.61](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.60...sn_cli-v0.86.61) - 2023-12-13

### Other
- *(cli)* refactor upload_files

## [0.86.60](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.59...sn_cli-v0.86.60) - 2023-12-13

### Other
- update dependencies

## [0.86.59](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.58...sn_cli-v0.86.59) - 2023-12-13

### Added
- *(cli)* download path is familiar to users

## [0.86.58](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.57...sn_cli-v0.86.58) - 2023-12-13

### Added
- audit DAG collection and visualization
- cli double spends audit from genesis

## [0.86.57](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.56...sn_cli-v0.86.57) - 2023-12-12

### Other
- update dependencies

## [0.86.56](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.55...sn_cli-v0.86.56) - 2023-12-12

### Added
- *(cli)* skip payment and upload for existing chunks

## [0.86.55](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.54...sn_cli-v0.86.55) - 2023-12-12

### Other
- update dependencies

## [0.86.54](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.53...sn_cli-v0.86.54) - 2023-12-12

### Added
- constant uploading across batches

### Fixed
- *(cli)* remove chunk_manager clone that is unsafe

### Other
- *(networking)* add replication logs
- *(networking)* solidify REPLICATION_RANGE use. exclude self_peer_id in some calcs
- *(cli)* bail early on any payment errors
- *(cli)* only report uploaded files if no errors

## [0.86.53](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.52...sn_cli-v0.86.53) - 2023-12-12

### Other
- update dependencies

## [0.86.52](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.51...sn_cli-v0.86.52) - 2023-12-11

### Other
- update dependencies

## [0.86.51](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.50...sn_cli-v0.86.51) - 2023-12-11

### Other
- *(cli)* ux improvements after upload completes

## [0.86.50](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.49...sn_cli-v0.86.50) - 2023-12-08

### Other
- update dependencies

## [0.86.49](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.48...sn_cli-v0.86.49) - 2023-12-08

### Other
- update dependencies

## [0.86.48](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.47...sn_cli-v0.86.48) - 2023-12-08

### Other
- update dependencies

## [0.86.47](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.46...sn_cli-v0.86.47) - 2023-12-07

### Other
- update dependencies

## [0.86.46](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.45...sn_cli-v0.86.46) - 2023-12-06

### Other
- update dependencies

## [0.86.45](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.44...sn_cli-v0.86.45) - 2023-12-06

### Other
- update dependencies

## [0.86.44](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.43...sn_cli-v0.86.44) - 2023-12-06

### Added
- *(cli)* enable gossipsub for client when wallet cmd requires it
- *(wallet)* basic impl of a watch-only wallet API

### Other
- *(wallet)* major refactoring removing redundant and unused code
- *(cli)* Fix duplicate use of 'n' short flag
- *(cli)* All --name flags have short 'n' flag

## [0.86.43](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.42...sn_cli-v0.86.43) - 2023-12-06

### Other
- remove some needless cloning
- remove needless pass by value
- use inline format args
- add boilerplate for workspace lints

## [0.86.42](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.41...sn_cli-v0.86.42) - 2023-12-05

### Other
- update dependencies

## [0.86.41](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.40...sn_cli-v0.86.41) - 2023-12-05

### Other
- update dependencies

## [0.86.40](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.39...sn_cli-v0.86.40) - 2023-12-05

### Other
- update dependencies

## [0.86.39](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.38...sn_cli-v0.86.39) - 2023-12-05

### Other
- update dependencies

## [0.86.38](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.37...sn_cli-v0.86.38) - 2023-12-05

### Added
- allow for cli chunk put retries for un verifiable chunks

### Fixed
- mark chunks as completed when no failures on retry

## [0.86.37](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.36...sn_cli-v0.86.37) - 2023-12-05

### Other
- *(cli)* print the failed uploads stats
- *(cli)* remove unpaid/paid distinction from chunk manager

## [0.86.36](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.35...sn_cli-v0.86.36) - 2023-12-05

### Other
- *(networking)* remove triggered bootstrap slowdown

## [0.86.35](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.34...sn_cli-v0.86.35) - 2023-12-04

### Other
- update dependencies

## [0.86.34](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.33...sn_cli-v0.86.34) - 2023-12-01

### Other
- *(ci)* fix CI build cache parsing error

## [0.86.33](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.32...sn_cli-v0.86.33) - 2023-11-29

### Other
- update dependencies

## [0.86.32](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.31...sn_cli-v0.86.32) - 2023-11-29

### Added
- most of nodes not subscribe to royalty_transfer topic

## [0.86.31](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.30...sn_cli-v0.86.31) - 2023-11-29

### Other
- update dependencies

## [0.86.30](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.29...sn_cli-v0.86.30) - 2023-11-29

### Other
- update dependencies

## [0.86.29](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.28...sn_cli-v0.86.29) - 2023-11-29

### Other
- update dependencies

## [0.86.28](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.27...sn_cli-v0.86.28) - 2023-11-29

### Other
- update dependencies

## [0.86.27](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.26...sn_cli-v0.86.27) - 2023-11-29

### Added
- verify all the way to genesis
- verify spends through the cli

### Fixed
- genesis check security flaw

## [0.86.26](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.25...sn_cli-v0.86.26) - 2023-11-28

### Added
- *(cli)* serialise chunks metadata on disk with MsgPack instead of bincode
- *(royalties)* serialise royalties notifs with MsgPack instead of bincode

## [0.86.25](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.24...sn_cli-v0.86.25) - 2023-11-28

### Other
- update dependencies

## [0.86.24](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.23...sn_cli-v0.86.24) - 2023-11-28

### Other
- update dependencies

## [0.86.23](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.22...sn_cli-v0.86.23) - 2023-11-27

### Other
- update dependencies

## [0.86.22](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.21...sn_cli-v0.86.22) - 2023-11-24

### Added
- *(cli)* peers displayed as list

## [0.86.21](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.20...sn_cli-v0.86.21) - 2023-11-24

### Other
- update dependencies

## [0.86.20](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.19...sn_cli-v0.86.20) - 2023-11-23

### Added
- record put retry even when not verifying
- retry at the record level, remove all other retries, report errors

### Other
- appease clippy
- fix tests compilation

## [0.86.19](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.18...sn_cli-v0.86.19) - 2023-11-23

### Other
- update dependencies

## [0.86.18](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.17...sn_cli-v0.86.18) - 2023-11-23

### Other
- update dependencies

## [0.86.17](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.16...sn_cli-v0.86.17) - 2023-11-23

### Other
- update dependencies

## [0.86.16](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.15...sn_cli-v0.86.16) - 2023-11-22

### Other
- update dependencies

## [0.86.15](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.14...sn_cli-v0.86.15) - 2023-11-22

### Added
- *(cli)* add download batch-size option

## [0.86.14](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.13...sn_cli-v0.86.14) - 2023-11-22

### Other
- update dependencies

## [0.86.13](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.12...sn_cli-v0.86.13) - 2023-11-21

### Added
- make joining gossip for clients and rpc nodes optional

## [0.86.12](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.11...sn_cli-v0.86.12) - 2023-11-21

### Other
- update dependencies

## [0.86.11](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.10...sn_cli-v0.86.11) - 2023-11-20

### Other
- increase default batch size

## [0.86.10](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.9...sn_cli-v0.86.10) - 2023-11-20

### Other
- update dependencies

## [0.86.9](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.8...sn_cli-v0.86.9) - 2023-11-20

### Other
- update dependencies

## [0.86.8](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.7...sn_cli-v0.86.8) - 2023-11-20

### Other
- update dependencies

## [0.86.7](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.6...sn_cli-v0.86.7) - 2023-11-20

### Other
- update dependencies

## [0.86.6](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.5...sn_cli-v0.86.6) - 2023-11-20

### Fixed
- use actual quote instead of dummy

## [0.86.5](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.4...sn_cli-v0.86.5) - 2023-11-17

### Other
- update dependencies

## [0.86.4](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.3...sn_cli-v0.86.4) - 2023-11-17

### Other
- update dependencies

## [0.86.3](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.2...sn_cli-v0.86.3) - 2023-11-16

### Other
- update dependencies

## [0.86.2](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.1...sn_cli-v0.86.2) - 2023-11-16

### Added
- massive cleaning to prepare for quotes

## [0.86.1](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.86.0...sn_cli-v0.86.1) - 2023-11-15

### Other
- update dependencies

## [0.86.0](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.85.20...sn_cli-v0.86.0) - 2023-11-15

### Added
- *(client)* [**breaking**] error out if we cannot connect to the network in

### Other
- *(client)* [**breaking**] remove request_response timeout argument

## [0.85.20](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.85.19...sn_cli-v0.85.20) - 2023-11-15

### Added
- *(royalties)* make royalties payment to be 15% of the total storage cost
- *(protocol)* move test utils behind a feature gate

## [0.85.19](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.85.18...sn_cli-v0.85.19) - 2023-11-14

### Other
- *(royalties)* verify royalties fees amounts

## [0.85.18](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.85.17...sn_cli-v0.85.18) - 2023-11-14

### Other
- update dependencies

## [0.85.17](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.85.16...sn_cli-v0.85.17) - 2023-11-14

### Other
- update dependencies

## [0.85.16](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.85.15...sn_cli-v0.85.16) - 2023-11-14

### Fixed
- *(cli)* marking chunks as verified should mark them as paid too

## [0.85.15](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.85.14...sn_cli-v0.85.15) - 2023-11-14

### Fixed
- *(cli)* repay unpaid chunks due to transfer failures

## [0.85.14](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.85.13...sn_cli-v0.85.14) - 2023-11-13

### Fixed
- *(cli)* failed to move chunk path shall not get deleted

## [0.85.13](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.85.12...sn_cli-v0.85.13) - 2023-11-13

### Fixed
- avoid infinite looping on verification during upload

## [0.85.12](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.85.11...sn_cli-v0.85.12) - 2023-11-13

### Other
- update dependencies

## [0.85.11](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.85.10...sn_cli-v0.85.11) - 2023-11-13

### Other
- *(cli)* disable silent ignoring of wallet errors

## [0.85.10](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.85.9...sn_cli-v0.85.10) - 2023-11-10

### Added
- *(cli)* attempt to reload wallet from disk if storing it fails when receiving transfers online
- *(cli)* new cmd to listen to royalties payments and deposit them into a local wallet

### Other
- *(cli)* minor improvement to help docs

## [0.85.9](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.85.8...sn_cli-v0.85.9) - 2023-11-10

### Other
- update dependencies

## [0.85.8](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.85.7...sn_cli-v0.85.8) - 2023-11-09

### Other
- update dependencies

## [0.85.7](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.85.6...sn_cli-v0.85.7) - 2023-11-09

### Other
- update dependencies

## [0.85.6](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.85.5...sn_cli-v0.85.6) - 2023-11-09

### Added
- increase retry count for chunk put
- chunk put retry taking repayment into account

### Other
- const instead of magic num in code for wait time
- please ci

## [0.85.5](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.85.4...sn_cli-v0.85.5) - 2023-11-08

### Other
- update dependencies

## [0.85.4](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.85.3...sn_cli-v0.85.4) - 2023-11-08

### Fixed
- *(bench)* update benchmark to account for de duplicated files

## [0.85.3](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.85.2...sn_cli-v0.85.3) - 2023-11-08

### Other
- update dependencies

## [0.85.2](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.85.1...sn_cli-v0.85.2) - 2023-11-07

### Other
- update dependencies

## [0.85.1](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.85.0...sn_cli-v0.85.1) - 2023-11-07

### Other
- update dependencies

## [0.85.0](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.51...sn_cli-v0.85.0) - 2023-11-07

### Added
- *(cli)* store paid and upaid chunks separately
- *(cli)* use ChunkManager during the upload process
- *(cli)* implement ChunkManager to re-use already chunked files

### Fixed
- *(cli)* keep track of files that have been completely uploaded
- *(cli)* get bytes from OsStr by first converting it into lossy string
- *(client)* [**breaking**] make `Files::chunk_file` into an associated function
- *(upload)* don't ignore file if filename cannot be converted from OsString to String

### Other
- rename test function and spell correction
- *(cli)* add more tests to chunk manager for unpaid paid dir refactor
- *(cli)* add some docs to ChunkManager
- *(cli)* add tests for `ChunkManager`
- *(cli)* move chunk management to its own module

## [0.84.51](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.50...sn_cli-v0.84.51) - 2023-11-07

### Other
- update dependencies

## [0.84.50](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.49...sn_cli-v0.84.50) - 2023-11-07

### Other
- update dependencies

## [0.84.49](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.48...sn_cli-v0.84.49) - 2023-11-06

### Other
- update dependencies

## [0.84.48](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.47...sn_cli-v0.84.48) - 2023-11-06

### Other
- update dependencies

## [0.84.47](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.46...sn_cli-v0.84.47) - 2023-11-06

### Other
- update dependencies

## [0.84.46](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.45...sn_cli-v0.84.46) - 2023-11-06

### Other
- update dependencies

## [0.84.45](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.44...sn_cli-v0.84.45) - 2023-11-06

### Added
- *(deps)* upgrade libp2p to 0.53

## [0.84.44](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.43...sn_cli-v0.84.44) - 2023-11-03

### Other
- *(cli)* make file upload output cut n paste friendly

## [0.84.43](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.42...sn_cli-v0.84.43) - 2023-11-03

### Other
- update dependencies

## [0.84.42](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.41...sn_cli-v0.84.42) - 2023-11-02

### Other
- update dependencies

## [0.84.41](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.40...sn_cli-v0.84.41) - 2023-11-02

### Other
- update dependencies

## [0.84.40](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.39...sn_cli-v0.84.40) - 2023-11-01

### Other
- update dependencies

## [0.84.39](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.38...sn_cli-v0.84.39) - 2023-11-01

### Other
- update dependencies

## [0.84.38](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.37...sn_cli-v0.84.38) - 2023-11-01

### Other
- update dependencies

## [0.84.37](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.36...sn_cli-v0.84.37) - 2023-11-01

### Other
- update dependencies

## [0.84.36](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.35...sn_cli-v0.84.36) - 2023-11-01

### Other
- update dependencies

## [0.84.35](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.34...sn_cli-v0.84.35) - 2023-10-31

### Other
- update dependencies

## [0.84.34](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.33...sn_cli-v0.84.34) - 2023-10-31

### Other
- update dependencies

## [0.84.33](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.32...sn_cli-v0.84.33) - 2023-10-31

### Other
- update dependencies

## [0.84.32](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.31...sn_cli-v0.84.32) - 2023-10-30

### Other
- update dependencies

## [0.84.31](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.30...sn_cli-v0.84.31) - 2023-10-30

### Added
- *(cli)* error out if empty wallet
- *(cli)* error out if we do not have enough balance

## [0.84.30](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.29...sn_cli-v0.84.30) - 2023-10-30

### Other
- update dependencies

## [0.84.29](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.28...sn_cli-v0.84.29) - 2023-10-30

### Other
- *(node)* use Bytes for Gossip related data types
- *(release)* sn_client-v0.95.11/sn_protocol-v0.8.7/sn_transfers-v0.14.8/sn_networking-v0.9.10

## [0.84.28](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.27...sn_cli-v0.84.28) - 2023-10-27

### Other
- update dependencies

## [0.84.27](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.26...sn_cli-v0.84.27) - 2023-10-27

### Other
- update dependencies

## [0.84.26](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.25...sn_cli-v0.84.26) - 2023-10-27

### Added
- *(cli)* verify as we upload when 1 batch

## [0.84.25](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.24...sn_cli-v0.84.25) - 2023-10-26

### Other
- update dependencies

## [0.84.24](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.23...sn_cli-v0.84.24) - 2023-10-26

### Other
- update dependencies

## [0.84.23](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.22...sn_cli-v0.84.23) - 2023-10-26

### Other
- update dependencies

## [0.84.22](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.21...sn_cli-v0.84.22) - 2023-10-26

### Other
- update dependencies

## [0.84.21](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.20...sn_cli-v0.84.21) - 2023-10-26

### Other
- update dependencies

## [0.84.20](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.19...sn_cli-v0.84.20) - 2023-10-25

### Added
- *(cli)* chunk files in parallel

### Fixed
- *(cli)* remove Arc from ProgressBar as it is Arc internally

### Other
- *(cli)* add logs to indicate the time spent on chunking the files

## [0.84.19](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.18...sn_cli-v0.84.19) - 2023-10-24

### Added
- *(cli)* wallet deposit cmd with no arg was not reading cash notes from disk
- *(cli)* new wallet create cmd allowing users to create a wallet from a given secret key

## [0.84.18](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.17...sn_cli-v0.84.18) - 2023-10-24

### Other
- update dependencies

## [0.84.17](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.16...sn_cli-v0.84.17) - 2023-10-24

### Other
- update dependencies

## [0.84.16](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.15...sn_cli-v0.84.16) - 2023-10-24

### Other
- update dependencies

## [0.84.15](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.14...sn_cli-v0.84.15) - 2023-10-24

### Added
- *(log)* use LogBuilder to initialize logging

### Other
- *(client)* log and wait tweaks

## [0.84.14](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.13...sn_cli-v0.84.14) - 2023-10-24

### Other
- update dependencies

## [0.84.13](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.12...sn_cli-v0.84.13) - 2023-10-23

### Other
- update dependencies

## [0.84.12](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.11...sn_cli-v0.84.12) - 2023-10-23

### Other
- update dependencies

## [0.84.11](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.10...sn_cli-v0.84.11) - 2023-10-23

### Fixed
- *(cli)* don't bail if a payment was not found during verify/repayment

## [0.84.10](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.9...sn_cli-v0.84.10) - 2023-10-23

### Other
- more custom debug and debug skips

## [0.84.9](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.8...sn_cli-v0.84.9) - 2023-10-23

### Other
- update dependencies

## [0.84.8](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.7...sn_cli-v0.84.8) - 2023-10-22

### Other
- update dependencies

## [0.84.7](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.6...sn_cli-v0.84.7) - 2023-10-21

### Other
- update dependencies

## [0.84.6](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.5...sn_cli-v0.84.6) - 2023-10-20

### Other
- update dependencies

## [0.84.5](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.4...sn_cli-v0.84.5) - 2023-10-20

### Other
- update dependencies

## [0.84.4](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.3...sn_cli-v0.84.4) - 2023-10-19

### Other
- update dependencies

## [0.84.3](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.2...sn_cli-v0.84.3) - 2023-10-19

### Other
- update dependencies

## [0.84.2](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.1...sn_cli-v0.84.2) - 2023-10-19

### Other
- update dependencies

## [0.84.1](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.84.0...sn_cli-v0.84.1) - 2023-10-18

### Other
- update dependencies

## [0.84.0](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.52...sn_cli-v0.84.0) - 2023-10-18

### Added
- *(client)* verify register uploads and retry and repay if failed

### Other
- *(client)* always validate storage payments
- repay for data in node rewards tests

## [0.83.52](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.51...sn_cli-v0.83.52) - 2023-10-18

### Other
- update dependencies

## [0.83.51](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.50...sn_cli-v0.83.51) - 2023-10-17

### Other
- update dependencies

## [0.83.50](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.49...sn_cli-v0.83.50) - 2023-10-16

### Other
- update dependencies

## [0.83.49](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.48...sn_cli-v0.83.49) - 2023-10-16

### Other
- update dependencies

## [0.83.48](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.47...sn_cli-v0.83.48) - 2023-10-13

### Other
- update dependencies

## [0.83.47](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.46...sn_cli-v0.83.47) - 2023-10-13

### Other
- update dependencies

## [0.83.46](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.45...sn_cli-v0.83.46) - 2023-10-12

### Other
- update dependencies

## [0.83.45](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.44...sn_cli-v0.83.45) - 2023-10-12

### Other
- update dependencies

## [0.83.44](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.43...sn_cli-v0.83.44) - 2023-10-12

### Other
- update dependencies

## [0.83.43](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.42...sn_cli-v0.83.43) - 2023-10-11

### Fixed
- expose RecordMismatch errors and cleanup wallet if we hit that

### Other
- *(docs)* cleanup comments and docs

## [0.83.42](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.41...sn_cli-v0.83.42) - 2023-10-11

### Other
- update dependencies

## [0.83.41](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.40...sn_cli-v0.83.41) - 2023-10-11

### Fixed
- make client handle payment error

## [0.83.40](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.39...sn_cli-v0.83.40) - 2023-10-11

### Added
- showing expected holders to CLI when required

## [0.83.39](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.38...sn_cli-v0.83.39) - 2023-10-11

### Other
- update dependencies

## [0.83.38](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.37...sn_cli-v0.83.38) - 2023-10-10

### Added
- *(transfer)* special event for transfer notifs over gossipsub

## [0.83.37](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.36...sn_cli-v0.83.37) - 2023-10-10

### Other
- update dependencies

## [0.83.36](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.35...sn_cli-v0.83.36) - 2023-10-10

### Other
- update dependencies

## [0.83.35](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.34...sn_cli-v0.83.35) - 2023-10-10

### Other
- update dependencies

## [0.83.34](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.33...sn_cli-v0.83.34) - 2023-10-09

### Other
- update dependencies

## [0.83.33](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.32...sn_cli-v0.83.33) - 2023-10-09

### Added
- ensure temp SE chunks got cleaned after uploading

## [0.83.32](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.31...sn_cli-v0.83.32) - 2023-10-08

### Other
- update dependencies

## [0.83.31](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.30...sn_cli-v0.83.31) - 2023-10-06

### Added
- feat!(sn_transfers): unify store api for wallet

### Other
- remove deposit vs received cashnote disctinction

## [0.83.30](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.29...sn_cli-v0.83.30) - 2023-10-06

### Other
- *(cli)* reuse the client::send function to send amount from wallet

## [0.83.29](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.28...sn_cli-v0.83.29) - 2023-10-06

### Other
- update dependencies

## [0.83.28](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.27...sn_cli-v0.83.28) - 2023-10-06

### Other
- update dependencies

## [0.83.27](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.26...sn_cli-v0.83.27) - 2023-10-05

### Added
- *(metrics)* enable node monitoring through dockerized grafana instance

## [0.83.26](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.25...sn_cli-v0.83.26) - 2023-10-05

### Added
- feat!(cli): remove concurrency argument

### Fixed
- *(client)* remove concurrency limitations

## [0.83.25](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.24...sn_cli-v0.83.25) - 2023-10-05

### Fixed
- *(sn_transfers)* be sure we store CashNotes before writing the wallet file

## [0.83.24](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.23...sn_cli-v0.83.24) - 2023-10-05

### Fixed
- use specific verify func for chunk stored verification

## [0.83.23](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.22...sn_cli-v0.83.23) - 2023-10-05

### Added
- use progress bars on `files upload`

### Other
- use one files api and clarify variable names
- pay_for_chunks returns cost and new balance

## [0.83.22](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.21...sn_cli-v0.83.22) - 2023-10-04

### Other
- update dependencies

## [0.83.21](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.20...sn_cli-v0.83.21) - 2023-10-04

### Other
- update dependencies

## [0.83.20](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.19...sn_cli-v0.83.20) - 2023-10-04

### Added
- *(client)* log the command invoked for safe

## [0.83.19](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.18...sn_cli-v0.83.19) - 2023-10-04

### Other
- update dependencies

## [0.83.18](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.17...sn_cli-v0.83.18) - 2023-10-04

### Other
- update dependencies

## [0.83.17](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.16...sn_cli-v0.83.17) - 2023-10-03

### Other
- update dependencies

## [0.83.16](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.15...sn_cli-v0.83.16) - 2023-10-03

### Other
- update dependencies

## [0.83.15](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.14...sn_cli-v0.83.15) - 2023-10-03

### Other
- update dependencies

## [0.83.14](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.13...sn_cli-v0.83.14) - 2023-10-03

### Other
- update dependencies

## [0.83.13](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.12...sn_cli-v0.83.13) - 2023-10-03

### Other
- update dependencies

## [0.83.12](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.11...sn_cli-v0.83.12) - 2023-10-02

### Other
- update dependencies

## [0.83.11](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.10...sn_cli-v0.83.11) - 2023-10-02

### Added
- add read transfer from file option
- faucet using transfers instead of sending raw cashnotes

### Other
- trim transfer hex nl and spaces
- add some more error info printing

## [0.83.10](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.9...sn_cli-v0.83.10) - 2023-10-02

### Other
- update dependencies

## [0.83.9](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.8...sn_cli-v0.83.9) - 2023-10-02

### Added
- *(client)* show feedback on long wait for costs

## [0.83.8](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.7...sn_cli-v0.83.8) - 2023-10-02

### Other
- update dependencies

## [0.83.7](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.6...sn_cli-v0.83.7) - 2023-09-29

### Other
- update dependencies

## [0.83.6](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.5...sn_cli-v0.83.6) - 2023-09-29

### Fixed
- *(cli)* dont bail on errors during repay/upload

## [0.83.5](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.4...sn_cli-v0.83.5) - 2023-09-29

### Fixed
- *(client)* just skip empty files

## [0.83.4](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.3...sn_cli-v0.83.4) - 2023-09-28

### Added
- client to client transfers

## [0.83.3](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.2...sn_cli-v0.83.3) - 2023-09-27

### Added
- *(networking)* remove optional_semaphore being passed down from apps

## [0.83.2](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.1...sn_cli-v0.83.2) - 2023-09-27

### Other
- update dependencies

## [0.83.1](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.83.0...sn_cli-v0.83.1) - 2023-09-27

### Added
- *(logging)* set default log levels to be more verbose
- *(logging)* set default logging to data-dir

### Other
- *(client)* add timestamp to client log path

## [0.83.0](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.82.8...sn_cli-v0.83.0) - 2023-09-27

### Added
- deep clean sn_transfers, reduce exposition, remove dead code

## [0.82.8](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.82.7...sn_cli-v0.82.8) - 2023-09-26

### Other
- update dependencies

## [0.82.7](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.82.6...sn_cli-v0.82.7) - 2023-09-26

### Added
- *(apis)* adding client and node APIs, as well as safenode RPC service to unsubscribe from gossipsub topics

## [0.82.6](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.82.5...sn_cli-v0.82.6) - 2023-09-25

### Other
- update dependencies

## [0.82.5](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.82.4...sn_cli-v0.82.5) - 2023-09-25

### Other
- update dependencies

## [0.82.4](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.82.3...sn_cli-v0.82.4) - 2023-09-25

### Added
- *(cli)* wrap repayment error for clarity

## [0.82.3](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.82.2...sn_cli-v0.82.3) - 2023-09-25

### Added
- *(peers)* use a common way to bootstrap into the network for all the bins
- *(cli)* fetch network contacts for the provided network name
- *(cli)* fetch bootstrap peers from network contacts

### Other
- more logs around parsing network-contacts
- *(cli)* feature gate network contacts and fetch from URL

## [0.82.2](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.82.1...sn_cli-v0.82.2) - 2023-09-25

### Other
- update dependencies

## [0.82.1](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.82.0...sn_cli-v0.82.1) - 2023-09-22

### Added
- *(apis)* adding client and node APIs, as well as safenode RPC services to pub/sub to gossipsub topics

## [0.82.0](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.64...sn_cli-v0.82.0) - 2023-09-22

### Added
- *(cli)* deps update and arbitrary change for cli

## [0.81.64](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.63...sn_cli-v0.81.64) - 2023-09-21

### Added
- provide a `files ls` command

### Other
- *(release)* sn_client-v0.89.22
- store uploaded files list as text
- clarify `files download` usage
- output address of uploaded file

## [0.81.63](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.62...sn_cli-v0.81.63) - 2023-09-20

### Other
- update dependencies

## [0.81.62](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.61...sn_cli-v0.81.62) - 2023-09-20

### Other
- update dependencies

## [0.81.61](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.60...sn_cli-v0.81.61) - 2023-09-20

### Other
- update dependencies

## [0.81.60](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.59...sn_cli-v0.81.60) - 2023-09-20

### Other
- update dependencies

## [0.81.59](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.58...sn_cli-v0.81.59) - 2023-09-20

### Other
- update dependencies

## [0.81.58](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.57...sn_cli-v0.81.58) - 2023-09-20

### Other
- update dependencies

## [0.81.57](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.56...sn_cli-v0.81.57) - 2023-09-20

### Other
- update dependencies

## [0.81.56](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.55...sn_cli-v0.81.56) - 2023-09-20

### Other
- update dependencies

## [0.81.55](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.54...sn_cli-v0.81.55) - 2023-09-20

### Fixed
- make clearer cli send asks for whole token amounts, not nanos

## [0.81.54](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.53...sn_cli-v0.81.54) - 2023-09-20

### Other
- update dependencies

## [0.81.53](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.52...sn_cli-v0.81.53) - 2023-09-20

### Other
- update dependencies

## [0.81.52](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.51...sn_cli-v0.81.52) - 2023-09-19

### Other
- update dependencies

## [0.81.51](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.50...sn_cli-v0.81.51) - 2023-09-19

### Other
- update dependencies

## [0.81.50](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.49...sn_cli-v0.81.50) - 2023-09-19

### Other
- error handling when failed fetch store cost

## [0.81.49](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.48...sn_cli-v0.81.49) - 2023-09-19

### Other
- update dependencies

## [0.81.48](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.47...sn_cli-v0.81.48) - 2023-09-19

### Other
- update dependencies

## [0.81.47](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.46...sn_cli-v0.81.47) - 2023-09-19

### Other
- update dependencies

## [0.81.46](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.45...sn_cli-v0.81.46) - 2023-09-18

### Fixed
- avoid verification too close to put; remove un-necessary wait for put

## [0.81.45](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.44...sn_cli-v0.81.45) - 2023-09-18

### Other
- some cleanups within the upload procedure

## [0.81.44](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.43...sn_cli-v0.81.44) - 2023-09-18

### Other
- update dependencies

## [0.81.43](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.42...sn_cli-v0.81.43) - 2023-09-18

### Fixed
- *(cli)* repay and upload after verifying all the chunks

### Other
- *(cli)* use iter::chunks() API to batch and pay for our chunks

## [0.81.42](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.41...sn_cli-v0.81.42) - 2023-09-15

### Added
- *(client)* pay for chunks in batches

### Other
- *(cli)* move 'chunk_path' to files.rs
- *(client)* refactor chunk upload code to allow greater concurrency

## [0.81.41](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.40...sn_cli-v0.81.41) - 2023-09-15

### Other
- update dependencies

## [0.81.40](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.39...sn_cli-v0.81.40) - 2023-09-15

### Other
- update dependencies

## [0.81.39](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.38...sn_cli-v0.81.39) - 2023-09-15

### Other
- update dependencies

## [0.81.38](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.37...sn_cli-v0.81.38) - 2023-09-14

### Other
- update dependencies

## [0.81.37](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.36...sn_cli-v0.81.37) - 2023-09-14

### Added
- expose batch_size to cli
- split upload procedure into batches

## [0.81.36](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.35...sn_cli-v0.81.36) - 2023-09-14

### Other
- *(metrics)* rename feature flag and small fixes

## [0.81.35](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.34...sn_cli-v0.81.35) - 2023-09-13

### Added
- *(register)* paying nodes for Register storage

## [0.81.34](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.33...sn_cli-v0.81.34) - 2023-09-12

### Added
- utilize stream decryptor

## [0.81.33](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.32...sn_cli-v0.81.33) - 2023-09-12

### Other
- update dependencies

## [0.81.32](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.31...sn_cli-v0.81.32) - 2023-09-12

### Other
- *(metrics)* rename network metrics and remove from default features list

## [0.81.31](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.30...sn_cli-v0.81.31) - 2023-09-12

### Added
- add tx and parent spends verification
- chunk payments using UTXOs instead of DBCs

### Other
- use updated sn_dbc

## [0.81.30](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.29...sn_cli-v0.81.30) - 2023-09-11

### Other
- update dependencies

## [0.81.29](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.28...sn_cli-v0.81.29) - 2023-09-11

### Other
- utilize stream encryptor

## [0.81.28](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.27...sn_cli-v0.81.28) - 2023-09-11

### Other
- update dependencies

## [0.81.27](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.26...sn_cli-v0.81.27) - 2023-09-08

### Added
- *(client)* repay for chunks if they cannot be validated

### Fixed
- *(client)* dont bail on failed upload before verify/repay

### Other
- *(client)* refactor to have permits at network layer
- *(refactor)* remove wallet_client args from upload flow
- *(refactor)* remove upload_chunks semaphore arg

## [0.81.26](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.25...sn_cli-v0.81.26) - 2023-09-07

### Other
- update dependencies

## [0.81.25](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.24...sn_cli-v0.81.25) - 2023-09-07

### Other
- update dependencies

## [0.81.24](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.23...sn_cli-v0.81.24) - 2023-09-07

### Other
- update dependencies

## [0.81.23](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.22...sn_cli-v0.81.23) - 2023-09-06

### Other
- update dependencies

## [0.81.22](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.21...sn_cli-v0.81.22) - 2023-09-05

### Other
- update dependencies

## [0.81.21](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.20...sn_cli-v0.81.21) - 2023-09-05

### Other
- update dependencies

## [0.81.20](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.19...sn_cli-v0.81.20) - 2023-09-05

### Other
- update dependencies

## [0.81.19](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.18...sn_cli-v0.81.19) - 2023-09-05

### Added
- *(cli)* properly init color_eyre, advise on hex parse fail

## [0.81.18](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.17...sn_cli-v0.81.18) - 2023-09-05

### Other
- update dependencies

## [0.81.17](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.16...sn_cli-v0.81.17) - 2023-09-04

### Other
- update dependencies

## [0.81.16](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.15...sn_cli-v0.81.16) - 2023-09-04

### Other
- update dependencies

## [0.81.15](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.14...sn_cli-v0.81.15) - 2023-09-04

### Other
- update dependencies

## [0.81.14](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.13...sn_cli-v0.81.14) - 2023-09-04

### Other
- update dependencies

## [0.81.13](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.12...sn_cli-v0.81.13) - 2023-09-02

### Other
- update dependencies

## [0.81.12](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.11...sn_cli-v0.81.12) - 2023-09-01

### Other
- update dependencies

## [0.81.11](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.10...sn_cli-v0.81.11) - 2023-09-01

### Other
- *(cli)* better formatting for elapsed time statements
- *(transfers)* store dbcs by ref to avoid more clones

## [0.81.10](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.9...sn_cli-v0.81.10) - 2023-09-01

### Other
- update dependencies

## [0.81.9](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.8...sn_cli-v0.81.9) - 2023-09-01

### Other
- update dependencies

## [0.81.8](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.7...sn_cli-v0.81.8) - 2023-08-31

### Added
- *(cli)* perform wallet actions without connecting to the network

### Other
- remove unused async

## [0.81.7](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.6...sn_cli-v0.81.7) - 2023-08-31

### Other
- update dependencies

## [0.81.6](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.5...sn_cli-v0.81.6) - 2023-08-31

### Added
- *(cli)* wallet cmd flag enabing to query a node's local wallet balance

### Fixed
- *(cli)* don't try to create wallet paths when checking balance

## [0.81.5](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.4...sn_cli-v0.81.5) - 2023-08-31

### Other
- update dependencies

## [0.81.4](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.3...sn_cli-v0.81.4) - 2023-08-31

### Other
- update dependencies

## [0.81.3](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.2...sn_cli-v0.81.3) - 2023-08-31

### Fixed
- correct bench download calculation

## [0.81.2](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.1...sn_cli-v0.81.2) - 2023-08-31

### Other
- update dependencies

## [0.81.1](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.81.0...sn_cli-v0.81.1) - 2023-08-31

### Added
- *(cli)* expose 'concurrency' flag
- *(cli)* increase put parallelisation

### Other
- *(client)* improve download concurrency.

## [0.81.0](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.64...sn_cli-v0.81.0) - 2023-08-30

### Added
- refactor to allow greater upload parallelisation
- one transfer per data set, mapped dbcs to content addrs
- [**breaking**] pay each chunk holder direct
- feat!(protocol): get price and pay for each chunk individually
- feat!(protocol): remove chunk merkletree to simplify payment

### Fixed
- *(tokio)* remove tokio fs

### Other
- *(deps)* bump tokio to 1.32.0
- *(client)* refactor client wallet to reduce dbc clones
- *(client)* pass around content payments map mut ref
- *(client)* reduce transferoutputs cloning

## [0.80.64](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.63...sn_cli-v0.80.64) - 2023-08-30

### Other
- update dependencies

## [0.80.63](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.62...sn_cli-v0.80.63) - 2023-08-30

### Other
- update dependencies

## [0.80.62](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.61...sn_cli-v0.80.62) - 2023-08-29

### Other
- update dependencies

## [0.80.61](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.60...sn_cli-v0.80.61) - 2023-08-25

### Other
- update dependencies

## [0.80.60](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.59...sn_cli-v0.80.60) - 2023-08-24

### Other
- *(cli)* verify bench uploads once more

## [0.80.59](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.58...sn_cli-v0.80.59) - 2023-08-24

### Other
- rust 1.72.0 fixes

## [0.80.58](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.57...sn_cli-v0.80.58) - 2023-08-24

### Other
- update dependencies

## [0.80.57](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.56...sn_cli-v0.80.57) - 2023-08-22

### Other
- update dependencies

## [0.80.56](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.55...sn_cli-v0.80.56) - 2023-08-22

### Fixed
- fixes to allow upload file works properly

## [0.80.55](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.54...sn_cli-v0.80.55) - 2023-08-21

### Other
- update dependencies

## [0.80.54](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.53...sn_cli-v0.80.54) - 2023-08-21

### Other
- update dependencies

## [0.80.53](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.52...sn_cli-v0.80.53) - 2023-08-18

### Other
- update dependencies

## [0.80.52](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.51...sn_cli-v0.80.52) - 2023-08-18

### Other
- update dependencies

## [0.80.51](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.50...sn_cli-v0.80.51) - 2023-08-17

### Other
- update dependencies

## [0.80.50](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.49...sn_cli-v0.80.50) - 2023-08-17

### Other
- update dependencies

## [0.80.49](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.48...sn_cli-v0.80.49) - 2023-08-17

### Other
- update dependencies

## [0.80.48](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.47...sn_cli-v0.80.48) - 2023-08-17

### Fixed
- avoid download bench result polluted

### Other
- more client logs

## [0.80.47](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.46...sn_cli-v0.80.47) - 2023-08-16

### Other
- update dependencies

## [0.80.46](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.45...sn_cli-v0.80.46) - 2023-08-16

### Other
- update dependencies

## [0.80.45](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.44...sn_cli-v0.80.45) - 2023-08-16

### Other
- optimize benchmark flow

## [0.80.44](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.43...sn_cli-v0.80.44) - 2023-08-15

### Other
- update dependencies

## [0.80.43](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.42...sn_cli-v0.80.43) - 2023-08-14

### Other
- update dependencies

## [0.80.42](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.41...sn_cli-v0.80.42) - 2023-08-14

### Other
- update dependencies

## [0.80.41](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.40...sn_cli-v0.80.41) - 2023-08-11

### Other
- *(cli)* print cost info

## [0.80.40](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.39...sn_cli-v0.80.40) - 2023-08-11

### Other
- update dependencies

## [0.80.39](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.38...sn_cli-v0.80.39) - 2023-08-10

### Other
- update dependencies

## [0.80.38](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.37...sn_cli-v0.80.38) - 2023-08-10

### Other
- update dependencies

## [0.80.37](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.36...sn_cli-v0.80.37) - 2023-08-09

### Other
- update dependencies

## [0.80.36](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.35...sn_cli-v0.80.36) - 2023-08-08

### Fixed
- *(cli)* remove manual faucet claim from benchmarking.
- *(node)* prevent panic in storage calcs

### Other
- *(cli)* get more money for benching
- log bench errors

## [0.80.35](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.34...sn_cli-v0.80.35) - 2023-08-07

### Other
- update dependencies

## [0.80.34](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.33...sn_cli-v0.80.34) - 2023-08-07

### Other
- *(node)* dont verify during benchmarks

## [0.80.33](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.32...sn_cli-v0.80.33) - 2023-08-07

### Added
- rework register addresses to include pk

### Other
- cleanup comments and names

## [0.80.32](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.31...sn_cli-v0.80.32) - 2023-08-07

### Other
- update dependencies

## [0.80.31](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.30...sn_cli-v0.80.31) - 2023-08-04

### Other
- update dependencies

## [0.80.30](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.29...sn_cli-v0.80.30) - 2023-08-04

### Other
- update dependencies

## [0.80.29](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.28...sn_cli-v0.80.29) - 2023-08-03

### Other
- update dependencies

## [0.80.28](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.27...sn_cli-v0.80.28) - 2023-08-03

### Other
- update dependencies

## [0.80.27](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.26...sn_cli-v0.80.27) - 2023-08-03

### Other
- update dependencies

## [0.80.26](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.25...sn_cli-v0.80.26) - 2023-08-03

### Other
- update dependencies

## [0.80.25](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.24...sn_cli-v0.80.25) - 2023-08-03

### Other
- update dependencies

## [0.80.24](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.23...sn_cli-v0.80.24) - 2023-08-02

### Other
- update dependencies

## [0.80.23](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.22...sn_cli-v0.80.23) - 2023-08-02

### Other
- update dependencies

## [0.80.22](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.21...sn_cli-v0.80.22) - 2023-08-01

### Other
- update dependencies

## [0.80.21](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.20...sn_cli-v0.80.21) - 2023-08-01

### Other
- update dependencies

## [0.80.20](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.19...sn_cli-v0.80.20) - 2023-08-01

### Other
- update dependencies

## [0.80.19](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.18...sn_cli-v0.80.19) - 2023-08-01

### Other
- update dependencies

## [0.80.18](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.17...sn_cli-v0.80.18) - 2023-08-01

### Added
- *(cli)* add no-verify flag to cli

### Other
- *(cli)* update logs and ci for payments

## [0.80.17](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.16...sn_cli-v0.80.17) - 2023-08-01

### Other
- update dependencies

## [0.80.16](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.15...sn_cli-v0.80.16) - 2023-07-31

### Other
- update dependencies

## [0.80.15](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.14...sn_cli-v0.80.15) - 2023-07-31

### Other
- update dependencies

## [0.80.14](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.13...sn_cli-v0.80.14) - 2023-07-31

### Other
- update dependencies

## [0.80.13](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.12...sn_cli-v0.80.13) - 2023-07-31

### Other
- update dependencies

## [0.80.12](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.11...sn_cli-v0.80.12) - 2023-07-28

### Other
- update dependencies

## [0.80.11](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.10...sn_cli-v0.80.11) - 2023-07-28

### Other
- update dependencies

## [0.80.10](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.9...sn_cli-v0.80.10) - 2023-07-28

### Other
- update dependencies

## [0.80.9](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.8...sn_cli-v0.80.9) - 2023-07-28

### Other
- update dependencies

## [0.80.8](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.7...sn_cli-v0.80.8) - 2023-07-27

### Other
- update dependencies

## [0.80.7](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.6...sn_cli-v0.80.7) - 2023-07-26

### Other
- update dependencies

## [0.80.6](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.5...sn_cli-v0.80.6) - 2023-07-26

### Other
- update dependencies

## [0.80.5](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.4...sn_cli-v0.80.5) - 2023-07-26

### Other
- update dependencies

## [0.80.4](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.3...sn_cli-v0.80.4) - 2023-07-26

### Other
- update dependencies

## [0.80.3](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.2...sn_cli-v0.80.3) - 2023-07-26

### Other
- update dependencies

## [0.80.2](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.1...sn_cli-v0.80.2) - 2023-07-26

### Other
- update dependencies

## [0.80.1](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.80.0...sn_cli-v0.80.1) - 2023-07-25

### Other
- update dependencies

## [0.80.0](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.32...sn_cli-v0.80.0) - 2023-07-21

### Added
- *(cli)* allow to pass the hex-encoded DBC as arg
- *(protocol)* [**breaking**] make Chunks storage payment required

## [0.79.32](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.31...sn_cli-v0.79.32) - 2023-07-20

### Other
- update dependencies

## [0.79.31](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.30...sn_cli-v0.79.31) - 2023-07-20

### Other
- update dependencies

## [0.79.30](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.29...sn_cli-v0.79.30) - 2023-07-19

### Other
- update dependencies

## [0.79.29](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.28...sn_cli-v0.79.29) - 2023-07-19

### Other
- update dependencies

## [0.79.28](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.27...sn_cli-v0.79.28) - 2023-07-19

### Other
- update dependencies

## [0.79.27](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.26...sn_cli-v0.79.27) - 2023-07-19

### Other
- update dependencies

## [0.79.26](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.25...sn_cli-v0.79.26) - 2023-07-18

### Other
- update dependencies

## [0.79.25](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.24...sn_cli-v0.79.25) - 2023-07-18

### Other
- update dependencies

## [0.79.24](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.23...sn_cli-v0.79.24) - 2023-07-18

### Fixed
- client

## [0.79.23](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.22...sn_cli-v0.79.23) - 2023-07-18

### Other
- update dependencies

## [0.79.22](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.21...sn_cli-v0.79.22) - 2023-07-17

### Fixed
- *(cli)* add more context when failing to decode a wallet

## [0.79.21](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.20...sn_cli-v0.79.21) - 2023-07-17

### Other
- update dependencies

## [0.79.20](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.19...sn_cli-v0.79.20) - 2023-07-17

### Added
- *(networking)* upgrade to libp2p 0.52.0

## [0.79.19](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.18...sn_cli-v0.79.19) - 2023-07-17

### Added
- *(client)* keep storage payment proofs in local wallet

## [0.79.18](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.17...sn_cli-v0.79.18) - 2023-07-13

### Other
- update dependencies

## [0.79.17](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.16...sn_cli-v0.79.17) - 2023-07-13

### Other
- update dependencies

## [0.79.16](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.15...sn_cli-v0.79.16) - 2023-07-12

### Other
- client to upload paid chunks in batches
- chunk files only once when making payment for their storage

## [0.79.15](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.14...sn_cli-v0.79.15) - 2023-07-11

### Other
- update dependencies

## [0.79.14](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.13...sn_cli-v0.79.14) - 2023-07-11

### Fixed
- *(client)* publish register on creation

## [0.79.13](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.12...sn_cli-v0.79.13) - 2023-07-11

### Other
- update dependencies

## [0.79.12](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.11...sn_cli-v0.79.12) - 2023-07-11

### Other
- update dependencies

## [0.79.11](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.10...sn_cli-v0.79.11) - 2023-07-11

### Other
- update dependencies

## [0.79.10](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.9...sn_cli-v0.79.10) - 2023-07-10

### Other
- update dependencies

## [0.79.9](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.8...sn_cli-v0.79.9) - 2023-07-10

### Other
- update dependencies

## [0.79.8](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.7...sn_cli-v0.79.8) - 2023-07-10

### Added
- faucet server and cli DBC read

### Fixed
- use Deposit --stdin instead of Read in cli
- wallet store

## [0.79.7](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.6...sn_cli-v0.79.7) - 2023-07-10

### Other
- update dependencies

## [0.79.6](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.5...sn_cli-v0.79.6) - 2023-07-07

### Other
- update dependencies

## [0.79.5](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.4...sn_cli-v0.79.5) - 2023-07-07

### Other
- update dependencies

## [0.79.4](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.3...sn_cli-v0.79.4) - 2023-07-07

### Other
- update dependencies

## [0.79.3](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.2...sn_cli-v0.79.3) - 2023-07-07

### Other
- update dependencies

## [0.79.2](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.1...sn_cli-v0.79.2) - 2023-07-06

### Other
- update dependencies

## [0.79.1](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.79.0...sn_cli-v0.79.1) - 2023-07-06

### Other
- update dependencies

## [0.79.0](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.26...sn_cli-v0.79.0) - 2023-07-06

### Added
- introduce `--log-format` arguments
- provide `--log-output-dest` arg for `safe`
- provide `--log-output-dest` arg for `safenode`

### Other
- use data-dir rather than root-dir
- incorporate various feedback items

## [0.78.26](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.25...sn_cli-v0.78.26) - 2023-07-05

### Other
- update dependencies

## [0.78.25](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.24...sn_cli-v0.78.25) - 2023-07-05

### Other
- update dependencies

## [0.78.24](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.23...sn_cli-v0.78.24) - 2023-07-05

### Other
- update dependencies

## [0.78.23](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.22...sn_cli-v0.78.23) - 2023-07-04

### Other
- update dependencies

## [0.78.22](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.21...sn_cli-v0.78.22) - 2023-07-03

### Other
- reduce SAMPLE_SIZE for the data_with_churn test

## [0.78.21](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.20...sn_cli-v0.78.21) - 2023-06-29

### Other
- update dependencies

## [0.78.20](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.19...sn_cli-v0.78.20) - 2023-06-29

### Other
- update dependencies

## [0.78.19](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.18...sn_cli-v0.78.19) - 2023-06-28

### Other
- update dependencies

## [0.78.18](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.17...sn_cli-v0.78.18) - 2023-06-28

### Added
- register refactor, kad reg without cmds

## [0.78.17](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.16...sn_cli-v0.78.17) - 2023-06-28

### Other
- update dependencies

## [0.78.16](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.15...sn_cli-v0.78.16) - 2023-06-28

### Other
- update dependencies

## [0.78.15](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.14...sn_cli-v0.78.15) - 2023-06-27

### Other
- update dependencies

## [0.78.14](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.13...sn_cli-v0.78.14) - 2023-06-27

### Other
- update dependencies

## [0.78.13](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.12...sn_cli-v0.78.13) - 2023-06-27

### Other
- benchmark client download

## [0.78.12](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.11...sn_cli-v0.78.12) - 2023-06-26

### Other
- update dependencies

## [0.78.11](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.10...sn_cli-v0.78.11) - 2023-06-26

### Other
- update dependencies

## [0.78.10](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.9...sn_cli-v0.78.10) - 2023-06-26

### Other
- update dependencies

## [0.78.9](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.8...sn_cli-v0.78.9) - 2023-06-26

### Other
- update dependencies

## [0.78.8](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.7...sn_cli-v0.78.8) - 2023-06-26

### Other
- update dependencies

## [0.78.7](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.6...sn_cli-v0.78.7) - 2023-06-24

### Other
- update dependencies

## [0.78.6](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.5...sn_cli-v0.78.6) - 2023-06-23

### Other
- update dependencies

## [0.78.5](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.4...sn_cli-v0.78.5) - 2023-06-23

### Other
- update dependencies

## [0.78.4](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.3...sn_cli-v0.78.4) - 2023-06-23

### Other
- update dependencies

## [0.78.3](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.2...sn_cli-v0.78.3) - 2023-06-23

### Other
- update dependencies

## [0.78.2](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.1...sn_cli-v0.78.2) - 2023-06-22

### Other
- update dependencies

## [0.78.1](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.78.0...sn_cli-v0.78.1) - 2023-06-22

### Other
- *(client)* initial refactor around uploads

## [0.78.0](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.49...sn_cli-v0.78.0) - 2023-06-22

### Added
- use standarised directories for files/wallet commands

## [0.77.49](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.48...sn_cli-v0.77.49) - 2023-06-21

### Other
- update dependencies

## [0.77.48](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.47...sn_cli-v0.77.48) - 2023-06-21

### Other
- update dependencies

## [0.77.47](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.46...sn_cli-v0.77.47) - 2023-06-21

### Other
- *(node)* obtain parent_tx from SignedSpend

## [0.77.46](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.45...sn_cli-v0.77.46) - 2023-06-21

### Added
- provide option for log output in json

## [0.77.45](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.44...sn_cli-v0.77.45) - 2023-06-20

### Other
- update dependencies

## [0.77.44](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.43...sn_cli-v0.77.44) - 2023-06-20

### Other
- update dependencies

## [0.77.43](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.42...sn_cli-v0.77.43) - 2023-06-20

### Other
- include the Tx instead of output DBCs as part of storage payment proofs
- use a set to collect Chunks addrs for build payment proof

## [0.77.42](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.41...sn_cli-v0.77.42) - 2023-06-20

### Other
- update dependencies

## [0.77.41](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.40...sn_cli-v0.77.41) - 2023-06-20

### Other
- update dependencies

## [0.77.40](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.39...sn_cli-v0.77.40) - 2023-06-20

### Other
- update dependencies

## [0.77.39](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.38...sn_cli-v0.77.39) - 2023-06-20

### Other
- update dependencies

## [0.77.38](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.37...sn_cli-v0.77.38) - 2023-06-20

### Other
- update dependencies

## [0.77.37](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.36...sn_cli-v0.77.37) - 2023-06-19

### Other
- update dependencies

## [0.77.36](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.35...sn_cli-v0.77.36) - 2023-06-19

### Other
- update dependencies

## [0.77.35](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.34...sn_cli-v0.77.35) - 2023-06-19

### Other
- update dependencies

## [0.77.34](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.33...sn_cli-v0.77.34) - 2023-06-19

### Other
- update dependencies

## [0.77.33](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.32...sn_cli-v0.77.33) - 2023-06-19

### Other
- update dependencies

## [0.77.32](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.31...sn_cli-v0.77.32) - 2023-06-19

### Fixed
- *(safe)* check if upload path contains a file

## [0.77.31](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.30...sn_cli-v0.77.31) - 2023-06-16

### Fixed
- CLI is missing local-discovery feature

## [0.77.30](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.29...sn_cli-v0.77.30) - 2023-06-16

### Other
- update dependencies

## [0.77.29](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.28...sn_cli-v0.77.29) - 2023-06-16

### Other
- update dependencies

## [0.77.28](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.27...sn_cli-v0.77.28) - 2023-06-16

### Other
- improve memory benchmarks, remove broken download bench

## [0.77.27](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.26...sn_cli-v0.77.27) - 2023-06-16

### Other
- update dependencies

## [0.77.26](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.25...sn_cli-v0.77.26) - 2023-06-16

### Fixed
- *(bin)* negate local-discovery check

## [0.77.25](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.24...sn_cli-v0.77.25) - 2023-06-16

### Other
- update dependencies

## [0.77.24](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.23...sn_cli-v0.77.24) - 2023-06-15

### Other
- update dependencies

## [0.77.23](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.22...sn_cli-v0.77.23) - 2023-06-15

### Fixed
- parent spend issue

## [0.77.22](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.21...sn_cli-v0.77.22) - 2023-06-15

### Other
- update dependencies

## [0.77.21](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.20...sn_cli-v0.77.21) - 2023-06-15

### Other
- update dependencies

## [0.77.20](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.19...sn_cli-v0.77.20) - 2023-06-15

### Other
- update dependencies

## [0.77.19](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.18...sn_cli-v0.77.19) - 2023-06-15

### Other
- use throughput for benchmarking

## [0.77.18](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.17...sn_cli-v0.77.18) - 2023-06-15

### Other
- add initial benchmarks for prs and chart generation

## [0.77.17](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.16...sn_cli-v0.77.17) - 2023-06-14

### Added
- include output DBC within payment proof for Chunks storage

## [0.77.16](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.15...sn_cli-v0.77.16) - 2023-06-14

### Other
- update dependencies

## [0.77.15](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.14...sn_cli-v0.77.15) - 2023-06-14

### Other
- use clap env and parse multiaddr

## [0.77.14](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.13...sn_cli-v0.77.14) - 2023-06-14

### Added
- *(client)* expose req/resp timeout to client cli

### Other
- *(client)* parse duration in clap derivation

## [0.77.13](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.12...sn_cli-v0.77.13) - 2023-06-13

### Other
- update dependencies

## [0.77.12](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.11...sn_cli-v0.77.12) - 2023-06-13

### Other
- update dependencies

## [0.77.11](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.10...sn_cli-v0.77.11) - 2023-06-12

### Other
- update dependencies

## [0.77.10](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.9...sn_cli-v0.77.10) - 2023-06-12

### Other
- update dependencies

## [0.77.9](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.8...sn_cli-v0.77.9) - 2023-06-09

### Other
- improve documentation for cli commands

## [0.77.8](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.7...sn_cli-v0.77.8) - 2023-06-09

### Other
- manually change crate version

## [0.77.7](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.6...sn_cli-v0.77.7) - 2023-06-09

### Other
- update dependencies

## [0.77.6](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.5...sn_cli-v0.77.6) - 2023-06-09

### Other
- emit git info with vergen

## [0.77.5](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.4...sn_cli-v0.77.5) - 2023-06-09

### Other
- update dependencies

## [0.77.4](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.3...sn_cli-v0.77.4) - 2023-06-09

### Other
- provide clarity on command arguments

## [0.77.3](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.2...sn_cli-v0.77.3) - 2023-06-08

### Other
- update dependencies

## [0.77.2](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.1...sn_cli-v0.77.2) - 2023-06-08

### Other
- improve documentation for cli arguments

## [0.77.1](https://github.com/maidsafe/safe_network/compare/sn_cli-v0.77.0...sn_cli-v0.77.1) - 2023-06-07

### Added
- making the CLI --peer arg global so it can be passed in any order
- bail out if empty list of addreses is provided for payment proof generation
- *(client)* add progress indicator for initial network connections
- attach payment proof when uploading Chunks
- collect payment proofs and make sure merkletree always has pow-of-2 leaves
- node side payment proof validation from a given Chunk, audit trail, and reason-hash
- use all Chunks of a file to generate payment the payment proof tree
- Chunk storage payment and building payment proofs

### Other
- Revert "chore(release): sn_cli-v0.77.1/sn_client-v0.85.2/sn_networking-v0.1.2/sn_node-v0.83.1"
- improve CLI --peer arg doc
- *(release)* sn_cli-v0.77.1/sn_client-v0.85.2/sn_networking-v0.1.2/sn_node-v0.83.1
- Revert "chore(release): sn_cli-v0.77.1/sn_client-v0.85.2/sn_networking-v0.1.2/sn_protocol-v0.1.2/sn_node-v0.83.1/sn_record_store-v0.1.2/sn_registers-v0.1.2"
- *(release)* sn_cli-v0.77.1/sn_client-v0.85.2/sn_networking-v0.1.2/sn_protocol-v0.1.2/sn_node-v0.83.1/sn_record_store-v0.1.2/sn_registers-v0.1.2
- *(logs)* enable metrics feature by default
- small log wording updates
- making Chunk payment proof optional for now
- moving all payment proofs utilities into sn_transfers crate
