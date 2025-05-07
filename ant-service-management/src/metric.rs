// use ant_protocol::CLOSE_GROUP_SIZE;
use crate::error::{Error, Result};
use async_trait::async_trait;
use libp2p::{Multiaddr, PeerId};
use std::path::PathBuf;
use tokio::time::Duration;
// const MAX_CONNECTION_RETRY_ATTEMPTS: u8 = 5;
//const CONNECTION_RETRY_DELAY_SEC: Duration = Duration::from_secs(1);

#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub pid: u32,
    pub peer_id: PeerId,
    pub log_path: PathBuf,
    pub data_path: PathBuf,
    pub version: String,
    pub uptime: Duration,
    pub wallet_balance: u64,
}

#[derive(Debug, Clone)]
pub struct NetworkInfo {
    pub connected_peers: Vec<PeerId>,
    pub listeners: Vec<Multiaddr>,
}

#[async_trait]
pub trait MetricActions: Sync {
    async fn node_info(&self) -> Result<NodeInfo>;
    async fn network_info(&self) -> Result<NetworkInfo>;
    async fn is_node_connected_to_network(&self, timeout: Duration) -> Result<()>;
}
#[derive(Debug, Clone)]
pub struct NodeInfoMetrics {
    peer_id: PeerId,
    pid: u32,
    bin_version: String,
    root_dir: PathBuf,
    log_dir: PathBuf,
    uptime: Duration,
    wallet_balance: u64,
}

impl Default for NodeInfoMetrics {
    fn default() -> Self {
        Self {
            peer_id: PeerId::random(), //initialization
            pid: 0,
            bin_version: String::from("unknown"),
            root_dir: PathBuf::new(),
            log_dir: PathBuf::new(),
            uptime: Duration::new(0, 0),
            wallet_balance: 0,
        }
    }
}
pub struct NetworkInfoMetrics {
    pub connected_peers: Vec<String>,
    pub listeners: Vec<String>,
}

impl NetworkInfoMetrics {
    pub fn new(connected_peers_id: Vec<String>, listeners_id: Vec<String>) -> Self {
        Self {
            connected_peers: connected_peers_id,
            listeners: listeners_id,
        }
    }
}

pub fn read_network_metrics_from_file(root_dir: PathBuf) -> NetworkInfoMetrics {
    let connected_peers_path = root_dir.join("connected_peers");
    let listeners_path = root_dir.join("network_info_listeners");

    let mut connected_peers = Vec::new();
    if std::path::Path::new(&connected_peers_path).exists() {
        match std::fs::read_to_string(&connected_peers_path) {
            Ok(contents) => connected_peers = contents.lines().map(|s| s.to_string()).collect(),
            Err(e) => eprintln!("Failed to read the connected peers file: {}", e),
        }
    }

    let mut listeners = Vec::new();
    if std::path::Path::new(&listeners_path).exists() {
        match std::fs::read_to_string(&listeners_path) {
            Ok(contents) => listeners = contents.lines().map(|s| s.to_string()).collect(),
            Err(e) => eprintln!("Failed to read the listeners file: {}", e),
        }
    }

    NetworkInfoMetrics::new(connected_peers, listeners)
}

#[derive(Debug, Clone)]
pub struct MetricClient {
    endpoint_port: String,
}

impl MetricClient {
    pub fn new(endpoint_port: u16) -> Self {
        Self {
            endpoint_port: endpoint_port.to_string(),
        }
    }

    pub async fn get_endpoint_metrics(
        &self,
        endpoint_name: &str,
    ) -> Result<prometheus_parse::Scrape> {
        debug!(
            "Attempting connection to collect {} metrics from {}...",
            endpoint_name, self.endpoint_port
        );

        let body = reqwest::get(&format!(
            "http://localhost:{}/{endpoint_name}",
            self.endpoint_port
        ))
        .await
        .map_err(|_| Error::MetricServiceConnectionError(self.endpoint_port.clone()))?
        .text()
        .await
        .map_err(|_| Error::MetricServiceInfoResponseError)?;
        let lines: Vec<_> = body.lines().map(|s| Ok(s.to_owned())).collect();
        let all_metrics = prometheus_parse::Scrape::parse(lines.into_iter())?;

        Ok(all_metrics)
    }

    pub fn get_node_info_from_metadata_extended(
        &self,
        scrape: &prometheus_parse::Scrape,
        node_info: &mut NodeInfoMetrics,
    ) -> Result<()> {
        for sample in scrape.samples.iter() {
            for (key, value) in sample.labels.iter() {
                match key.as_str() {
                    "peer_id" => node_info.peer_id = value.parse()?,
                    "pid" => node_info.pid = value.parse()?,
                    "bin_version" => node_info.bin_version = value.to_string(),
                    "root_dir" => node_info.root_dir = PathBuf::from(value),
                    "log_dir" => node_info.log_dir = PathBuf::from(value),
                    _ => {}
                }
            }
        }
        Ok(())
    }

