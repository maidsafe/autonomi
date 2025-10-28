// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::ANT_PEERS_ENV;
use crate::BootstrapCacheStore;
use crate::BootstrapConfig;
use crate::ContactsFetcher;
use crate::Result;
use crate::contacts_fetcher::ALPHANET_CONTACTS;
use crate::contacts_fetcher::MAINNET_CONTACTS;
use crate::craft_valid_multiaddr;
use crate::craft_valid_multiaddr_from_str;
use crate::error::Error;
use crate::multiaddr_get_peer_id;
use ant_protocol::version::ALPHANET_ID;
use ant_protocol::version::MAINNET_ID;
use ant_protocol::version::get_network_id;
use libp2p::{
    Multiaddr, PeerId, Swarm,
    core::connection::ConnectedPoint,
    multiaddr::Protocol,
    swarm::{
        DialError, NetworkBehaviour,
        dial_opts::{DialOpts, PeerCondition},
    },
};
use std::collections::{HashSet, VecDeque};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::UnboundedSender;
use url::Url;

/// Timeout for individual fetch operations
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// Minimum number of initial addresses to fetch before returning from `new()`
const MIN_INITIAL_ADDRS: usize = 5;

/// Timeout in seconds to wait for initial addresses during bootstrap initialization
const INITIAL_ADDR_FETCH_TIMEOUT_SECS: u64 = 30;

/// Manages the flow of obtaining bootstrap peer addresses from various sources and also writes to the bootstrap cache.
///
/// The sources are tried in the following order while reading:
/// 1. Environment variable `ANT_PEERS`
/// 2. Command-line provided addresses
/// 3. Bootstrap cache file on disk
/// 4. Network contacts endpoints
///
/// Addresses are returned one at a time via the `next_addr` method.
/// It handles asynchronous fetching from the cache and contacts endpoints,
/// ensuring only one fetch is in progress at a time.
///
/// If no more addresses are available from any source, `next_addr` returns an error.
/// It is expected that the caller will retry `next_addr` later to allow
/// for asynchronous fetches to complete.
#[derive(custom_debug::Debug)]
pub struct Bootstrap {
    cache_store: BootstrapCacheStore,
    addrs: VecDeque<Multiaddr>,
    // The task responsible for syncing the cache, this is aborted on drop.
    #[debug(skip)]
    cache_task: Option<tokio::task::JoinHandle<()>>,
    // fetcher
    cache_pending: bool,
    contacts_progress: Option<ContactsProgress>,
    event_tx: UnboundedSender<FetchEvent>,
    event_rx: UnboundedReceiver<FetchEvent>,
    fetch_in_progress: Option<FetchKind>,
    // dialer
    ongoing_dials: HashSet<Multiaddr>,
    bootstrap_peer_ids: HashSet<PeerId>,
    bootstrap_completed: bool,
}

impl Bootstrap {
    /// Create a new Bootstrap manager with the given configuration.
    /// Use `new_with_preloaded_addrs` to ensure that the struct contains at least MIN_INITIAL_ADDRS addresses immediately.
    ///
    /// Must be called from a tokio runtime context.
    pub fn new(config: BootstrapConfig) -> Result<Self> {
        let contacts_progress = Self::build_contacts_progress(&config)?;

        let mut addrs_queue = VecDeque::new();
        let mut bootstrap_peer_ids = HashSet::new();
        if !config.first {
            if !config.disable_env_peers {
                for addr in Self::fetch_from_env() {
                    Self::push_addr(&mut addrs_queue, &mut bootstrap_peer_ids, addr);
                }
            } else {
                info!("Skipping ANT_PEERS environment variable as per configuration");
            }

            for addr in config.initial_peers.iter() {
                if let Some(addr) = craft_valid_multiaddr(addr) {
                    info!("Adding addr from arguments: {addr}");
                    Self::push_addr(&mut addrs_queue, &mut bootstrap_peer_ids, addr);
                } else {
                    warn!("Invalid multiaddress format from arguments: {addr}");
                }
            }
        }

        let cache_pending = !config.first && !config.disable_cache_reading;
        if !cache_pending {
            info!(
                "Not loading from cache as per configuration (first={}, disable_cache_reading={})",
                config.first, config.disable_cache_reading
            );
        } else {
            info!("Cache loading is enabled - cache will be fetched if needed");
        }
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();

        let cache_store = BootstrapCacheStore::new(config.clone())?;

        let mut bootstrap = Self {
            cache_store: cache_store.clone(),
            addrs: addrs_queue,
            cache_pending,
            contacts_progress,
            event_tx,
            event_rx,
            fetch_in_progress: None,
            ongoing_dials: HashSet::new(),
            bootstrap_peer_ids,
            bootstrap_completed: config.first,
            cache_task: None,
        };

        info!("Cache store is initialized and will sync and flush periodically");
        let cache_task = bootstrap.cache_store.sync_and_flush_periodically();
        bootstrap.cache_task = Some(cache_task);

        if config.first {
            tokio::spawn(async move {
                if let Err(err) = cache_store.write().await {
                    error!("Failed to clear bootstrap cache for first node: {err}");
                } else {
                    info!("Bootstrap cache cleared for first node");
                }
            });
        }

        Ok(bootstrap)
    }

