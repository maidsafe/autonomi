# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.11](https://github.com/maidsafe/safe_network/compare/sn_peers_acquisition-v0.1.10...sn_peers_acquisition-v0.1.11) - 2023-12-01

### Other
- *(ci)* fix CI build cache parsing error

## [0.1.10](https://github.com/maidsafe/safe_network/compare/sn_peers_acquisition-v0.1.9...sn_peers_acquisition-v0.1.10) - 2023-11-22

### Added
- *(peers_acq)* shuffle peers before we return.

## [0.1.9](https://github.com/maidsafe/safe_network/compare/sn_peers_acquisition-v0.1.8...sn_peers_acquisition-v0.1.9) - 2023-11-06

### Added
- *(deps)* upgrade libp2p to 0.53

## [0.1.8](https://github.com/maidsafe/safe_network/compare/sn_peers_acquisition-v0.1.7...sn_peers_acquisition-v0.1.8) - 2023-10-26

### Fixed
- always put SAFE_PEERS as one of the bootstrap peer, if presents

## [0.1.7](https://github.com/maidsafe/safe_network/compare/sn_peers_acquisition-v0.1.6...sn_peers_acquisition-v0.1.7) - 2023-09-25

### Added
- *(peers)* use rustls-tls and readd https to the network-contacts url
- *(peers)* use a common way to bootstrap into the network for all the bins

### Fixed
- *(peers_acquisition)* bail on fail to parse peer id

### Other
- more logs around parsing network-contacts
- log the actual contacts url in messages

## [0.1.6](https://github.com/maidsafe/safe_network/compare/sn_peers_acquisition-v0.1.5...sn_peers_acquisition-v0.1.6) - 2023-08-30

### Other
- *(docs)* adjust --peer docs

## [0.1.5](https://github.com/maidsafe/safe_network/compare/sn_peers_acquisition-v0.1.4...sn_peers_acquisition-v0.1.5) - 2023-08-29

### Added
- *(node)* add feature flag for tcp/quic

## [0.1.4](https://github.com/maidsafe/safe_network/compare/sn_peers_acquisition-v0.1.3...sn_peers_acquisition-v0.1.4) - 2023-07-17

### Added
- *(networking)* upgrade to libp2p 0.52.0

## [0.1.3](https://github.com/maidsafe/safe_network/compare/sn_peers_acquisition-v0.1.2...sn_peers_acquisition-v0.1.3) - 2023-07-03

### Other
- various tidy up

## [0.1.2](https://github.com/maidsafe/safe_network/compare/sn_peers_acquisition-v0.1.1...sn_peers_acquisition-v0.1.2) - 2023-06-28

### Added
- *(node)* dial without PeerId

## [0.1.1](https://github.com/maidsafe/safe_network/compare/sn_peers_acquisition-v0.1.0...sn_peers_acquisition-v0.1.1) - 2023-06-14

### Other
- use clap env and parse multiaddr

## [0.1.0](https://github.com/jacderida/safe_network/releases/tag/sn_peers_acquisition-v0.1.0) - 2023-06-04

### Fixed
- *(node)* correct dead peer detection
- local-discovery deps
