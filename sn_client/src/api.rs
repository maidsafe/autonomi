// Copyright 2023 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::{
    error::{Error, Result},
    Client, ClientEvent, ClientEventsChannel, ClientEventsReceiver, ClientRegister,
};

use bls::{PublicKey, SecretKey, Signature};
use indicatif::ProgressBar;
use libp2p::{
    kad::{Record, RecordKey, K_VALUE},
    Multiaddr,
};
use sn_dbc::{DbcId, SignedSpend};
use sn_networking::{multiaddr_is_global, NetworkEvent, SwarmDriver};
use sn_protocol::{
    error::Error as ProtocolError,
    messages::PaymentProof,
    storage::{
        try_deserialize_record, try_serialize_record, Chunk, ChunkAddress, ChunkWithPayment,
        DbcAddress, RecordHeader, RecordKind, RegisterAddress,
    },
};
use sn_registers::SignedRegister;
use sn_transfers::client_transfers::SpendRequest;
use std::num::NonZeroUsize;
use std::time::Duration;
use tokio::task::spawn;
use tracing::trace;
use xor_name::XorName;

/// The timeout duration for the client to receive any response from the network.
const INACTIVITY_TIMEOUT: std::time::Duration = tokio::time::Duration::from_secs(10);

/// The initial rounds of `get_random` allowing client to fill up the RT.
const INITIAL_GET_RANDOM_ROUNDS: usize = 5;

impl Client {
    /// Instantiate a new client.
    pub async fn new(
        signer: SecretKey,
        peers: Option<Vec<Multiaddr>>,
        req_response_timeout: Option<Duration>,
    ) -> Result<Self> {
        // If any of our contact peers has a global address, we'll assume we're in a global network.
        let local = match peers {
            Some(ref peers) => !peers.iter().any(multiaddr_is_global),
            None => true,
        };

        info!("Startup a client with peers {peers:?} and local {local:?} flag");
        info!("Starting Kad swarm in client mode...");

        let (network, mut network_event_receiver, swarm_driver) =
            SwarmDriver::new_client(local, req_response_timeout)?;
        info!("Client constructed network and swarm_driver");
        let events_channel = ClientEventsChannel::default();

        let client = Self {
            network: network.clone(),
            events_channel,
            signer,
            peers_added: 0,
            progress: Some(Self::setup_connection_progress()),
        };

        // subscribe to our events channel first, so we don't have intermittent
        // errors if it does not exist and we cannot send to it
        // (eg, if PeerAdded happens faster than our events channel is created)
        let mut client_events_rx = client.events_channel();

        let mut must_dial_network = true;

        let mut client_clone = client.clone();

        let _swarm_driver = spawn({
            trace!("Starting up client swarm_driver");
            swarm_driver.run()
        });

        let _event_handler = spawn(async move {
            loop {
                if let Some(peers) = peers.clone() {
                    if must_dial_network {
                        let network = network.clone();
                        let _handle = spawn(async move {
                            for addr in peers {
                                trace!(%addr, "dialing initial peer");

                                if let Err(err) = network.dial(addr.clone()).await {
                                    tracing::error!(%addr, "Failed to dial: {err:?}");
                                };
                            }
                        });

                        must_dial_network = false;
                    }
                }

                match tokio::time::timeout(INACTIVITY_TIMEOUT, network_event_receiver.recv()).await
                {
                    Ok(event) => {
                        let the_event = match event {
                            Some(the_event) => the_event,
                            None => {
                                error!("The `NetworkEvent` channel has been closed");
                                continue;
                            }
                        };
                        if let Err(err) = client_clone.handle_network_event(the_event) {
                            warn!("Error handling network event: {err}");
                        }
                    }
                    Err(_elapse_err) => {
                        info!("Client experienced inactivity when waiting for a network event");
                        if let Err(error) = client_clone
                            .events_channel
                            .broadcast(ClientEvent::InactiveClient(INACTIVITY_TIMEOUT))
                        {
                            error!("Error broadcasting inactive client event: {error}");
                        }
                    }
                }
            }
        });

        let mut is_connected = false;

        loop {
            let mut rng = rand::thread_rng();
            // Carry out 5 rounds of random get to fill up the RT at the beginning.
            for _ in 0..INITIAL_GET_RANDOM_ROUNDS {
                let random_target = ChunkAddress::new(XorName::random(&mut rng));
                let _ = client.get_chunk(random_target).await;
            }
            match client_events_rx.recv().await {
                Ok(ClientEvent::ConnectedToNetwork) => {
                    is_connected = true;
                    info!("Client connected to the Network {is_connected:?}.");
                    break;
                }
                Ok(ClientEvent::InactiveClient(timeout)) => {
                    let random_target = ChunkAddress::new(XorName::random(&mut rng));
                    debug!("No ClientEvent activity in the past {timeout:?}, performing a random get_chunk query to target: {random_target:?}");
                    if is_connected {
                        println!("The client experienced inactivity in the past {timeout:?}.");
                    } else {
                        println!("The client still does not know enough network nodes.");
                    }
                    let _ = client.get_chunk(random_target).await;
                    continue;
                }
                Err(err) => {
                    error!("Unexpected error during client startup {err:?}");
                    println!("Unexpected error during client startup {err:?}");
                    return Err(err);
                }
            }
        }

        Ok(client)
    }