    /// Create a new Bootstrap manager and ensure it has at least MIN_INITIAL_ADDRS addresses preloaded.
    pub async fn new_with_preloaded_addrs(config: BootstrapConfig) -> Result<Self> {
        let mut bootstrap = Self::new(config)?;

        // ensure the initial queue is not empty by fetching from cache/contacts if needed
        //
        // not required for 'first' node
        let mut collected_addrs = Vec::new();
        if bootstrap.addrs.len() < MIN_INITIAL_ADDRS {
            info!("Initial address queue < {MIN_INITIAL_ADDRS}; fetching from cache/contacts");
            let now = std::time::Instant::now();
            loop {
                match bootstrap.next_addr() {
                    Ok(Some(addr)) => {
                        collected_addrs.push(addr);
                        if Self::try_finalize_initial_addrs(
                            &mut bootstrap,
                            &mut collected_addrs,
                            MIN_INITIAL_ADDRS,
                        ) {
                            break;
                        }
                        continue;
                    }
                    Ok(None) => {
                        debug!(
                            "No immediate address available; waiting for async fetch to complete"
                        );
                    }
                    Err(err) => {
                        if Self::try_finalize_initial_addrs(&mut bootstrap, &mut collected_addrs, 1)
                        {
                            break;
                        }
                        warn!("Failed to fetch initial address: {err}");
                        return Err(err);
                    }
                }

                if now.elapsed() > std::time::Duration::from_secs(INITIAL_ADDR_FETCH_TIMEOUT_SECS) {
                    if Self::try_finalize_initial_addrs(&mut bootstrap, &mut collected_addrs, 1) {
                        break;
                    }
                    error!("Timed out waiting for initial addresses. ");
                    return Err(Error::NoBootstrapPeersFound);
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }

        Ok(bootstrap)
    }

    /// Attempts to finalize the initial address collection by extending the bootstrap with collected addresses.
    /// Returns `true` if addresses were successfully added and initialization should complete.
    /// Returns `false` if no addresses are available yet.
    fn try_finalize_initial_addrs(
        bootstrap: &mut Bootstrap,
        collected_addrs: &mut Vec<Multiaddr>,
        min_address: usize,
    ) -> bool {
        if collected_addrs.len() < min_address {
            return false;
        }
        info!(
            "Collected minimum required initial addresses ({}), proceeding with bootstrap.",
            collected_addrs.len()
        );
        bootstrap.extend_with_addrs(std::mem::take(collected_addrs));
        true
    }

    /// Returns the next address from the sources. Returns `Ok(None)` if we are waiting for a source to return more
    /// addresses.
    /// Error if we have exhausted all sources and have no more addresses to return.
    ///
    /// This does not start any dial attempts, it returns the next address to dial.
    /// Use `trigger_bootstrapping_process` to poll the dialing process.
    pub fn next_addr(&mut self) -> Result<Option<Multiaddr>> {
        loop {
            self.process_events();

            if let Some(addr) = self.addrs.pop_front() {
                info!("Returning next bootstrap address: {addr}");
                return Ok(Some(addr));
            }

            if let Some(fetch_kind) = self.fetch_in_progress {
                debug!("Fetch in progress: {fetch_kind:?}; waiting for addresses");
                return Ok(None);
            }

            if self.cache_pending && !matches!(self.fetch_in_progress, Some(FetchKind::Cache)) {
                info!("Triggering cache fetch");
                self.start_cache_fetch()?;
                continue;
            }

            if self.contacts_progress.is_some()
                && !matches!(self.fetch_in_progress, Some(FetchKind::Contacts))
            {
                info!("Triggering contacts fetch");
                self.start_contacts_fetch()?;
                if self.fetch_in_progress.is_some() {
                    return Ok(None);
                }
                continue;
            }

            warn!("No more sources to fetch bootstrap addresses from, and address queue is empty.");
            return Err(Error::NoBootstrapPeersFound);
        }
    }

    /// Check if the address queue is empty without consuming an address.
    /// Returns `Some(true)` if empty, `Some(false)` if not empty, and `None` if still fetching or has cache or contacts
    /// sources pending.
    pub fn is_addr_queue_empty(&self) -> Option<bool> {
        if !self.addrs.is_empty() {
            return Some(false);
        }

        if self.fetch_in_progress.is_some() {
            return None;
        }

        if self.cache_pending {
            return None;
        }

        if self.contacts_progress.is_some() {
            return None;
        }

        Some(true)
    }

    fn process_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                FetchEvent::Cache(addrs) => {
                    if addrs.is_empty() {
                        info!("Cache fetch has completed, but read 0 addresses");
                    } else {
                        info!("Cache fetch has completed. Got {} addresses", addrs.len());
                        self.extend_with_addrs(addrs);
                    }
                }
                FetchEvent::Contacts(addrs) => {
                    info!(
                        "Contacts fetch has completed. Got {} addresses",
                        addrs.len()
                    );
                    self.extend_with_addrs(addrs);
                    if self
                        .contacts_progress
                        .as_ref()
                        .is_none_or(ContactsProgress::is_empty)
                    {
                        self.contacts_progress = None;
                    }
                }
            }

            self.fetch_in_progress = None;
        }
    }

    fn extend_with_addrs(&mut self, addrs: Vec<Multiaddr>) {
        if addrs.is_empty() {
            return;
        }
        for addr in addrs {
            Self::push_addr(&mut self.addrs, &mut self.bootstrap_peer_ids, addr);
        }
    }

    fn push_addr(queue: &mut VecDeque<Multiaddr>, peer_ids: &mut HashSet<PeerId>, addr: Multiaddr) {
        if let Some(peer_id) = multiaddr_get_peer_id(&addr) {
            peer_ids.insert(peer_id);
        }
        queue.push_back(addr);
    }

    fn pop_p2p(addr: &mut Multiaddr) -> Option<PeerId> {
        if let Some(Protocol::P2p(peer_id)) = addr.iter().last() {
            let _ = addr.pop();
            Some(peer_id)
        } else {
            None
        }
    }

    /// Return true if the bootstrapping process has completed or if we have run out of addresses, otherwise false.
    fn has_bootstrap_completed(&self, contacted_peers: usize) -> bool {
        if self.bootstrap_completed {
            debug!("Initial bootstrap process has already completed successfully.");
            return true;
        }

        if contacted_peers
            >= self
                .cache_store
                .config()
                .max_contacted_peers_before_termination
        {
            info!(
                "Initial bootstrap process completed successfully. We have {contacted_peers} peers in the routing table."
            );
            return true;
        }

        // If addresses are empty AND no fetch is in progress AND no contacts endpoints are left to try, then
        // we have exhausted all sources.
        if self.addrs.is_empty()
            && !self.cache_pending
            && self.contacts_progress.is_none()
            && self.fetch_in_progress.is_none()
        {
            info!(
                "We have {contacted_peers} peers in RT, but no more addresses to dial. Stopping initial bootstrap."
            );
            return true;
        }

        false
    }

    /// Manages the bootstrapping process by attempting to dial peers from the available addresses.
    ///
    /// Returns `true` if the bootstrapping process has ended (either due to successful connection or due to exhaustion
    /// of addresses), otherwise `false`.
    pub fn trigger_bootstrapping_process<B: NetworkBehaviour>(
        &mut self,
        swarm: &mut Swarm<B>,
        contacted_peers: usize,
    ) -> bool {
        if self.has_bootstrap_completed(contacted_peers) {
            self.bootstrap_completed = true;
            self.addrs.clear();
            self.ongoing_dials.clear();
            return true;
        }

        while self.ongoing_dials.len() < self.cache_store.config().max_concurrent_dials {
            match self.next_addr() {
                Ok(Some(mut addr)) => {
                    let addr_clone = addr.clone();
                    let peer_id = Self::pop_p2p(&mut addr);

                    let opts = match peer_id {
                        Some(peer_id) => DialOpts::peer_id(peer_id)
                            .condition(PeerCondition::NotDialing)
                            .addresses(vec![addr])
                            .build(),
                        None => DialOpts::unknown_peer_id().address(addr).build(),
                    };

                    info!("Trying to dial peer with address: {addr_clone}");

                    match swarm.dial(opts) {
                        Ok(()) => {
                            info!(
                                "Dial attempt initiated for peer with address: {addr_clone}. Ongoing dial attempts: {}",
                                self.ongoing_dials.len() + 1
                            );
                            let _ = self.ongoing_dials.insert(addr_clone);
                        }
                        Err(err) => match err {
                            DialError::LocalPeerId { .. } => {
                                warn!(
                                    "Failed to dial peer with address: {addr_clone}. This is our own peer ID. Dialing the next peer"
                                );
                            }
                            DialError::NoAddresses => {
                                error!(
                                    "Failed to dial peer with address: {addr_clone}. No addresses found. Dialing the next peer"
                                );
                            }
                            DialError::DialPeerConditionFalse(_) => {
                                warn!(
                                    "We are already dialing the peer with address: {addr_clone}. Dialing the next peer. This error is harmless."
                                );
                            }
                            DialError::Aborted => {
                                error!(
                                    "Pending connection attempt has been aborted for {addr_clone}. Dialing the next peer."
                                );
                            }
                            DialError::WrongPeerId { obtained, .. } => {
                                error!(
                                    "The peer identity obtained on the connection did not match the one that was expected. Obtained: {obtained}. Dialing the next peer."
                                );
                            }
                            DialError::Denied { cause } => {
                                error!(
                                    "The dialing attempt was denied by the remote peer. Cause: {cause}. Dialing the next peer."
                                );
                            }
                            DialError::Transport(items) => {
                                error!(
                                    "Failed to dial peer with address: {addr_clone}. Transport error: {items:?}. Dialing the next peer."
                                );
                            }
                        },
                    }
                }
                Ok(None) => {
                    debug!("Waiting for additional bootstrap addresses before continuing to dial");
                    break;
                }
                Err(Error::NoBootstrapPeersFound) => {
                    info!("No more bootstrap peers available to dial.");
                    self.bootstrap_completed = true;
                    break;
                }
                Err(err) => {
                    warn!("Failed to obtain next bootstrap address: {err}");
                    break;
                }
            }
        }
        self.bootstrap_completed
    }

    pub fn on_connection_established(&mut self, peer_id: &PeerId, endpoint: &ConnectedPoint) {
        if self.bootstrap_completed {
            return;
        }

        if let ConnectedPoint::Dialer { address, .. } = endpoint
            && !self.ongoing_dials.remove(address)
        {
            self.ongoing_dials
                .retain(|addr| match multiaddr_get_peer_id(addr) {
                    Some(id) => id != *peer_id,
                    None => true,
                });
        }
    }

    pub fn on_outgoing_connection_error(&mut self, peer_id: Option<PeerId>) {
        if self.bootstrap_completed {
            return;
        }

        match peer_id {
            Some(peer_id) => {
                self.ongoing_dials.retain(|addr| {
                    if let Some(id) = multiaddr_get_peer_id(addr) {
                        id != peer_id
                    } else {
                        true
                    }
                });
            }
            None => {
                // we are left with no option but to remove all the addresses from the ongoing dials that
                // do not have a peer ID.
                self.ongoing_dials
                    .retain(|addr| multiaddr_get_peer_id(addr).is_some());
            }
        }
    }

    pub fn is_bootstrap_peer(&self, peer_id: &PeerId) -> bool {
        self.bootstrap_peer_ids.contains(peer_id)
    }

    pub fn has_terminated(&self) -> bool {
        self.bootstrap_completed
    }

    fn start_cache_fetch(&mut self) -> Result<()> {
        if matches!(self.fetch_in_progress, Some(FetchKind::Cache)) {
            error!("Cache fetch already in progress, not starting another");
            return Ok(());
        }

        self.cache_pending = false;
        let config = self.cache_store.config().clone();
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            let fetch_result = tokio::time::timeout(FETCH_TIMEOUT, async move {
                tokio::task::spawn_blocking(move || BootstrapCacheStore::load_cache_data(&config))
                    .await
            })
            .await;

            let addrs = match fetch_result {
                Ok(spawn_result) => match spawn_result {
                    Ok(Ok(cache_data)) => cache_data.get_all_addrs().cloned().collect(),
                    Ok(Err(err)) => {
                        warn!("Failed to load cache data: {err}");
                        Vec::new()
                    }
                    Err(err) => {
                        warn!("Cache fetch task failed to join: {err}");
                        Vec::new()
                    }
                },
                Err(_) => {
                    warn!(
                        "Cache fetch timed out after {} seconds",
                        FETCH_TIMEOUT.as_secs()
                    );
                    Vec::new()
                }
            };

            info!(
                "Bootstrap cache loaded from disk with {} addresses",
                addrs.len()
            );
            if let Err(err) = event_tx.send(FetchEvent::Cache(addrs)) {
                error!("Failed to send cache fetch event: {err:?}");
            }
        });

        self.fetch_in_progress = Some(FetchKind::Cache);

        Ok(())
    }

    fn start_contacts_fetch(&mut self) -> Result<()> {
        if matches!(self.fetch_in_progress, Some(FetchKind::Contacts)) {
            error!("Contacts fetch already in progress, not starting another");
            return Ok(());
        }

        let Some(progress) = self.contacts_progress.as_mut() else {
            info!("No contacts progress available");
            return Ok(());
        };

        let Some(endpoint) = progress.next_endpoint() else {
            info!("No more contacts endpoints to try");
            self.contacts_progress = None;
            return Ok(());
        };

        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            let fetch_result = tokio::time::timeout(FETCH_TIMEOUT, async {
                let fetcher = ContactsFetcher::with_endpoints(vec![endpoint.clone()])?;
                fetcher.fetch_bootstrap_addresses().await
            })
            .await;

            let addrs = match fetch_result {
                Ok(Ok(addrs)) => addrs,
                Ok(Err(err)) => {
                    warn!("Failed to fetch contacts from {endpoint}: {err}");
                    Vec::new()
                }
                Err(_) => {
                    warn!(
                        "Contacts fetch from {endpoint} timed out after {} seconds",
                        FETCH_TIMEOUT.as_secs()
                    );
                    Vec::new()
                }
            };

            info!(
                "Contacts fetch completed from endpoint {endpoint:?} with {} addresses",
                addrs.len()
            );
            if let Err(err) = event_tx.send(FetchEvent::Contacts(addrs)) {
                error!("Failed to send contacts fetch event: {err:?}");
            }
        });

        self.fetch_in_progress = Some(FetchKind::Contacts);

        Ok(())
    }

    fn build_contacts_progress(config: &BootstrapConfig) -> Result<Option<ContactsProgress>> {
        if config.first {
            info!("First node in network; not fetching contacts");
            return Ok(None);
        }

        if config.local {
            info!("Local network configuration; skipping contacts endpoints");
            return Ok(None);
        }

        if !config.network_contacts_url.is_empty() {
            let endpoints = config
                .network_contacts_url
                .iter()
                .map(|endpoint| endpoint.parse::<Url>().map_err(|_| Error::FailedToParseUrl))
                .collect::<Result<Vec<_>>>()?;
            info!("Using provided contacts endpoints: {endpoints:?}");
            return Ok(ContactsProgress::new(endpoints));
        }

        match get_network_id() {
            id if id == MAINNET_ID => {
                info!("Using built-in mainnet contacts endpoints");
                Ok(ContactsProgress::from_static(MAINNET_CONTACTS))
            }

            id if id == ALPHANET_ID => {
                info!("Using built-in alphanet contacts endpoints");
                Ok(ContactsProgress::from_static(ALPHANET_CONTACTS))
            }
            _ => Ok(None),
        }
    }

    pub fn fetch_from_env() -> Vec<Multiaddr> {
        let mut bootstrap_addresses = Vec::new();
        // Read from ANT_PEERS environment variable if present
        if let Ok(addrs) = std::env::var(ANT_PEERS_ENV) {
            for addr_str in addrs.split(',') {
                if let Some(addr) = craft_valid_multiaddr_from_str(addr_str) {
                    info!("Adding addr from environment variable: {addr}");
                    bootstrap_addresses.push(addr);
                } else {
                    warn!("Invalid multiaddress format from environment variable: {addr_str}");
                }
            }
        }
        bootstrap_addresses
    }

    pub fn cache_store_mut(&mut self) -> &mut BootstrapCacheStore {
        &mut self.cache_store
    }

    pub fn cache_store(&self) -> &BootstrapCacheStore {
        &self.cache_store
    }
}

