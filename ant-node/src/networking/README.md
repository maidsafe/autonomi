### Some reference implementations
The [libp2p/file-sharing](https://github.com/libp2p/rust-libp2p/tree/master/examples/file-sharing) example showcases [how to integrate libp2p into larger applications](https://github.com/libp2p/rust-libp2p/pull/2186)

Couple of codebase that follow a similar pattern,
1. https://github.com/paritytech/substrate/blob/be9fa62238fcfd7eb49218809a6b981f71c34eb3/client/network/src/service.rs
2. [Filecoin node](https://github.com/ChainSafe/forest/blob/bce3deded7af10d3bc4237801d306f8e810a4282/node/forest_libp2p/bitswap/src/behaviour.rs)
3. https://github.com/eqlabs/pathfinder/blob/00c9bfb9b285fe7de8d77e8a25c0607db2b6dd34/crates/p2p/src/lib.rs
4. https://github.com/Dione-Software/dione/blob/c46b5e43d9e69f0c567b76d514d17e097231ffd5/dione-server/src/network/mod.rs#L260
5. https://github.com/fleek-network/ursa/blob/4dcd1ad18fad5e8bdcfe72ac7fb6bdb1d5612473/crates/ursa-network/src/service.rs#L79
6. https://github.com/subconsciousnetwork/noosphere/blob/9520826029235e5dc32adca77193b4f82b9de80c/rust/noosphere-ns/src/dht/processor.rs