    pub fn get_node_info_from_metrics(
        &self,
        scrape: &prometheus_parse::Scrape,
        node_info: &mut NodeInfoMetrics,
    ) {
        for sample in scrape.samples.iter() {
            if sample.metric == "ant_node_current_reward_wallet_balance" {
                // Attos
                match sample.value {
                    prometheus_parse::Value::Counter(val)
                    | prometheus_parse::Value::Gauge(val)
                    | prometheus_parse::Value::Untyped(val) => {
                        node_info.wallet_balance = val as u64;
                    }
                    _ => {}
                }
            } else if sample.metric == "ant_node_uptime" {
                match sample.value {
                    prometheus_parse::Value::Counter(val)
                    | prometheus_parse::Value::Gauge(val)
                    | prometheus_parse::Value::Untyped(val) => {
                        node_info.uptime = Duration::new(val as u64, 0);
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn get_connected_peer_num_from_metrics(
        &self,
        scrape: &prometheus_parse::Scrape,
    ) -> Result<u64> {
        for sample in scrape.samples.iter() {
            if sample.metric == "ant_node_connected_peers" {
                match sample.value {
                    prometheus_parse::Value::Counter(val)
                    | prometheus_parse::Value::Gauge(val)
                    | prometheus_parse::Value::Untyped(val) => {
                        return Ok(val as u64);
                    }
                    _ => {}
                }
            }
        }
        Err(Error::MetricServiceInfoResponseError)
    }
}

#[async_trait]
impl MetricActions for MetricClient {
    async fn node_info(&self) -> Result<NodeInfo> {
        let scrape = self.get_endpoint_metrics("metadata_extended").await?;
        let mut node_info = NodeInfoMetrics::default();
        let _ = self.get_node_info_from_metadata_extended(&scrape, &mut node_info);
        let scrape = self.get_endpoint_metrics("metrics").await?;
        self.get_node_info_from_metrics(&scrape, &mut node_info);

        Ok(NodeInfo {
            peer_id: node_info.peer_id,
            pid: node_info.pid,
            version: node_info.bin_version,
            data_path: node_info.root_dir,
            log_path: node_info.log_dir,
            uptime: node_info.uptime,
            wallet_balance: node_info.wallet_balance,
        })
    }

    async fn network_info(&self) -> Result<NetworkInfo> {
        let scrape = self.get_endpoint_metrics("metadata_extended").await?;
        let mut node_info = NodeInfoMetrics::default();
        let _ = self.get_node_info_from_metadata_extended(&scrape, &mut node_info);

        let network_info_metrics = read_network_metrics_from_file(node_info.root_dir);

        let connected_peers_ = network_info_metrics
            .connected_peers
            .into_iter()
            .filter_map(|s| s.parse().ok()) // Ignores errors
            .collect();

        let listeners_ = network_info_metrics
            .listeners
            .into_iter()
            .filter_map(|s| s.parse().ok()) // Ignores errors
            .collect();

        Ok(NetworkInfo {
            connected_peers: connected_peers_,
            listeners: listeners_,
        })
    }

    async fn is_node_connected_to_network(&self, _timeout: Duration) -> Result<()> {
        // Todo: This is causing 5 mins delay during starting the node,
        // Todo: metrics server starts way later than the rpc server in node, need to refactor it further.

        //         let max_attempts = std::cmp::max(1, timeout.as_secs() / CONNECTION_RETRY_DELAY_SEC.as_secs());
        // trace!(
        //     "Metric conneciton max attempts set to: {max_attempts} with retry_delay of {:?}",
        //     CONNECTION_RETRY_DELAY_SEC
        // );
        // let mut attempts = 0;
        // loop {
        //     debug!(
        //         "Attempting connection to node metric endpoint at {}...",
        //         self.endpoint_port
        //     );

        //     let scrape = self.get_endpoint_metrics("metrics").await?;

        //     if let Ok(peer_num) = self.get_connected_peer_num_from_metrics(&scrape) {
        //         debug!("Connection to metric service successful");
        //             if peer_num  as usize > CLOSE_GROUP_SIZE {
        //                 return Ok(());
        //             } else {
        //                 error!(
        //                     "Node does not have enough peers connected yet. Retrying {attempts}/{max_attempts}",
        //                 );
        //             }
        //     } else {
        //         error!(
        //             "Could not connect to Metric endpoint {:?}. Retrying {attempts}/{max_attempts}",
        //             self.endpoint_port,
        //         );
        //     }
        //     attempts += 1;
        //     tokio::time::sleep(CONNECTION_RETRY_DELAY_SEC).await;
        //         if attempts >= max_attempts {
        //             return Err(Error::MetricServiceConnectionError(self.endpoint_port.clone()));
        //         }
        //     }
        Ok(())
    }
}
