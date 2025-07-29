// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod bad_node;
mod metadata;
mod reachability_check;
mod relay_client;
pub(super) mod service;
mod upnp;

use crate::networking::log_markers::Marker;
use crate::{ReachabilityStatus, networking::MetricsRegistries};
use bad_node::{BadNodeMetrics, BadNodeMetricsMsg, TimeFrame};
use libp2p::{
    PeerId,
    metrics::{Metrics as Libp2pMetrics, Recorder},
};
pub(crate) use metadata::MetadataExtendedRecorder;
pub(crate) use metadata::MetadataRecorder;
use prometheus_client::{
    encoding::EncodeLabelSet,
    metrics::{counter::Counter, family::Family, gauge::Gauge},
};

use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use sysinfo::{Pid, ProcessRefreshKind, System};
use tokio::time::Duration;
use tokio::time::sleep;

const UPDATE_INTERVAL: Duration = Duration::from_secs(60);
const TO_MB: u64 = 1_000_000;

// Add this new struct for version labels
#[derive(Clone, Hash, PartialEq, Eq, Debug, EncodeLabelSet)]
pub(crate) struct VersionLabels {
    version: String,
}

/// The shared recorders that are used to record metrics.
#[allow(dead_code)] // TODO: Remove this once we have removed all the deprecated metrics related to relay.
pub(crate) struct NetworkMetricsRecorder {
    // Records libp2p related metrics
    // Must directly call self.libp2p_metrics.record(libp2p_event) with Recorder trait in scope. But since we have
    // re-implemented the trait for the wrapper struct, we can instead call self.record(libp2p_event)
    libp2p_metrics: Libp2pMetrics,
    upnp_events: Family<upnp::UpnpEventLabels, Counter>,
    // DEPRECATED: This is no longer used.
    relay_client_events: Family<relay_client::RelayClientEventLabels, Counter>,

    // metrics from ant-networking
    pub(crate) connected_peers: Gauge,
    // DEPRECATED: This is no longer used.
    pub(crate) connected_relay_clients: Gauge,
    // DEPRECATED: This is no longer used.
    pub(crate) estimated_network_size: Gauge,
    // DEPRECATED: This is no longer used.
    pub(crate) relay_peers_percentage: Gauge<f64, AtomicU64>,
    pub(crate) open_connections: Gauge,
    pub(crate) peers_in_routing_table: Gauge,
    // DEPRECATED: This is no longer used.
    pub(crate) relay_peers_in_routing_table: Gauge,
    pub(crate) records_stored: Gauge,
    // DEPRECATED: This is no longer used.
    pub(crate) relay_reservation_health: Gauge<f64, AtomicU64>,
    pub(crate) node_versions: Family<VersionLabels, Gauge>,
    pub(crate) reachability_check_progress: Gauge<f64, AtomicU64>,

    // quoting metrics
    relevant_records: Gauge,
    max_records: Gauge,
    received_payment_count: Gauge,
    live_time: Gauge,

    // bad node metrics
    bad_peers_count: Counter,
    shunned_count: Counter,
    #[allow(dead_code)] // updated by background task
    shunned_count_across_time_frames: Family<TimeFrame, Gauge>,
    #[allow(dead_code)]
    shunned_by_close_group: Gauge,
    #[allow(dead_code)]
    shunned_by_old_close_group: Gauge,

    // system info
    process_memory_used_mb: Gauge<f64, AtomicU64>,
    process_cpu_usage_percentage: Gauge<f64, AtomicU64>,

    // helpers
    bad_nodes_notifier: tokio::sync::mpsc::Sender<BadNodeMetricsMsg>,
}