impl Drop for Bootstrap {
    fn drop(&mut self) {
        if let Some(cache_sync_task) = self.cache_task.take() {
            cache_sync_task.abort();
        }
    }
}

#[derive(Debug)]
struct ContactsProgress {
    remaining: VecDeque<Url>,
}

enum FetchEvent {
    Cache(Vec<Multiaddr>),
    Contacts(Vec<Multiaddr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FetchKind {
    Cache,
    Contacts,
}

impl ContactsProgress {
    fn new(urls: Vec<Url>) -> Option<Self> {
        if urls.is_empty() {
            None
        } else {
            Some(Self {
                remaining: VecDeque::from(urls),
            })
        }
    }

    fn from_static(urls: &[&str]) -> Option<Self> {
        let mut parsed = Vec::new();
        for url in urls {
            match url.parse::<Url>() {
                Ok(parsed_url) => parsed.push(parsed_url),
                Err(err) => {
                    warn!("Failed to parse static contacts URL {url}: {err}");
                }
            }
        }
        Self::new(parsed)
    }

    fn next_endpoint(&mut self) -> Option<Url> {
        self.remaining.pop_front()
    }

    fn is_empty(&self) -> bool {
        self.remaining.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        InitialPeersConfig,
        cache_store::{BootstrapCacheStore, cache_data_v1::CacheData},
        multiaddr_get_peer_id,
    };
    use libp2p::Multiaddr;
    use std::collections::HashSet;
    use std::sync::{Arc, OnceLock};
    use std::time::{Duration, Instant};
    use tempfile::TempDir;
    use tokio::sync::{Mutex, OwnedMutexGuard};
    use tokio::time::sleep;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    async fn env_lock() -> OwnedMutexGuard<()> {
        static ENV_MUTEX: OnceLock<Arc<Mutex<()>>> = OnceLock::new();
        Arc::clone(ENV_MUTEX.get_or_init(|| Arc::new(Mutex::new(()))))
            .lock_owned()
            .await
    }

    #[allow(unsafe_code)]
    fn set_env_var(key: &str, value: &str) {
        unsafe {
            std::env::set_var(key, value);
        }
    }

    #[allow(unsafe_code)]
    fn remove_env_var(key: &str) {
        unsafe {
            std::env::remove_var(key);
        }
    }

    async fn expect_next_addr(flow: &mut Bootstrap) -> Result<Multiaddr> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match flow.next_addr() {
                Ok(Some(addr)) => return Ok(addr),
                Ok(None) => {
                    if Instant::now() >= deadline {
                        panic!("Timed out waiting for next address");
                    }
                    sleep(Duration::from_millis(5)).await;
                }
                Err(err) => return Err(err),
            }
        }
    }

