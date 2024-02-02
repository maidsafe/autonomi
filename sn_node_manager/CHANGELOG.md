# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.40](https://github.com/maidsafe/safe_network/compare/sn-node-manager-v0.1.39...sn-node-manager-v0.1.40) - 2024-02-02

### Fixed
- *(manager)* set the entire service file details for linux
- *(manager)* set safenode service KillMode to fix restarts

## [0.1.39](https://github.com/maidsafe/safe_network/compare/sn-node-manager-v0.1.38...sn-node-manager-v0.1.39) - 2024-02-02

### Other
- update dependencies

## [0.1.38](https://github.com/maidsafe/safe_network/compare/sn-node-manager-v0.1.37...sn-node-manager-v0.1.38) - 2024-02-02

### Other
- update dependencies

## [0.1.37](https://github.com/maidsafe/safe_network/compare/sn-node-manager-v0.1.36...sn-node-manager-v0.1.37) - 2024-02-01

### Other
- update dependencies

## [0.1.36](https://github.com/maidsafe/safe_network/compare/sn-node-manager-v0.1.35...sn-node-manager-v0.1.36) - 2024-02-01

### Other
- update dependencies

## [0.1.35](https://github.com/maidsafe/safe_network/compare/sn-node-manager-v0.1.34...sn-node-manager-v0.1.35) - 2024-02-01

### Other
- update dependencies

## [0.1.34](https://github.com/maidsafe/safe_network/compare/sn-node-manager-v0.1.33...sn-node-manager-v0.1.34) - 2024-01-31

### Added
- provide `--build` flag for commands

### Other
- download binary once for `add` command
- misc clean up for local testnets

## [0.1.33](https://github.com/maidsafe/safe_network/compare/sn-node-manager-v0.1.32...sn-node-manager-v0.1.33) - 2024-01-31

### Other
- update dependencies

## [0.1.32](https://github.com/maidsafe/safe_network/compare/sn-node-manager-v0.1.31...sn-node-manager-v0.1.32) - 2024-01-31

### Other
- update dependencies

## [0.1.31](https://github.com/maidsafe/safe_network/compare/sn-node-manager-v0.1.30...sn-node-manager-v0.1.31) - 2024-01-30

### Other
- update dependencies

## [0.1.30](https://github.com/maidsafe/safe_network/compare/sn-node-manager-v0.1.29...sn-node-manager-v0.1.30) - 2024-01-30

### Other
- update dependencies

## [0.1.29](https://github.com/maidsafe/safe_network/compare/sn-node-manager-v0.1.28...sn-node-manager-v0.1.29) - 2024-01-30

### Other
- update dependencies

## [0.1.28](https://github.com/maidsafe/safe_network/compare/sn-node-manager-v0.1.27...sn-node-manager-v0.1.28) - 2024-01-30

### Other
- update dependencies

## [0.1.27](https://github.com/maidsafe/safe_network/compare/sn-node-manager-v0.1.26...sn-node-manager-v0.1.27) - 2024-01-30

### Other
- *(manager)* provide rpc address instead of rpc port

## [0.1.26](https://github.com/maidsafe/safe_network/compare/sn-node-manager-v0.1.25...sn-node-manager-v0.1.26) - 2024-01-29

### Other
- *(manager)* make VerbosityLevel a public type

## [0.1.25](https://github.com/maidsafe/safe_network/compare/sn-node-manager-v0.1.24...sn-node-manager-v0.1.25) - 2024-01-29

### Other
- provide verbosity level
- improve error handling for `start` command
- improve error handling for `add` command
- version and url arguments conflict

## [0.1.24](https://github.com/maidsafe/safe_network/compare/sn-node-manager-v0.1.23...sn-node-manager-v0.1.24) - 2024-01-29

### Other
- update dependencies

## [0.1.23](https://github.com/maidsafe/safe_network/compare/sn-node-manager-v0.1.22...sn-node-manager-v0.1.23) - 2024-01-26

### Other
- update dependencies

## [0.1.22](https://github.com/maidsafe/safe_network/compare/sn-node-manager-v0.1.21...sn-node-manager-v0.1.22) - 2024-01-25

### Other
- update dependencies

## [0.1.21](https://github.com/maidsafe/safe_network/compare/sn-node-manager-v0.1.20...sn-node-manager-v0.1.21) - 2024-01-25

### Other
- update dependencies