impl NetworkMetricsRecorder {
    pub(crate) fn new(
        registries: &mut MetricsRegistries,
        reachability_status: &Option<ReachabilityStatus>,
    ) -> Self {
        // ==== Standard metrics =====

        let libp2p_metrics = Libp2pMetrics::new(&mut registries.standard_metrics);
        let sub_registry = registries
            .standard_metrics
            .sub_registry_with_prefix("ant_networking");

        // reachability check should be a part of the standard metrics and is a gauge value, but never changes.
        let reachability_adapter_metric =
            reachability_check::get_reachability_adapter_metric(reachability_status);

        sub_registry.register(
            "reachability_adapter",
            "The reachability adapter status of the node.",
            reachability_adapter_metric,
        );

        let reachability_check_progress = Gauge::<f64, AtomicU64>::default();
        // set to 100 if reachability_status is found (completed)
        if reachability_status.is_some() {
            let _ = reachability_check_progress.set(100.0);
        } else {
            let _ = reachability_check_progress.set(0.0);
        }
        sub_registry.register(
            "reachability_check_progress",
            "Progress indicator for reachability check. 0 = not run, 1-99 = in progress, 100 = completed",
            reachability_check_progress.clone(),
        );

        let records_stored = Gauge::default();
        sub_registry.register(
            "records_stored",
            "The number of records stored locally",
            records_stored.clone(),
        );
        let relay_reservation_health = Gauge::<f64, AtomicU64>::default();
        sub_registry.register(
            "relay_reservation_health",
            "The average health of all the relay reservation connections. Value is between 0-1",
            relay_reservation_health.clone(),
        );

        let connected_peers = Gauge::default();
        sub_registry.register(
            "connected_peers",
            "The number of peers that we are currently connected to",
            connected_peers.clone(),
        );
        let connected_relay_clients = Gauge::default();
        sub_registry.register(
            "connected_relay_clients",
            "The number of relay clients that are currently connected to us",
            connected_relay_clients.clone(),
        );

        let estimated_network_size = Gauge::default();
        sub_registry.register(
            "estimated_network_size",
            "The estimated number of nodes in the network calculated by the peers in our RT",
            estimated_network_size.clone(),
        );
        let relay_peers_percentage = Gauge::<f64, AtomicU64>::default();
        sub_registry.register(
            "relay_peers_percentage",
            "The percentage of relay peers in our routing table",
            relay_peers_percentage.clone(),
        );
        let open_connections = Gauge::default();
        sub_registry.register(
            "open_connections",
            "The number of active connections to other peers",
            open_connections.clone(),
        );
        let peers_in_routing_table = Gauge::default();
        sub_registry.register(
            "peers_in_routing_table",
            "The total number of peers in our routing table",
            peers_in_routing_table.clone(),
        );
        let relay_peers_in_routing_table = Gauge::default();
        sub_registry.register(
            "relay_peers_in_routing_table",
            "The total number of relay peers in our routing table",
            relay_peers_in_routing_table.clone(),
        );

        let shunned_count = Counter::default();
        sub_registry.register(
            "shunned_count",
            "Number of peers that have shunned our node",
            shunned_count.clone(),
        );

        let bad_peers_count = Counter::default();
        sub_registry.register(
            "bad_peers_count",
            "Number of bad peers that have been detected by us and been added to the blocklist",
            bad_peers_count.clone(),
        );

        let upnp_events = Family::default();
        sub_registry.register(
            "upnp_events",
            "Events emitted by the UPnP behaviour",
            upnp_events.clone(),
        );

        let relay_client_events = Family::default();
        sub_registry.register(
            "relay_client_events",
            "Events emitted by the relay client",
            relay_client_events.clone(),
        );

        // Add this new metric registration
        let node_versions = Family::default();
        sub_registry.register(
            "node_versions",
            "Number of nodes running each version",
            node_versions.clone(),
        );

        let process_memory_used_mb = Gauge::<f64, AtomicU64>::default();
        sub_registry.register(
            "process_memory_used_mb",
            "Memory used by the process in MegaBytes",
            process_memory_used_mb.clone(),
        );

        let process_cpu_usage_percentage = Gauge::<f64, AtomicU64>::default();
        sub_registry.register(
            "process_cpu_usage_percentage",
            "The percentage of CPU used by the process. Value is from 0-100",
            process_cpu_usage_percentage.clone(),
        );

        // quoting metrics
        let relevant_records = Gauge::default();
        sub_registry.register(
            "relevant_records",
            "The number of records that we're responsible for. This is used to calculate the store cost",
            relevant_records.clone(),
        );
        let max_records = Gauge::default();
        sub_registry.register(
            "max_records",
            "The maximum number of records that we can store. This is used to calculate the store cost",
            max_records.clone(),
        );
        let received_payment_count = Gauge::default();
        sub_registry.register(
            "received_payment_count",
            "The number of payments received by our node. This is used to calculate the store cost",
            received_payment_count.clone(),
        );
        let live_time = Gauge::default();
        sub_registry.register(
            "live_time",
            "The time for which the node has been alive. This is used to calculate the store cost",
            live_time.clone(),
        );

        let shunned_by_close_group = Gauge::default();
        sub_registry.register(
            "shunned_by_close_group",
            "The number of close group peers that have shunned our node",
            shunned_by_close_group.clone(),
        );

        let shunned_by_old_close_group = Gauge::default();
        sub_registry.register(
            "shunned_by_old_close_group",
            "The number of close group peers that have shunned our node. This contains the peers that were once in our close group but have since been evicted.",
            shunned_by_old_close_group.clone(),
        );

        // ==== Extended metrics =====

        let extended_metrics_sub_registry = registries
            .extended_metrics
            .sub_registry_with_prefix("ant_networking");
        let shunned_count_across_time_frames = Family::default();
        extended_metrics_sub_registry.register(
            "shunned_count_across_time_frames",
            "The number of times our node has been shunned by other nodes across different time frames",
            shunned_count_across_time_frames.clone(),
        );

        let bad_nodes_notifier = BadNodeMetrics::spawn_background_task(
            shunned_count_across_time_frames.clone(),
            shunned_by_close_group.clone(),
            shunned_by_old_close_group.clone(),
        );
        let network_metrics = Self {
            libp2p_metrics,
            upnp_events,
            relay_client_events,

            records_stored,
            estimated_network_size,
            relay_peers_percentage,
            connected_peers,
            connected_relay_clients,
            open_connections,
            relay_reservation_health,
            peers_in_routing_table,
            relay_peers_in_routing_table,
            relevant_records,
            max_records,
            received_payment_count,
            live_time,
            node_versions,
            reachability_check_progress,

            bad_peers_count,
            shunned_count_across_time_frames,
            shunned_count,
            shunned_by_close_group,
            shunned_by_old_close_group,

            process_memory_used_mb,
            process_cpu_usage_percentage,

            bad_nodes_notifier,
        };

        network_metrics.system_metrics_recorder_task();
        network_metrics
    }