    async fn expect_err(flow: &mut Bootstrap) -> Error {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match flow.next_addr() {
                Ok(Some(addr)) => panic!("unexpected address returned: {addr}"),
                Ok(None) => {
                    if Instant::now() >= deadline {
                        panic!("Timed out waiting for error from flow");
                    }
                    sleep(Duration::from_millis(5)).await;
                }
                Err(err) => return err,
            }
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn address_sources_follow_priority_env_cli_cache_contacts() {
        let _env_guard = env_lock().await;
        remove_env_var(ANT_PEERS_ENV);

        let env_addr: Multiaddr =
            "/ip4/10.1.0.1/udp/1300/quic-v1/p2p/12D3KooWBbtXX6gY5xPD7NzNGGbj2428NJQ4HNvvBnSE5g4R7Pkf"
                .parse()
                .unwrap();
        let cli_addr: Multiaddr =
            "/ip4/10.1.0.2/udp/1301/quic-v1/p2p/12D3KooWCRfYwq9c3PAXo5cTp3snq72Knqukcec4c9qT1AMyvMPd"
                .parse()
                .unwrap();
        set_env_var(ANT_PEERS_ENV, &env_addr.to_string());

        let cache_addr_one: Multiaddr =
            "/ip4/10.1.0.3/udp/1302/quic-v1/p2p/12D3KooWMmKJcWUP9UqP4g1n3LH1htkvSUStn1aQGQxGc1dQcYxA"
                .parse()
                .unwrap();
        let cache_addr_two: Multiaddr =
            "/ip4/10.1.0.4/udp/1303/quic-v1/p2p/12D3KooWA4b4T6Dz4RUtqnYDEBt3eGkqRykGGBqBP3ZiZsaAJ2jp"
                .parse()
                .unwrap();

        let temp_dir = TempDir::new().unwrap();
        let file_name = BootstrapCacheStore::cache_file_name(false);
        let mut cache_data = CacheData::default();
        cache_data.add_peer(
            multiaddr_get_peer_id(&cache_addr_one).unwrap(),
            std::iter::once(&cache_addr_one),
            3,
            10,
        );
        cache_data.add_peer(
            multiaddr_get_peer_id(&cache_addr_two).unwrap(),
            std::iter::once(&cache_addr_two),
            3,
            10,
        );
        cache_data
            .write_to_file(temp_dir.path(), &file_name)
            .unwrap();

        let mock_server = MockServer::start().await;
        let contact_one: Multiaddr =
            "/ip4/10.1.0.5/udp/1304/quic-v1/p2p/12D3KooWQGyiCWkmKvgFVF1PsvBLnBxG29BAsoAhH4m6qjUpBAk1"
                .parse()
                .unwrap();
        let contact_two: Multiaddr =
            "/ip4/10.1.0.6/udp/1305/quic-v1/p2p/12D3KooWGpMibW82dManEXZDV4SSQSSHqzTeWY5Avzkdx6yrosNG"
                .parse()
                .unwrap();

        Mock::given(method("GET"))
            .and(path("/contacts_one"))
            .respond_with(ResponseTemplate::new(200).set_body_string(contact_one.to_string()))
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/contacts_two"))
            .respond_with(ResponseTemplate::new(200).set_body_string(contact_two.to_string()))
            .expect(1)
            .mount(&mock_server)
            .await;

        let config = InitialPeersConfig {
            addrs: vec![cli_addr.clone()],
            bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
            network_contacts_url: vec![
                format!("{}/contacts_one", mock_server.uri()),
                format!("{}/contacts_two", mock_server.uri()),
            ],
            ..Default::default()
        };
        let config = BootstrapConfig::try_from(&config).expect("Failed to create BootstrapConfig");
        let mut flow = Bootstrap::new(config).unwrap();

        let initial_results = vec![
            expect_next_addr(&mut flow).await.unwrap(),
            expect_next_addr(&mut flow).await.unwrap(),
        ];
        let initial_set: HashSet<_> = initial_results.into_iter().collect();
        let expected_initial: HashSet<_> =
            [env_addr.clone(), cli_addr.clone()].into_iter().collect();
        assert_eq!(initial_set, expected_initial);

        let cache_results = vec![
            expect_next_addr(&mut flow).await.unwrap(),
            expect_next_addr(&mut flow).await.unwrap(),
        ];
        let cache_set: HashSet<_> = cache_results.into_iter().collect();
        let expected_cache: HashSet<_> = [cache_addr_one.clone(), cache_addr_two.clone()]
            .into_iter()
            .collect();
        assert_eq!(cache_set, expected_cache);

        let contact_first = expect_next_addr(&mut flow).await.unwrap();
        assert_eq!(contact_first, contact_one);

        let contact_second = expect_next_addr(&mut flow).await.unwrap();
        assert_eq!(contact_second, contact_two);

        let err = expect_err(&mut flow).await;
        assert!(matches!(err, Error::NoBootstrapPeersFound));

        let requests = mock_server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].url.path(), "/contacts_one");
        assert_eq!(requests[1].url.path(), "/contacts_two");