## [0.1.20](https://github.com/maidsafe/safe_network/compare/sn-node-manager-v0.1.19...sn-node-manager-v0.1.20) - 2024-01-25

### Fixed
- *(manager)* increase port unbinding time

### Other
- rename sn_node_manager crate
- *(manager)* rename node manager crate

## [0.1.19](https://github.com/maidsafe/sn-node-manager/compare/v0.1.18...v0.1.19) - 2024-01-23

### Fixed
- add delay to make sure we drop the socket

### Other
- force skip validation

## [0.1.18](https://github.com/maidsafe/sn-node-manager/compare/v0.1.17...v0.1.18) - 2024-01-22

### Added
- provide `faucet` command
- `status` command enhancements
- provide `--local` flag for `add`

### Other
- fixup after rebase
- provide script for local network
- additional info in `status` cmd

## [0.1.17](https://github.com/maidsafe/sn-node-manager/compare/v0.1.16...v0.1.17) - 2024-01-18

### Added
- add quic/tcp features and set quic as default

## [0.1.16](https://github.com/maidsafe/sn-node-manager/compare/v0.1.15...v0.1.16) - 2024-01-16

### Other
- tidy peer management for `join` command

## [0.1.15](https://github.com/maidsafe/sn-node-manager/compare/v0.1.14...v0.1.15) - 2024-01-15

### Other
- manually parse environment variable

## [0.1.14](https://github.com/maidsafe/sn-node-manager/compare/v0.1.13...v0.1.14) - 2024-01-12

### Added
- apply `--first` argument to added service

## [0.1.13](https://github.com/maidsafe/sn-node-manager/compare/v0.1.12...v0.1.13) - 2024-01-10

### Fixed
- apply to correct argument

## [0.1.12](https://github.com/maidsafe/sn-node-manager/compare/v0.1.11...v0.1.12) - 2024-01-09

### Other
- use `--first` arg for genesis node

## [0.1.11](https://github.com/maidsafe/sn-node-manager/compare/v0.1.10...v0.1.11) - 2023-12-21

### Added
- download binaries in absence of paths

## [0.1.10](https://github.com/maidsafe/sn-node-manager/compare/v0.1.9...v0.1.10) - 2023-12-19

### Added
- provide `run` command

## [0.1.9](https://github.com/maidsafe/sn-node-manager/compare/v0.1.8...v0.1.9) - 2023-12-14

### Added
- custom port arguments for `add` command

## [0.1.8](https://github.com/maidsafe/sn-node-manager/compare/v0.1.7...v0.1.8) - 2023-12-13

### Other
- remove network contacts from peer acquisition

## [0.1.7](https://github.com/maidsafe/sn-node-manager/compare/v0.1.6...v0.1.7) - 2023-12-13

### Added
- provide `--url` argument for `add` command

## [0.1.6](https://github.com/maidsafe/sn-node-manager/compare/v0.1.5...v0.1.6) - 2023-12-12

### Fixed
- accommodate service restarts in `status` cmd

## [0.1.5](https://github.com/maidsafe/sn-node-manager/compare/v0.1.4...v0.1.5) - 2023-12-08

### Added
- provide `upgrade` command
- each service instance to use its own binary

## [0.1.4](https://github.com/maidsafe/sn-node-manager/compare/v0.1.3...v0.1.4) - 2023-12-05

### Other
- upload 'latest' version to S3

## [0.1.3](https://github.com/maidsafe/sn-node-manager/compare/v0.1.2...v0.1.3) - 2023-12-05

### Added
- provide `remove` command

## [0.1.2](https://github.com/maidsafe/sn-node-manager/compare/v0.1.1...v0.1.2) - 2023-12-05

### Added
- provide `--peer` argument

### Other
- rename `install` command to `add`

## [0.1.1](https://github.com/maidsafe/sn-node-manager/compare/v0.1.0...v0.1.1) - 2023-11-29

### Other
- improve docs for `start` and `stop` commands

## [0.1.0](https://github.com/maidsafe/sn-node-manager/releases/tag/v0.1.0) - 2023-11-29

### Added
- provide `status` command
- provide `stop` command
- provide `start` command
- provide `install` command

### Other
- release process and licensing
- extend the e2e test for new commands
- reference `sn_node_rpc_client` crate
- specify root and log dirs at install time
- provide initial integration tests
- Initial commit