    /// Set up our initial progress bar for network connectivity
    fn setup_connection_progress() -> ProgressBar {
        // Network connection progress bar
        let progress = ProgressBar::new_spinner();
        progress.enable_steady_tick(Duration::from_millis(120));
        progress.set_message("Connecting to The SAFE Network...");
        let new_style = progress.style().tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈🔗");
        progress.set_style(new_style);

        progress.set_message("Connecting to The SAFE Network...");

        progress
    }

    fn handle_network_event(&mut self, event: NetworkEvent) -> Result<()> {
        match event {
            // Clients do not handle requests.
            NetworkEvent::RequestReceived { .. } => {}
            // Clients do not handle responses
            NetworkEvent::ResponseReceived { .. } => {}
            // We do not listen on sockets.
            NetworkEvent::NewListenAddr(_) => {}
            // We are not doing AutoNAT and don't care about our status.
            NetworkEvent::NatStatusChanged(_) => {}
            NetworkEvent::PeerAdded(peer_id) => {
                debug!("PeerAdded: {peer_id}");
                // In case client running in non-local-discovery mode,
                // it may take some time to fill up the RT.
                // To avoid such delay may fail the query with RecordNotFound,
                // wait till certain amount of peers populated into RT
                if let Some(peers_added) = NonZeroUsize::new(self.peers_added) {
                    if peers_added >= K_VALUE {
                        if let Some(progress) = &self.progress {
                            progress.finish_with_message("Connected to the Network");
                            // Remove the progress bar
                            self.progress = None;
                        }

                        self.events_channel
                            .broadcast(ClientEvent::ConnectedToNetwork)?;
                    } else {
                        debug!("{}/{} initial peers found.", self.peers_added, K_VALUE);

                        if let Some(progress) = &self.progress {
                            progress.set_message(format!(
                                "{}/{} initial peers found.",
                                self.peers_added, K_VALUE
                            ));
                        }
                    }
                }
                self.peers_added += 1;
            }
            NetworkEvent::PeerRemoved(_) | NetworkEvent::UnverifiedRecord(_) => {}
        }

        Ok(())
    }

    /// Get the client events channel.
    pub fn events_channel(&self) -> ClientEventsReceiver {
        self.events_channel.subscribe()
    }

    /// Sign the given data
    pub fn sign<T: AsRef<[u8]>>(&self, data: T) -> Signature {
        self.signer.sign(data)
    }

    /// Return the public key of the data signing key
    pub fn signer_pk(&self) -> PublicKey {
        self.signer.public_key()
    }

    /// Get a register from network
    pub async fn get_signed_register_from_network(
        &self,
        address: RegisterAddress,
    ) -> Result<SignedRegister> {
        let record = self
            .network
            .get_record_from_network(RecordKey::new(address.name()))
            .await
            .map_err(|_| ProtocolError::RegisterNotFound(address))?;
        debug!("Got record from the network, {:?}", record.key);
        let header = RecordHeader::from_record(&record)
            .map_err(|_| ProtocolError::RegisterNotFound(address))?;

        if let RecordKind::Register = header.kind {
            let register = try_deserialize_record::<SignedRegister>(&record)
                .map_err(|_| ProtocolError::RegisterNotFound(address))?;
            Ok(register)
        } else {
            error!("RecordKind mismatch while trying to retrieve a signed register");
            Err(Error::Protocol(ProtocolError::RecordKindMismatch(
                RecordKind::Register,
            )))
        }
    }

    /// Retrieve a Register from the network.
    pub async fn get_register(&self, xorname: XorName, tag: u64) -> Result<ClientRegister> {
        info!("Retrieving a Register replica with name {xorname} and tag {tag}");
        ClientRegister::retrieve(self.clone(), xorname, tag).await
    }

    /// Create a new Register on the Network.
    pub async fn create_register(&self, xorname: XorName, tag: u64) -> Result<ClientRegister> {
        info!("Instantiating a new Register replica with name {xorname} and tag {tag}");
        ClientRegister::create_online(self.clone(), xorname, tag).await
    }

