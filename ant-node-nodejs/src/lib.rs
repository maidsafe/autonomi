//! Node.js bindings for ant-node.
//!
//! This library provides Node.js bindings for the ant-node library, which
//! provides network spawning capabilities and convergent encryption on file-based data.

use napi::bindgen_prelude::*;
use napi::{Result, Status};
use napi_derive::napi;
use std::path::PathBuf;

// Convert Rust errors to JavaScript errors
fn map_error<E>(err: E) -> napi::Error
where
    E: std::error::Error,
{
    let mut err_str = String::new();
    err_str.push_str(&format!("{err:?}: {err}\n"));
    let mut source = err.source();
    while let Some(err) = source {
        err_str.push_str(&format!(" Caused by: {err:?}: {err}\n"));
        source = err.source();
    }

    napi::Error::new(Status::GenericFailure, err_str)
}

fn try_from_big_int<T: TryFrom<u64>>(value: BigInt, arg: &str) -> Result<T> {
    let (_signed, value, losless) = value.get_u64();
    if losless {
        if let Ok(value) = T::try_from(value) {
            return Ok(value);
        }
    }

    Err(napi::Error::new(
        Status::InvalidArg,
        format!(
            "expected `{arg}` to fit in a {}",
            std::any::type_name::<T>()
        ),
    ))
}

/// A spawner for creating local SAFE networks for testing and development.
#[napi]
pub struct NetworkSpawner(ant_node::spawn::network_spawner::NetworkSpawner);

#[napi]
impl NetworkSpawner {
    #[napi(constructor)]
    pub fn new(args: Option<NetworkSpawnerFields>) -> Self {
        let mut spawner = ant_node::spawn::network_spawner::NetworkSpawner::new();
        if let Some(args) = args {
            if let Some(local) = args.local {
                spawner = spawner.with_local(local);
            }
            if let Some(no_upnp) = args.no_upnp {
                spawner = spawner.with_no_upnp(no_upnp);
            }
            if let Some(root_dir) = args.root_dir {
                spawner = spawner.with_root_dir(root_dir.map(PathBuf::from));
            }
            if let Some(size) = args.size {
                spawner = spawner.with_size(size as usize);
            }
        }

        Self(spawner)
    }
}

#[napi(object)]
pub struct NetworkSpawnerFields {
    // pub evm_network: Option<EvmNetwork>,
    // pub rewards_address: Option<RewardsAddress>,
    pub local: Option<bool>,
    pub no_upnp: Option<bool>,
    pub root_dir: Option<Option<String>>,
    pub size: Option<u32>,
}