        remove_env_var(ANT_PEERS_ENV);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn env_variable_parses_comma_separated_multiaddrs() {
        let _env_guard = env_lock().await;
        set_env_var(
            ANT_PEERS_ENV,
            "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE,\
/ip4/127.0.0.2/udp/8081/quic-v1/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5",
        );

        let parsed = Bootstrap::fetch_from_env();
        remove_env_var(ANT_PEERS_ENV);

        assert_eq!(parsed.len(), 2);
        let parsed_set: std::collections::HashSet<_> =
            parsed.into_iter().map(|addr| addr.to_string()).collect();
        let expected = std::collections::HashSet::from([
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .to_string(),
        "/ip4/127.0.0.2/udp/8081/quic-v1/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5"
            .to_string(),
    ]);
        assert_eq!(parsed_set, expected);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn cache_loads_varying_quantities_zero_few_many() {
        let _env_guard = env_lock().await;
        let temp_dir = TempDir::new().unwrap();
        let file_name = BootstrapCacheStore::cache_file_name(true);

        let mut cache_data_one = CacheData::default();
        let cache_addr_one: Multiaddr =
            "/ip4/127.0.0.1/tcp/1202/p2p/12D3KooWKGt8umjJQ4sDzFXo2UcHBaF33rqmFcWtXM6nbryL5G4J"
                .parse()
                .unwrap();
        cache_data_one.add_peer(
            multiaddr_get_peer_id(&cache_addr_one).unwrap(),
            std::iter::once(&cache_addr_one),
            3,
            10,
        );
        cache_data_one
            .write_to_file(temp_dir.path(), &file_name)
            .unwrap();

        let config = InitialPeersConfig {
            local: true,
            bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
            ..Default::default()
        };
        let mut config =
            BootstrapConfig::try_from(&config).expect("Failed to create BootstrapConfig");
        config.disable_env_peers = true;
        let mut flow = Bootstrap::new(config).unwrap();

        let got = expect_next_addr(&mut flow).await.unwrap();
        assert_eq!(got, cache_addr_one);

        let err = expect_err(&mut flow).await;
        assert!(matches!(err, Error::NoBootstrapPeersFound));

        let temp_dir_few = TempDir::new().unwrap();
        let mut cache_data_few = CacheData::default();
        for i in 0..MIN_INITIAL_ADDRS - 2 {
            let peer_id = libp2p::PeerId::random();
            let addr: Multiaddr =
                format!("/ip4/10.4.{}.1/udp/{}/quic-v1/p2p/{peer_id}", i, 4000 + i)
                    .parse()
                    .unwrap();
            cache_data_few.add_peer(peer_id, std::iter::once(&addr), 3, 10);
        }
        cache_data_few
            .write_to_file(temp_dir_few.path(), &file_name)
            .unwrap();

        let config_few = InitialPeersConfig {
            local: true,
            bootstrap_cache_dir: Some(temp_dir_few.path().to_path_buf()),
            ..Default::default()
        };
        let mut config_few =
            BootstrapConfig::try_from(&config_few).expect("Failed to create BootstrapConfig");
        config_few.disable_env_peers = true;

        let result_few = Bootstrap::new_with_preloaded_addrs(config_few).await;
        assert!(
            result_few.is_ok(),
            "Should succeed with few contacts (< 5 but > 0)"
        );

        let mut flow_few = result_few.unwrap();
        let mut count = 0;
        while let Ok(Some(_addr)) = flow_few.next_addr() {
            count += 1;
            if count >= MIN_INITIAL_ADDRS {
                break;
            }
        }
        assert_eq!(
            count,
            MIN_INITIAL_ADDRS - 2,
            "Should have exactly 3 contacts"
        );

        let temp_dir_many = TempDir::new().unwrap();
        let mut cache_data_many = CacheData::default();
        for i in 0..MIN_INITIAL_ADDRS + 1 {
            let peer_id = libp2p::PeerId::random();
            let addr: Multiaddr =
                format!("/ip4/10.3.{}.1/udp/{}/quic-v1/p2p/{peer_id}", i, 3000 + i)
                    .parse()
                    .unwrap();
            cache_data_many.add_peer(peer_id, std::iter::once(&addr), 3, 10);
        }
        cache_data_many
            .write_to_file(temp_dir_many.path(), &file_name)
            .unwrap();

        let config_many = InitialPeersConfig {
            local: true,
            bootstrap_cache_dir: Some(temp_dir_many.path().to_path_buf()),
            ..Default::default()
        };
        let mut config_many =
            BootstrapConfig::try_from(&config_many).expect("Failed to create BootstrapConfig");
        config_many.disable_env_peers = true;

        let result_many = Bootstrap::new_with_preloaded_addrs(config_many).await;
        assert!(
            result_many.is_ok(),
            "Should successfully initialize with many contacts in cache"
        );

        let mut flow_many = result_many.unwrap();
        let mut count_many = 0;
        while let Ok(Some(_addr)) = flow_many.next_addr() {
            count_many += 1;
            if count_many >= MIN_INITIAL_ADDRS {
                break;
            }
        }
        assert!(
            count_many > 0,
            "Should have loaded contacts from cache, got {count_many}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn contacts_fetched_sequentially_from_multiple_endpoints() {
        let _env_guard = env_lock().await;

        let mock_server = MockServer::start().await;

        let contact_one: Multiaddr =
        "/ip4/192.168.0.1/udp/1203/quic-v1/p2p/12D3KooWPULWT1qXJ1jzYVtQocvKXgcv6U7Pp3ui3EB7mN8hXAsP"
            .parse()
            .unwrap();
        let contact_two: Multiaddr =
        "/ip4/192.168.0.2/udp/1204/quic-v1/p2p/12D3KooWPsMPaEjaWjW6GWpAne6LYcwBQEJfnDbhQFNs6ytzmBn5"
            .parse()
            .unwrap();

        Mock::given(method("GET"))
            .and(path("/first"))
            .respond_with(ResponseTemplate::new(200).set_body_string(contact_one.to_string()))
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/second"))
            .respond_with(ResponseTemplate::new(200).set_body_string(contact_two.to_string()))
            .expect(1)
            .mount(&mock_server)
            .await;

        let config = InitialPeersConfig {
            ignore_cache: true,
            network_contacts_url: vec![
                format!("{}/first", mock_server.uri()),
                format!("{}/second", mock_server.uri()),
            ],
            ..Default::default()
        };
        let mut config =
            BootstrapConfig::try_from(&config).expect("Failed to create BootstrapConfig");
        config.disable_env_peers = true;
        let mut flow = Bootstrap::new(config).unwrap();

        let first = expect_next_addr(&mut flow).await.unwrap();
        assert_eq!(first, contact_one);

        let second = expect_next_addr(&mut flow).await.unwrap();
        assert_eq!(second, contact_two);

        let err = expect_err(&mut flow).await;
        assert!(matches!(err, Error::NoBootstrapPeersFound));

        let requests = mock_server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].url.path(), "/first");
        assert_eq!(requests[1].url.path(), "/second");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn first_node_skips_sources_clears_cache_and_terminates() {
        let _env_guard = env_lock().await;

        let peer_id = libp2p::PeerId::random();
        let cache_addr: Multiaddr = format!("/ip4/10.2.0.1/udp/2007/quic-v1/p2p/{peer_id}")
            .parse()
            .unwrap();

        let temp_dir = TempDir::new().unwrap();
        let file_name = BootstrapCacheStore::cache_file_name(false);

        let mut cache_data = CacheData::default();
        cache_data.add_peer(peer_id, std::iter::once(&cache_addr), 3, 10);
        cache_data
            .write_to_file(temp_dir.path(), &file_name)
            .unwrap();

        let file_path = temp_dir.path().join(format!(
            "version_{}/{}",
            CacheData::CACHE_DATA_VERSION,
            file_name
        ));

        let contents_before = std::fs::read_to_string(&file_path).unwrap();
        assert!(
            contents_before.contains(&cache_addr.to_string()),
            "Cache should contain the address before initialization"
        );

        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
        .and(path("/peers"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE",
        ))
        .expect(0)
        .mount(&mock_server)
        .await;

        let config = InitialPeersConfig {
            first: true,
            addrs: vec![
            "/ip4/127.0.0.2/udp/8081/quic-v1/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5"
                .parse()
                .unwrap(),
            ],
            network_contacts_url: vec![format!("{}/peers", mock_server.uri())],
            bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
            ..Default::default()
        };
        let mut config =
            BootstrapConfig::try_from(&config).expect("Failed to create BootstrapConfig");
        config.disable_env_peers = true;
        let mut flow = Bootstrap::new(config).unwrap();

        let err = expect_err(&mut flow).await;
        assert!(matches!(err, Error::NoBootstrapPeersFound));
        assert!(
            mock_server.received_requests().await.unwrap().is_empty(),
            "first flag should prevent contact fetches"
        );

        assert!(flow.has_terminated());

        tokio::time::sleep(Duration::from_millis(100)).await;

        let contents_after = std::fs::read_to_string(&file_path).unwrap();
        assert!(
            !contents_after.contains(&cache_addr.to_string()),
            "Cache should be cleared for first node"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn preload_respects_disabled_source_flags() {
        let peer_id = libp2p::PeerId::random();
        let env_addr: Multiaddr = format!("/ip4/10.2.0.1/udp/2000/quic-v1/p2p/{peer_id}")
            .parse()
            .unwrap();

        let _env_guard = env_lock().await;
        set_env_var(ANT_PEERS_ENV, &env_addr.to_string());

        let temp_dir = TempDir::new().unwrap();

        let config = InitialPeersConfig {
            local: true,
            ignore_cache: true,
            bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
            ..Default::default()
        };
        let mut config =
            BootstrapConfig::try_from(&config).expect("Failed to create BootstrapConfig");
        config.disable_env_peers = true;

        let result = Bootstrap::new_with_preloaded_addrs(config).await;
        assert!(
            result.is_err(),
            "Should error when env peers are disabled and no other sources available"
        );
        assert!(matches!(result.unwrap_err(), Error::NoBootstrapPeersFound));

        let peer_id2 = libp2p::PeerId::random();
        let cache_addr: Multiaddr = format!("/ip4/10.2.0.1/udp/2001/quic-v1/p2p/{peer_id2}")
            .parse()
            .unwrap();

        let temp_dir2 = TempDir::new().unwrap();
        let file_name = BootstrapCacheStore::cache_file_name(true);

        let mut cache_data = CacheData::default();
        cache_data.add_peer(peer_id2, std::iter::once(&cache_addr), 3, 10);
        cache_data
            .write_to_file(temp_dir2.path(), &file_name)
            .unwrap();

        let config2 = InitialPeersConfig {
            local: true,
            bootstrap_cache_dir: Some(temp_dir2.path().to_path_buf()),
            ..Default::default()
        };
        let mut config2 =
            BootstrapConfig::try_from(&config2).expect("Failed to create BootstrapConfig");
        config2.disable_env_peers = true;
        config2.disable_cache_reading = true;

        let result2 = Bootstrap::new_with_preloaded_addrs(config2).await;
        assert!(
            result2.is_err(),
            "Should error when cache reading is disabled and no other sources available"
        );
        assert!(matches!(result2.unwrap_err(), Error::NoBootstrapPeersFound));

        remove_env_var(ANT_PEERS_ENV);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn preload_timeout_when_no_addresses_available() {
        let _env_guard = env_lock().await;

        let temp_dir = TempDir::new().unwrap();
        let file_name = BootstrapCacheStore::cache_file_name(true);
        let cache_data = CacheData::default();
        cache_data
            .write_to_file(temp_dir.path(), &file_name)
            .unwrap();

        let config = InitialPeersConfig {
            local: true,
            bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
            ..Default::default()
        };
        let mut config =
            BootstrapConfig::try_from(&config).expect("Failed to create BootstrapConfig");
        config.disable_env_peers = true;

        let result = Bootstrap::new_with_preloaded_addrs(config).await;

        assert!(
            result.is_err(),
            "Should error when no addresses are available from any source"
        );
        assert!(matches!(result.unwrap_err(), Error::NoBootstrapPeersFound));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn contacts_disabled_by_local_flag() {
        let _env_guard = env_lock().await;

        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/should-not-be-called"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE",
            ))
            .expect(0)
            .mount(&mock_server)
            .await;

        let temp_dir = TempDir::new().unwrap();

        let peer_id = libp2p::PeerId::random();
        let addr: Multiaddr = format!("/ip4/10.2.0.1/udp/2006/quic-v1/p2p/{peer_id}")
            .parse()
            .unwrap();

        let config = InitialPeersConfig {
            local: true,
            ignore_cache: true,
            bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
            network_contacts_url: vec![format!("{}/should-not-be-called", mock_server.uri())],
            addrs: vec![addr],
            ..Default::default()
        };
        let mut config =
            BootstrapConfig::try_from(&config).expect("Failed to create BootstrapConfig");
        config.disable_env_peers = true;

        let addr_from_config = config.initial_peers[0].clone();
        let mut flow = Bootstrap::new_with_preloaded_addrs(config).await.unwrap();

        let first = expect_next_addr(&mut flow).await.unwrap();
        assert_eq!(
            first, addr_from_config,
            "Should get the address from config"
        );

        let err = expect_err(&mut flow).await;
        assert!(matches!(err, Error::NoBootstrapPeersFound));

        assert!(
            mock_server.received_requests().await.unwrap().is_empty(),
            "local flag should prevent contact fetches"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn contacts_fetch_failures_handled_gracefully() {
        let _env_guard = env_lock().await;

        let temp_dir = TempDir::new().unwrap();
        let file_name = BootstrapCacheStore::cache_file_name(false);
        let cache_data = CacheData::default();
        cache_data
            .write_to_file(temp_dir.path(), &file_name)
            .unwrap();

        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/failing-endpoint"))
            .respond_with(ResponseTemplate::new(500))
            .expect(1..)
            .mount(&mock_server)
            .await;

        let config = InitialPeersConfig {
            bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
            network_contacts_url: vec![format!("{}/failing-endpoint", mock_server.uri())],
            ..Default::default()
        };
        let mut config =
            BootstrapConfig::try_from(&config).expect("Failed to create BootstrapConfig");
        config.disable_env_peers = true;

        let result = Bootstrap::new_with_preloaded_addrs(config).await;

        assert!(
            result.is_err(),
            "Should error when all sources fail and no contacts are available"
        );
        assert!(matches!(result.unwrap_err(), Error::NoBootstrapPeersFound));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn invalid_multiaddrs_filtered_from_initial_peers() {
        let _env_guard = env_lock().await;

        let peer_id = libp2p::PeerId::random();
        let valid_addr: Multiaddr = format!("/ip4/10.2.0.1/udp/2005/quic-v1/p2p/{peer_id}")
            .parse()
            .unwrap();
        let invalid_addr: Multiaddr = "/ip4/127.0.0.1/tcp/1234".parse().unwrap();

        let temp_dir = TempDir::new().unwrap();

        let config = InitialPeersConfig {
            local: true,
            ignore_cache: true,
            bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
            addrs: vec![valid_addr.clone()],
            ..Default::default()
        };
        let mut config =
            BootstrapConfig::try_from(&config).expect("Failed to create BootstrapConfig");
        config.disable_env_peers = true;

        config.initial_peers.push(invalid_addr);

        let mut flow = Bootstrap::new_with_preloaded_addrs(config).await.unwrap();

        let first = expect_next_addr(&mut flow).await.unwrap();
        assert_eq!(first, valid_addr, "Should get the valid address");

        let err = expect_err(&mut flow).await;
        assert!(
            matches!(err, Error::NoBootstrapPeersFound),
            "Should not find any more peers after valid one (invalid addr was filtered)"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn bootstrap_peer_ids_tracked_from_all_sources() {
        let env_peer_id = libp2p::PeerId::random();
        let cli_peer_id = libp2p::PeerId::random();
        let env_addr: Multiaddr = format!("/ip4/10.2.0.1/udp/2003/quic-v1/p2p/{env_peer_id}")
            .parse()
            .unwrap();
        let cli_addr: Multiaddr = format!("/ip4/10.2.0.2/udp/2004/quic-v1/p2p/{cli_peer_id}")
            .parse()
            .unwrap();

        let _env_guard = env_lock().await;
        set_env_var(ANT_PEERS_ENV, &env_addr.to_string());

        let temp_dir = TempDir::new().unwrap();

        let config = InitialPeersConfig {
            local: true,
            ignore_cache: true,
            bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
            addrs: vec![cli_addr.clone()],
            ..Default::default()
        };
        let config = BootstrapConfig::try_from(&config).expect("Failed to create BootstrapConfig");
        let flow = Bootstrap::new_with_preloaded_addrs(config).await.unwrap();

        assert!(
            flow.is_bootstrap_peer(&env_peer_id),
            "Peer ID from env should be tracked"
        );
        assert!(
            flow.is_bootstrap_peer(&cli_peer_id),
            "Peer ID from CLI should be tracked"
        );

        let random_peer = libp2p::PeerId::random();
        assert!(!flow.is_bootstrap_peer(&random_peer));

        remove_env_var(ANT_PEERS_ENV);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn queue_empty_returns_correct_state_based_on_pending_fetches() {
        let temp_dir = TempDir::new().unwrap();
        let peer_id = libp2p::PeerId::random();
        let addr: Multiaddr = format!("/ip4/127.0.0.1/udp/5000/quic-v1/p2p/{peer_id}")
            .parse()
            .unwrap();

        let _env_guard = env_lock().await;
        set_env_var(ANT_PEERS_ENV, &addr.to_string());

        let config = InitialPeersConfig {
            local: true,
            ignore_cache: true,
            bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
            ..Default::default()
        };
        let config = BootstrapConfig::try_from(&config).expect("Failed to create BootstrapConfig");

        let flow = Bootstrap::new(config).unwrap();

        assert_eq!(flow.is_addr_queue_empty(), Some(false));

        remove_env_var(ANT_PEERS_ENV);

        let temp_dir_empty = TempDir::new().unwrap();

        let config_empty = InitialPeersConfig {
            local: true,
            ignore_cache: true,
            bootstrap_cache_dir: Some(temp_dir_empty.path().to_path_buf()),
            ..Default::default()
        };
        let mut config_empty =
            BootstrapConfig::try_from(&config_empty).expect("Failed to create BootstrapConfig");
        config_empty.disable_env_peers = true;

        let flow_empty = Bootstrap::new(config_empty).unwrap();

        assert_eq!(flow_empty.is_addr_queue_empty(), Some(true));

        let peer_id_cache = libp2p::PeerId::random();
        let cache_addr: Multiaddr = format!("/ip4/10.5.1.1/udp/5001/quic-v1/p2p/{peer_id_cache}")
            .parse()
            .unwrap();

        let temp_dir_pending = TempDir::new().unwrap();
        let file_name = BootstrapCacheStore::cache_file_name(true);

        let mut cache_data = CacheData::default();
        cache_data.add_peer(peer_id_cache, std::iter::once(&cache_addr), 3, 10);
        cache_data
            .write_to_file(temp_dir_pending.path(), &file_name)
            .unwrap();

        let config_pending = InitialPeersConfig {
            local: true,
            bootstrap_cache_dir: Some(temp_dir_pending.path().to_path_buf()),
            ..Default::default()
        };
        let mut config_pending =
            BootstrapConfig::try_from(&config_pending).expect("Failed to create BootstrapConfig");
        config_pending.disable_env_peers = true;

        let flow_pending = Bootstrap::new(config_pending).unwrap();

        assert_eq!(flow_pending.is_addr_queue_empty(), None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn terminated_false_until_completion_criteria_met() {
        let temp_dir_first = TempDir::new().unwrap();

        let config_first = InitialPeersConfig {
            first: true,
            bootstrap_cache_dir: Some(temp_dir_first.path().to_path_buf()),
            ..Default::default()
        };
        let config_first =
            BootstrapConfig::try_from(&config_first).expect("Failed to create BootstrapConfig");

        let flow_first = Bootstrap::new(config_first).unwrap();

        assert!(flow_first.has_terminated());

        let temp_dir = TempDir::new().unwrap();
        let peer_id = libp2p::PeerId::random();
        let addr: Multiaddr = format!("/ip4/10.5.4.1/udp/5004/quic-v1/p2p/{peer_id}")
            .parse()
            .unwrap();

        let config = InitialPeersConfig {
            local: true,
            ignore_cache: true,
            bootstrap_cache_dir: Some(temp_dir.path().to_path_buf()),
            addrs: vec![addr],
            ..Default::default()
        };
        let mut config =
            BootstrapConfig::try_from(&config).expect("Failed to create BootstrapConfig");
        config.disable_env_peers = true;

        let flow = Bootstrap::new(config).unwrap();

        assert!(!flow.has_terminated());
    }
}
