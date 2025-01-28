use async_trait::async_trait;
use libp2p::PeerId;
use crate::rpc::{RpcActions, NodeInfo, NetworkInfo, RecordAddress};
use tokio::time::Duration;
use std::path::PathBuf;
use std::io::Write;
use crate::error::Result;

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

pub fn write_network_metrics_to_file(root_dir: PathBuf, network_info: NetworkInfoMetrics, peer_id: String) {
    let network_info_dir_path = root_dir.join("network_info");

    std::fs::create_dir_all(&network_info_dir_path).unwrap();

    let connected_peers_path = network_info_dir_path.join(format!("connected_peers_{}", peer_id));
    let mut file = std::fs::File::create(&connected_peers_path).unwrap();
    for peer in network_info.connected_peers.iter() {
        writeln!(file, "{}", peer).unwrap();
    }

    let listeners_path = network_info_dir_path.join(format!("listeners_{}", peer_id));
    let mut file = std::fs::File::create(&listeners_path).unwrap();
    for listeners in network_info.listeners.iter() {
        writeln!(file, "{}", listeners).unwrap();
    }
}

pub fn read_network_metrics_from_file(root_dir: PathBuf, peer_id: String) -> NetworkInfoMetrics {
    let network_info_dir_path = root_dir.join("network_info");
    let connected_peers_path = network_info_dir_path.join(format!("connected_peers_{}", peer_id));
    let listeners_path = network_info_dir_path.join(format!("listeners_{}", peer_id));
    println!("connected_peers_path: {:?}", connected_peers_path);

    let mut connected_peers = Vec::new();
    if std::path::Path::new(&connected_peers_path).exists() {
        match std::fs::read_to_string(&connected_peers_path) {
            Ok(contents) => connected_peers =  contents.lines().map(|s| s.to_string()).collect(),
            Err(e) => eprintln!("Failed to read the connected peers file: {}", e),
        }
    }

    let mut listeners = Vec::new();
    if std::path::Path::new(&listeners_path).exists() {
        match std::fs::read_to_string(&listeners_path) {
            Ok(contents) => listeners =  contents.lines().map(|s| s.to_string()).collect(),
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

    pub async fn get_endpoint_metrics(&self, endpoint_name: &str) -> Result<prometheus_parse::Scrape, Box<dyn std::error::Error>> {
            debug!(
                "Attempting connection to collect {} metrics from {}...",
                endpoint_name,
                self.endpoint_port
            );

            let body = reqwest::get(&format!("http://localhost:{}/{endpoint_name}", self.endpoint_port))
            .await?
            .text()
            .await?;
            let lines: Vec<_> = body.lines().map(|s| Ok(s.to_owned())).collect();
            let all_metrics = prometheus_parse::Scrape::parse(lines.into_iter())?;

            Ok(all_metrics)
    }

    pub fn get_node_info_from_metadata_extended(&self ,scrape: &prometheus_parse::Scrape, node_info: &mut NodeInfoMetrics) {
        for sample in scrape.samples.iter() {
            for (key, value) in sample.labels.iter() {
                match key.as_str() {
                    "peer_id" => node_info.peer_id = value.parse().unwrap(),
                    "pid" => node_info.pid = value.parse().unwrap(),
                    "bin_version" => node_info.bin_version = value.parse().unwrap(),
                    "root_dir" => node_info.root_dir =value.parse().unwrap(),
                    "log_dir" => node_info.log_dir = value.parse().unwrap(),
                    _ => {}
                }
            }
        }
    }

    pub fn get_node_info_from_metrics(&self, scrape: &prometheus_parse::Scrape, node_info: &mut NodeInfoMetrics) {
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
}

#[async_trait]
impl RpcActions for MetricClient {
    async fn node_info(&self) -> Result<NodeInfo> {
        let scrape = self.get_endpoint_metrics("metadata_extended").await.expect("Failed to get endpoint metadata_extended");
        let mut node_info = NodeInfoMetrics::default();
        self.get_node_info_from_metadata_extended(&scrape, &mut node_info);
        let scrape = self.get_endpoint_metrics("metrics").await.expect("Failed to get endpoint metrics");
        self.get_node_info_from_metrics(&scrape, &mut node_info);
        println!("node_info: {:?}", node_info);
        Ok(NodeInfo{
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
        let scrape = self.get_endpoint_metrics("metadata_extended").await.expect("Failed to get endpoint metadata_extended");
        let mut node_info = NodeInfoMetrics::default();
        self.get_node_info_from_metadata_extended(&scrape, &mut node_info);

        let network_info_metrics = read_network_metrics_from_file(node_info.root_dir, node_info.peer_id.to_string());
        // let connected_peers = network_info_metrics.connected_peers.into_iter().map(|s| s.parse().unwrap()).collect();
        // println!("network_info_metrics: {:?}", connected_peers);
        Ok(NetworkInfo {
            connected_peers: network_info_metrics.connected_peers.into_iter().map(|s| s.parse().unwrap()).collect(),
            listeners: network_info_metrics.listeners.into_iter().map(|s| s.parse().unwrap()).collect(),
        })
    }

    async fn record_addresses(&self) -> Result<Vec<RecordAddress>> {
        Ok(vec![])
    }

    async fn node_restart(&self, _delay_millis: u64, _retain_peer_id: bool) -> Result<()> {
        Ok(())
    }

    async fn node_stop(&self, _delay_millis: u64) -> Result<()> {
        Ok(())
    }

    async fn is_node_connected_to_network(&self, _timeout: Duration) -> Result<()> {
        Ok(())
    }

    async fn update_log_level(&self, _log_levels: String) -> Result<()> {
        Ok(())
    }

    async fn node_update(&self, _delay_millis: u64) -> Result<()> {
        Ok(())
    }
}