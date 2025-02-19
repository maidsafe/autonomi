// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// Implementation to record `libp2p::upnp::Event` metrics
mod bad_node;
mod relay_client;
pub mod service;
mod upnp;

use std::collections::HashSet;
use std::sync::atomic::AtomicU64;

use crate::MetricsRegistries;
use crate::{log_markers::Marker, time::sleep};
use bad_node::{BadNodeMetrics, BadNodeMetricsMsg, TimeFrame};
use libp2p::{
    metrics::{Metrics as Libp2pMetrics, Recorder},
    PeerId,
};
use prometheus_client::{
    metrics::family::Family,
    metrics::{counter::Counter, gauge::Gauge},
};
use sysinfo::{Pid, ProcessRefreshKind, System};
use tokio::time::Duration;

const UPDATE_INTERVAL: Duration = Duration::from_secs(60);
const TO_MB: u64 = 1_000_000;

/// The shared recorders that are used to record metrics.
pub(crate) struct NetworkMetricsRecorder {
    // Records libp2p related metrics
    // Must directly call self.libp2p_metrics.record(libp2p_event) with Recorder trait in scope. But since we have
    // re-implemented the trait for the wrapper struct, we can instead call self.record(libp2p_event)
    libp2p_metrics: Libp2pMetrics,
    upnp_events: Family<upnp::UpnpEventLabels, Counter>,
    relay_client_events: Family<relay_client::RelayClientEventLabels, Counter>,

    // metrics from ant-networking
    pub(crate) connected_peers: Gauge,
    pub(crate) estimated_network_size: Gauge,
    pub(crate) open_connections: Gauge,
    pub(crate) peers_in_routing_table: Gauge,
    pub(crate) relay_peers_in_routing_table: Gauge,
    pub(crate) records_stored: Gauge,
    pub(crate) relay_reservation_health: Gauge<f64, AtomicU64>,

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

    // trackers
    // The set of relay peers in our RT
    pub(crate) relay_peers_tracker: HashSet<PeerId>,

    // helpers
    bad_nodes_notifier: tokio::sync::mpsc::Sender<BadNodeMetricsMsg>,
}

impl NetworkMetricsRecorder {
    pub fn new(registries: &mut MetricsRegistries) -> Self {
        // ==== Standard metrics =====

        let libp2p_metrics = Libp2pMetrics::new(&mut registries.standard_metrics);
        let sub_registry = registries
            .standard_metrics
            .sub_registry_with_prefix("ant_networking");

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

        let estimated_network_size = Gauge::default();
        sub_registry.register(
            "estimated_network_size",
            "The estimated number of nodes in the network calculated by the peers in our RT",
            estimated_network_size.clone(),
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
            connected_peers,
            open_connections,
            relay_reservation_health,
            peers_in_routing_table,
            relay_peers_in_routing_table,
            relevant_records,
            max_records,
            received_payment_count,
            live_time,

            bad_peers_count,
            shunned_count_across_time_frames,
            shunned_count,
            shunned_by_close_group,
            shunned_by_old_close_group,

            process_memory_used_mb,
            process_cpu_usage_percentage,

            relay_peers_tracker: Default::default(),
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

        tokio::spawn(async move {
            loop {
                system.refresh_process_specifics(pid, process_refresh_kind);
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
                crate::time::spawn(async move {
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
        crate::time::spawn(async move {
            if let Err(err) = bad_nodes_notifier
                .send(BadNodeMetricsMsg::CloseGroupUpdated(new_close_group))
                .await
            {
                error!("Failed to send shunned report via notifier: {err:?}");
            }
        });
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
