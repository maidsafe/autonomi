// Copyright 2023 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod api;
mod chunks;
mod error;
mod event;
mod file_apis;
mod register;

pub use self::{
    error::Error,
    event::{ClientEvent, ClientEventsReceiver},
    register::{Register, RegisterOffline},
};

use self::event::ClientEventsChannel;

use crate::network::Network;

use bls::SecretKey;

/// Client API implementation to store and get data.
#[derive(Clone)]
pub struct Client {
    network: Network,
    events_channel: ClientEventsChannel,
    signer: SecretKey,
}