    // Updates registry with sysinfo metrics
    fn system_metrics_recorder_task(&self) {
        // spawn task to record system metrics
        let process_memory_used_mb = self.process_memory_used_mb.clone();
        let process_cpu_usage_percentage = self.process_cpu_usage_percentage.clone();

        let pid = Pid::from_u32(std::process::id());
        let process_refresh_kind = ProcessRefreshKind::everything().without_disk_usage();
        let mut system = System::new();
        let physical_core_count = system.physical_core_count();

        #[allow(clippy::let_underscore_future)]
        let _ = tokio::spawn(async move {
            loop {
                let _ = system.refresh_process_specifics(pid, process_refresh_kind);
                if let (Some(process), Some(core_count)) =
                    (system.process(pid), physical_core_count)
                {
                    let mem_used =
                        ((process.memory() as f64 / TO_MB as f64) * 10000.0).round() / 10000.0;
                    let _ = process_memory_used_mb.set(mem_used);
                    // divide by core_count to get value between 0-100
                    let cpu_usage = ((process.cpu_usage() as f64 / core_count as f64) * 10000.0)
                        .round()
                        / 10000.0;
                    let _ = process_cpu_usage_percentage.set(cpu_usage);
                }
                sleep(UPDATE_INTERVAL).await;
            }
        });
    }

    // Records the metric
    pub(crate) fn record_from_marker(&self, log_marker: Marker) {
        match log_marker {
            Marker::PeerConsideredAsBad { .. } => {
                let _ = self.bad_peers_count.inc();
            }
            Marker::FlaggedAsBadNode { flagged_by } => {
                let _ = self.shunned_count.inc();
                let bad_nodes_notifier = self.bad_nodes_notifier.clone();
                let flagged_by = *flagged_by;
                #[allow(clippy::let_underscore_future)]
                let _ = tokio::spawn(async move {
                    if let Err(err) = bad_nodes_notifier
                        .send(BadNodeMetricsMsg::ShunnedByPeer(flagged_by))
                        .await
                    {
                        error!("Failed to send shunned report via notifier: {err:?}");
                    }
                });
            }
            Marker::QuotingMetrics { quoting_metrics } => {
                let _ = self.relevant_records.set(
                    quoting_metrics
                        .close_records_stored
                        .try_into()
                        .unwrap_or(i64::MAX),
                );
                let _ = self
                    .relevant_records
                    .set(quoting_metrics.close_records_stored as i64);
                let _ = self.max_records.set(quoting_metrics.max_records as i64);
                let _ = self
                    .received_payment_count
                    .set(quoting_metrics.received_payment_count as i64);
                let _ = self.live_time.set(quoting_metrics.live_time as i64);
            }
            _ => {}
        }
    }

    pub(crate) fn record_change_in_close_group(&self, new_close_group: Vec<PeerId>) {
        let bad_nodes_notifier = self.bad_nodes_notifier.clone();
        #[allow(clippy::let_underscore_future)]
        let _ = tokio::spawn(async move {
            if let Err(err) = bad_nodes_notifier
                .send(BadNodeMetricsMsg::CloseGroupUpdated(new_close_group))
                .await
            {
                error!("Failed to send shunned report via notifier: {err:?}");
            }
        });
    }

    pub(crate) fn update_node_versions(&self, versions: &HashMap<PeerId, String>) {
        // First, count occurrences of each version
        let mut version_counts: HashMap<String, u64> = HashMap::new();
        for version in versions.values() {
            *version_counts.entry(version.clone()).or_insert(0) += 1;
        }

        // Clean up old records, to avoid outdated versions pollute the statistic.
        self.node_versions.clear();

        // Update metrics
        for (version, count) in version_counts {
            let _ = self
                .node_versions
                .get_or_create(&VersionLabels { version })
                .set(count as i64);
        }
    }
}

/// Impl the Recorder traits again for our struct.
impl Recorder<libp2p::kad::Event> for NetworkMetricsRecorder {
    fn record(&self, event: &libp2p::kad::Event) {
        self.libp2p_metrics.record(event)
    }
}

impl Recorder<libp2p::relay::Event> for NetworkMetricsRecorder {
    fn record(&self, event: &libp2p::relay::Event) {
        self.libp2p_metrics.record(event)
    }
}

impl Recorder<libp2p::identify::Event> for NetworkMetricsRecorder {
    fn record(&self, event: &libp2p::identify::Event) {
        self.libp2p_metrics.record(event)
    }
}

impl<T> Recorder<libp2p::swarm::SwarmEvent<T>> for NetworkMetricsRecorder {
    fn record(&self, event: &libp2p::swarm::SwarmEvent<T>) {
        self.libp2p_metrics.record(event);
    }
}