    /// Store `Chunk` as a record.
    pub(super) async fn store_chunk(&self, chunk: Chunk, payment: PaymentProof) -> Result<()> {
        info!("Store chunk: {:?}", chunk.address());
        let chunk_with_payment = ChunkWithPayment { chunk, payment };
        let record = Record {
            key: RecordKey::new(chunk_with_payment.chunk.name()),
            value: try_serialize_record(&chunk_with_payment, RecordKind::Chunk)?,
            publisher: None,
            expires: None,
        };

        Ok(self.network.put_record(record).await?)
    }

    /// Retrieve a `Chunk` from the kad network.
    pub(super) async fn get_chunk(&self, address: ChunkAddress) -> Result<Chunk> {
        info!("Getting chunk: {address:?}");
        let xorname = address.name();
        let record = self
            .network
            .get_record_from_network(RecordKey::new(xorname))
            .await?;
        let header = RecordHeader::from_record(&record)?;
        if let RecordKind::Chunk = header.kind {
            let chunk_with_payment: ChunkWithPayment = try_deserialize_record(&record)?;
            Ok(chunk_with_payment.chunk)
        } else {
            Err(ProtocolError::RecordKindMismatch(RecordKind::Chunk).into())
        }
    }

    /// Send a `SpendDbc` request to the network
    pub(crate) async fn network_store_spend(&self, spend: SpendRequest) -> Result<()> {
        let dbc_id = *spend.signed_spend.dbc_id();
        let dbc_addr = DbcAddress::from_dbc_id(&dbc_id);

        trace!("Sending spend {dbc_id:?} to the network via put_record, with addr of {dbc_addr:?}");

        let record = Record {
            key: RecordKey::new(dbc_addr.name()),
            value: try_serialize_record(&[spend.signed_spend], RecordKind::DbcSpend)?,
            publisher: None,
            expires: None,
        };
        Ok(self.network.put_record(record).await?)
    }

    /// Get a dbc spend from network
    pub async fn get_spend_from_network(&self, dbc_id: &DbcId) -> Result<SignedSpend> {
        let address = DbcAddress::from_dbc_id(dbc_id);

        let record = self
            .network
            .get_record_from_network(RecordKey::new(address.name()))
            .await
            .map_err(|err| {
                Error::CouldNotVerifyTransfer(format!(
                    "Can't find record for the dbc_id {dbc_id:?} with error {err:?}"
                ))
            })?;
        debug!("Got record from the network, {:?}", record.key);

        let header = RecordHeader::from_record(&record).map_err(|err| {
            Error::CouldNotVerifyTransfer(format!(
                "Can't parse RecordHeader for the dbc_id {dbc_id:?} with error {err:?}"
            ))
        })?;

        if let RecordKind::DbcSpend = header.kind {
            match try_deserialize_record::<Vec<SignedSpend>>(&record)
                .map_err(|err| {
                    Error::CouldNotVerifyTransfer(format!(
                        "Can't deserialize record for the dbc_id {dbc_id:?} with error {err:?}"
                    ))
                })?
                .as_slice()
            {
                [one, two, ..] => {
                    error!("Found double spend for {address:?}");
                    Err(Error::CouldNotVerifyTransfer(format!(
                "Found double spend for the dbc_id {dbc_id:?}: spend_one {one:?} and spend_two {two:?}"
            )))
                }
                [signed_spend] => {
                    trace!("Spend get for address: {address:?} successful");
                    if dbc_id == signed_spend.dbc_id() {
                        match signed_spend.verify(signed_spend.spent_tx_hash()) {
                            Ok(_) => {
                                trace!("Verified signed spend got from networkfor {dbc_id:?}");
                                Ok(signed_spend.clone())
                            }
                            Err(err) => {
                                warn!("Invalid signed spend got from network for {dbc_id:?}: {err:?}.");
                                Err(Error::CouldNotVerifyTransfer(format!(
                                "Spend failed verifiation for the dbc_id {dbc_id:?} with error {err:?}")))
                            }
                        }
                    } else {
                        warn!("Signed spend ({:?}) got from network mismatched the expected one {dbc_id:?}.", signed_spend.dbc_id());
                        Err(Error::CouldNotVerifyTransfer(format!(
                                "Signed spend ({:?}) got from network mismatched the expected one {dbc_id:?}.", signed_spend.dbc_id())))
                    }
                }
                _ => {
                    trace!("Found no spend for {address:?}");
                    Err(Error::CouldNotVerifyTransfer(format!(
                        "Fetched record shows no spend for dbc {dbc_id:?}."
                    )))
                }
            }
        } else {
            error!("RecordKind mismatch while trying to retrieve a dbc spend");
            Err(ProtocolError::RecordKindMismatch(RecordKind::DbcSpend).into())
        }
    }
}
