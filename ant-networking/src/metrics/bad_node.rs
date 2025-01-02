// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::time::interval;
use ant_protocol::CLOSE_GROUP_SIZE;
use libp2p::PeerId;
use prometheus_client::{
    encoding::{EncodeLabelSet, EncodeLabelValue},
    metrics::{family::Family, gauge::Gauge},
};
use std::{
    collections::{HashSet, VecDeque},
    time::{Duration, Instant},
};
use strum::IntoEnumIterator;

const UPDATE_INTERVAL: Duration = Duration::from_secs(20);

#[cfg(not(test))]
const MAX_EVICTED_CLOSE_GROUP_PEERS: usize = 5 * CLOSE_GROUP_SIZE;
#[cfg(test)]
const MAX_EVICTED_CLOSE_GROUP_PEERS: usize = CLOSE_GROUP_SIZE + 2;

pub struct BadNodeMetrics {
    shunned_count_across_time_frames: ShunnedCountAcrossTimeFrames,
    shunned_by_close_group: ShunnedByCloseGroup,
}

pub enum BadNodeMetricsMsg {
    ShunnedByPeer(PeerId),
    CloseGroupUpdated(Vec<PeerId>),
}

struct ShunnedByCloseGroup {
    metric_current_group: Gauge,
    metric_old_group: Gauge,

    // trackers
    close_group_peers: Vec<PeerId>,
    old_close_group_peers: VecDeque<PeerId>,
    old_new_group_shunned_list: HashSet<PeerId>,
}

/// A struct to record the number of reports against our node across different time frames.
struct ShunnedCountAcrossTimeFrames {
    metric: Family<TimeFrame, Gauge>,
    shunned_report_tracker: Vec<ShunnedReportTracker>,
}

struct ShunnedReportTracker {
    time: Instant,
    least_bucket_it_fits_in: TimeFrameType,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct TimeFrame {
    time_frame: TimeFrameType,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, EncodeLabelValue, strum::EnumIter)]
pub enum TimeFrameType {
    LastTenMinutes,
    LastHour,
    LastSixHours,
    LastDay,
    LastWeek,
    Indefinite,
}

impl TimeFrameType {
    #[cfg(not(test))]
    fn get_duration_sec(&self) -> u64 {
        match self {
            TimeFrameType::LastTenMinutes => 10 * 60,
            TimeFrameType::LastHour => 60 * 60,
            TimeFrameType::LastSixHours => 6 * 60 * 60,
            TimeFrameType::LastDay => 24 * 60 * 60,
            TimeFrameType::LastWeek => 7 * 24 * 60 * 60,
            TimeFrameType::Indefinite => u64::MAX,
        }
    }

    #[cfg(test)]
    fn get_duration_sec(&self) -> u64 {
        match self {
            TimeFrameType::LastTenMinutes => 2,
            TimeFrameType::LastHour => 4,
            TimeFrameType::LastSixHours => 6,
            TimeFrameType::LastDay => 8,
            TimeFrameType::LastWeek => 10,
            TimeFrameType::Indefinite => u64::MAX,
        }
    }

    fn next_time_frame(&self) -> Self {
        match self {
            TimeFrameType::LastTenMinutes => TimeFrameType::LastHour,
            TimeFrameType::LastHour => TimeFrameType::LastSixHours,
            TimeFrameType::LastSixHours => TimeFrameType::LastDay,
            TimeFrameType::LastDay => TimeFrameType::LastWeek,
            TimeFrameType::LastWeek => TimeFrameType::Indefinite,
            TimeFrameType::Indefinite => TimeFrameType::Indefinite,
        }
    }
}

impl BadNodeMetrics {
    pub fn spawn_background_task(
        time_based_shunned_count: Family<TimeFrame, Gauge>,
        shunned_by_close_group: Gauge,
        shunned_by_old_close_group: Gauge,
    ) -> tokio::sync::mpsc::Sender<BadNodeMetricsMsg> {
        let mut bad_node_metrics = BadNodeMetrics {
            shunned_count_across_time_frames: ShunnedCountAcrossTimeFrames {
                metric: time_based_shunned_count,
                shunned_report_tracker: Vec::new(),
            },
            shunned_by_close_group: ShunnedByCloseGroup {
                metric_current_group: shunned_by_close_group,
                metric_old_group: shunned_by_old_close_group,

                close_group_peers: Vec::new(),
                old_close_group_peers: VecDeque::new(),
                // Shunned by old or new close group
                old_new_group_shunned_list: HashSet::new(),
            },
        };

        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        tokio::spawn(async move {
            let mut update_interval = interval(UPDATE_INTERVAL);
            update_interval.tick().await;

            loop {
                tokio::select! {
                    msg = rx.recv() => {
                        match msg {
                            Some(BadNodeMetricsMsg::ShunnedByPeer(peer)) => {
                                bad_node_metrics.shunned_count_across_time_frames.record_shunned_metric();
                                bad_node_metrics.shunned_by_close_group.record_shunned_metric(peer);

                            }
                            Some(BadNodeMetricsMsg::CloseGroupUpdated(new_closest_peers)) => {
                                bad_node_metrics.shunned_by_close_group.update_close_group_peers(new_closest_peers);
                            }
                            None => break,
                        }


                    }
                    _ = update_interval.tick() => {
                        bad_node_metrics.shunned_count_across_time_frames.try_update_state();
                    }
                }
            }
        });
        tx
    }
}

impl ShunnedByCloseGroup {
    pub(crate) fn record_shunned_metric(&mut self, peer: PeerId) {
        // increment the metric if the peer is in the close group (new or old) and hasn't shunned us before
        if !self.old_new_group_shunned_list.contains(&peer) {
            if self.close_group_peers.contains(&peer) {
                self.metric_current_group.inc();
                self.old_new_group_shunned_list.insert(peer);
            } else if self.old_close_group_peers.contains(&peer) {
                self.metric_old_group.inc();
                self.old_new_group_shunned_list.insert(peer);
            }
        }
    }

    pub(crate) fn update_close_group_peers(&mut self, new_closest_peers: Vec<PeerId>) {
        let new_members: Vec<PeerId> = new_closest_peers
            .iter()
            .filter(|p| !self.close_group_peers.contains(p))
            .cloned()
            .collect();
        let evicted_members: Vec<PeerId> = self
            .close_group_peers
            .iter()
            .filter(|p| !new_closest_peers.contains(p))
            .cloned()
            .collect();
        for new_member in &new_members {
            // if it has shunned us before, update the metrics.
            if self.old_new_group_shunned_list.contains(new_member) {
                self.metric_old_group.dec();
                self.metric_current_group.inc();
            }
        }

        for evicted_member in &evicted_members {
            self.old_close_group_peers.push_back(*evicted_member);
            // if it has shunned us before, update the metrics.
            if self.old_new_group_shunned_list.contains(evicted_member) {
                self.metric_current_group.dec();
                self.metric_old_group.inc();
            }
        }

        if !new_members.is_empty() {
            debug!("The close group has been updated. The new members are {new_members:?}. The evicted members are {evicted_members:?}");
            self.close_group_peers = new_closest_peers;

            while self.old_close_group_peers.len() > MAX_EVICTED_CLOSE_GROUP_PEERS {
                if let Some(removed_peer) = self.old_close_group_peers.pop_front() {
                    if self.old_new_group_shunned_list.remove(&removed_peer) {
                        self.metric_old_group.dec();
                    }
                }
            }
        }
    }
}

impl ShunnedCountAcrossTimeFrames {
    fn record_shunned_metric(&mut self) {
        let now = Instant::now();
        self.shunned_report_tracker.push(ShunnedReportTracker {
            time: now,
            least_bucket_it_fits_in: TimeFrameType::LastTenMinutes,
        });

        for variant in TimeFrameType::iter() {
            let time_frame = TimeFrame {
                time_frame: variant,
            };
            self.metric.get_or_create(&time_frame).inc();
        }
    }

    fn try_update_state(&mut self) {
        let now = Instant::now();
        let mut idx_to_remove = Vec::new();

        for (idx, tracked_value) in self.shunned_report_tracker.iter_mut().enumerate() {
            let time_elapsed_since_adding = now.duration_since(tracked_value.time).as_secs();

            if time_elapsed_since_adding > tracked_value.least_bucket_it_fits_in.get_duration_sec()
            {
                let time_frame = TimeFrame {
                    time_frame: tracked_value.least_bucket_it_fits_in,
                };
                self.metric.get_or_create(&time_frame).dec();

                let new_time_frame = tracked_value.least_bucket_it_fits_in.next_time_frame();
                if new_time_frame == TimeFrameType::Indefinite {
                    idx_to_remove.push(idx);
                } else {
                    tracked_value.least_bucket_it_fits_in = new_time_frame;
                }
            }
        }
        // remove the ones that are now indefinite
        for idx in idx_to_remove {
            self.shunned_report_tracker.remove(idx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eyre::Result;

    #[test]
    fn update_should_move_to_next_timeframe() -> Result<()> {
        let mut shunned_metrics = ShunnedCountAcrossTimeFrames {
            metric: Family::default(),
            shunned_report_tracker: Vec::new(),
        };
        shunned_metrics.record_shunned_metric();

        let current_state = shunned_metrics.shunned_report_tracker[0].least_bucket_it_fits_in;
        assert!(matches!(current_state, TimeFrameType::LastTenMinutes));
        // all the counters should be 1
        for variant in TimeFrameType::iter() {
            let time_frame = TimeFrame {
                time_frame: variant,
            };
            assert_eq!(shunned_metrics.metric.get_or_create(&time_frame).get(), 1);
        }

        println!(
            "current_state: {current_state:?}. Sleeping for {} sec",
            current_state.get_duration_sec() + 1
        );
        std::thread::sleep(std::time::Duration::from_secs(
            current_state.get_duration_sec() + 1,
        ));
        shunned_metrics.try_update_state();
        let current_state = shunned_metrics.shunned_report_tracker[0].least_bucket_it_fits_in;
        assert!(matches!(current_state, TimeFrameType::LastHour));
        // all the counters except LastTenMinutes should be 1
        for variant in TimeFrameType::iter() {
            let time_frame = TimeFrame {
                time_frame: variant,
            };
            if variant == TimeFrameType::LastTenMinutes {
                assert_eq!(shunned_metrics.metric.get_or_create(&time_frame).get(), 0);
            } else {
                assert_eq!(shunned_metrics.metric.get_or_create(&time_frame).get(), 1);
            }
        }

        println!(
            "current_state: {current_state:?}. Sleeping for {} sec",
            current_state.get_duration_sec() + 1
        );
        std::thread::sleep(std::time::Duration::from_secs(
            current_state.get_duration_sec() + 1,
        ));
        shunned_metrics.try_update_state();
        let current_state = shunned_metrics.shunned_report_tracker[0].least_bucket_it_fits_in;
        assert!(matches!(current_state, TimeFrameType::LastSixHours));
        // all the counters except LastTenMinutes and LastHour should be 1
        for variant in TimeFrameType::iter() {
            let time_frame = TimeFrame {
                time_frame: variant,
            };
            if variant == TimeFrameType::LastTenMinutes || variant == TimeFrameType::LastHour {
                assert_eq!(shunned_metrics.metric.get_or_create(&time_frame).get(), 0);
            } else {
                assert_eq!(shunned_metrics.metric.get_or_create(&time_frame).get(), 1);
            }
        }

        println!(
            "current_state: {current_state:?}. Sleeping for {} sec",
            current_state.get_duration_sec() + 1
        );
        std::thread::sleep(std::time::Duration::from_secs(
            current_state.get_duration_sec() + 1,
        ));
        shunned_metrics.try_update_state();
        let current_state = shunned_metrics.shunned_report_tracker[0].least_bucket_it_fits_in;
        assert!(matches!(current_state, TimeFrameType::LastDay));
        // all the counters except LastTenMinutes, LastHour and LastSixHours should be 1
        for variant in TimeFrameType::iter() {
            let time_frame = TimeFrame {
                time_frame: variant,
            };
            if variant == TimeFrameType::LastTenMinutes
                || variant == TimeFrameType::LastHour
                || variant == TimeFrameType::LastSixHours
            {
                assert_eq!(shunned_metrics.metric.get_or_create(&time_frame).get(), 0);
            } else {
                assert_eq!(shunned_metrics.metric.get_or_create(&time_frame).get(), 1);
            }
        }

        println!(
            "current_state: {current_state:?}. Sleeping for {} sec",
            current_state.get_duration_sec() + 1
        );
        std::thread::sleep(std::time::Duration::from_secs(
            current_state.get_duration_sec() + 1,
        ));
        shunned_metrics.try_update_state();
        let current_state = shunned_metrics.shunned_report_tracker[0].least_bucket_it_fits_in;
        assert!(matches!(current_state, TimeFrameType::LastWeek));
        // all the counters except LastTenMinutes, LastHour, LastSixHours and LastDay should be 1
        for variant in TimeFrameType::iter() {
            let time_frame = TimeFrame {
                time_frame: variant,
            };
            if variant == TimeFrameType::LastTenMinutes
                || variant == TimeFrameType::LastHour
                || variant == TimeFrameType::LastSixHours
                || variant == TimeFrameType::LastDay
            {
                assert_eq!(shunned_metrics.metric.get_or_create(&time_frame).get(), 0);
            } else {
                assert_eq!(shunned_metrics.metric.get_or_create(&time_frame).get(), 1);
            }
        }

        println!(
            "current_state: {current_state:?}. Sleeping for {} sec",
            current_state.get_duration_sec() + 1
        );
        std::thread::sleep(std::time::Duration::from_secs(
            current_state.get_duration_sec() + 1,
        ));
        shunned_metrics.try_update_state();
        assert_eq!(shunned_metrics.shunned_report_tracker.len(), 0);
        // all the counters except Indefinite should be 0
        for variant in TimeFrameType::iter() {
            let time_frame = TimeFrame {
                time_frame: variant,
            };
            if variant == TimeFrameType::Indefinite {
                assert_eq!(shunned_metrics.metric.get_or_create(&time_frame).get(), 1);
            } else {
                assert_eq!(shunned_metrics.metric.get_or_create(&time_frame).get(), 0);
            }
        }

        Ok(())
    }

    #[test]
    fn metrics_should_not_be_updated_if_close_group_is_not_set() -> Result<()> {
        let mut close_group_shunned = ShunnedByCloseGroup {
            metric_current_group: Gauge::default(),
            metric_old_group: Gauge::default(),

            close_group_peers: Vec::new(),
            old_close_group_peers: VecDeque::new(),
            old_new_group_shunned_list: HashSet::new(),
        };

        close_group_shunned.record_shunned_metric(PeerId::random());
        assert_eq!(close_group_shunned.metric_current_group.get(), 0);
        assert_eq!(close_group_shunned.metric_old_group.get(), 0);

        Ok(())
    }

    #[test]
    fn close_group_shunned_metric_should_be_updated_on_new_report() -> Result<()> {
        let mut close_group_shunned = ShunnedByCloseGroup {
            metric_current_group: Gauge::default(),
            metric_old_group: Gauge::default(),

            close_group_peers: Vec::new(),
            old_close_group_peers: VecDeque::new(),
            old_new_group_shunned_list: HashSet::new(),
        };
        close_group_shunned.update_close_group_peers(vec![
            PeerId::random(),
            PeerId::random(),
            PeerId::random(),
            PeerId::random(),
            PeerId::random(),
        ]);
        // report by a peer in the close group should increment the metric
        close_group_shunned.record_shunned_metric(close_group_shunned.close_group_peers[0]);
        assert_eq!(close_group_shunned.metric_current_group.get(), 1);
        assert_eq!(close_group_shunned.metric_old_group.get(), 0);

        // report by same peer should not increment the metric
        close_group_shunned.record_shunned_metric(close_group_shunned.close_group_peers[0]);
        assert_eq!(close_group_shunned.metric_current_group.get(), 1);
        assert_eq!(close_group_shunned.metric_old_group.get(), 0);

        // report by a different peer should increment the metric
        close_group_shunned.record_shunned_metric(close_group_shunned.close_group_peers[1]);
        assert_eq!(close_group_shunned.metric_current_group.get(), 2);
        assert_eq!(close_group_shunned.metric_old_group.get(), 0);

        // report by a peer that is not in the close group should not increment the metric
        close_group_shunned.record_shunned_metric(PeerId::random());
        assert_eq!(close_group_shunned.metric_current_group.get(), 2);
        assert_eq!(close_group_shunned.metric_old_group.get(), 0);

        Ok(())
    }

    #[test]
    fn change_in_close_group_should_update_the_metrics() -> Result<()> {
        let mut close_group_shunned = ShunnedByCloseGroup {
            metric_current_group: Gauge::default(),
            metric_old_group: Gauge::default(),

            close_group_peers: Vec::new(),
            old_close_group_peers: VecDeque::new(),
            old_new_group_shunned_list: HashSet::new(),
        };
        close_group_shunned.update_close_group_peers(vec![
            PeerId::random(),
            PeerId::random(),
            PeerId::random(),
            PeerId::random(),
            PeerId::random(),
        ]);
        let old_member = close_group_shunned.close_group_peers[0];
        close_group_shunned.record_shunned_metric(old_member);
        assert_eq!(close_group_shunned.metric_current_group.get(), 1);
        assert_eq!(close_group_shunned.metric_old_group.get(), 0);

        // update close group
        close_group_shunned.update_close_group_peers(vec![
            PeerId::random(),
            close_group_shunned.close_group_peers[1],
            close_group_shunned.close_group_peers[2],
            close_group_shunned.close_group_peers[3],
            close_group_shunned.close_group_peers[4],
        ]);

        // the peer that shunned us before should now be in the old group
        assert_eq!(close_group_shunned.metric_current_group.get(), 0);
        assert_eq!(close_group_shunned.metric_old_group.get(), 1);

        // report by the old member should not increment the metric
        close_group_shunned.record_shunned_metric(old_member);
        assert_eq!(close_group_shunned.metric_current_group.get(), 0);
        assert_eq!(close_group_shunned.metric_old_group.get(), 1);

        // update close group with old member
        close_group_shunned.update_close_group_peers(vec![
            old_member,
            close_group_shunned.close_group_peers[1],
            close_group_shunned.close_group_peers[2],
            close_group_shunned.close_group_peers[3],
            close_group_shunned.close_group_peers[4],
        ]);

        // the metrics of current_group and old_group should be updated
        assert_eq!(close_group_shunned.metric_current_group.get(), 1);
        assert_eq!(close_group_shunned.metric_old_group.get(), 0);

        Ok(())
    }

    #[test]
    fn update_close_group_metrics_on_reaching_max_evicted_peer_count() -> Result<()> {
        let mut close_group_shunned = ShunnedByCloseGroup {
            metric_current_group: Gauge::default(),
            metric_old_group: Gauge::default(),

            close_group_peers: Vec::new(),
            old_close_group_peers: VecDeque::new(),
            old_new_group_shunned_list: HashSet::new(),
        };
        close_group_shunned.update_close_group_peers(vec![
            PeerId::random(),
            PeerId::random(),
            PeerId::random(),
            PeerId::random(),
            PeerId::random(),
        ]);

        // evict 1 members
        let old_member_1 = close_group_shunned.close_group_peers[0];
        close_group_shunned.update_close_group_peers(vec![
            close_group_shunned.close_group_peers[1],
            close_group_shunned.close_group_peers[2],
            close_group_shunned.close_group_peers[3],
            close_group_shunned.close_group_peers[4],
            PeerId::random(),
        ]);

        // evict 1 members
        let old_member_2 = close_group_shunned.close_group_peers[0];
        close_group_shunned.update_close_group_peers(vec![
            close_group_shunned.close_group_peers[1],
            close_group_shunned.close_group_peers[2],
            close_group_shunned.close_group_peers[3],
            close_group_shunned.close_group_peers[4],
            PeerId::random(),
        ]);

        // report by the evicted members should increment the old group metric
        close_group_shunned.record_shunned_metric(old_member_1);
        assert_eq!(close_group_shunned.metric_current_group.get(), 0);
        assert_eq!(close_group_shunned.metric_old_group.get(), 1);
        close_group_shunned.record_shunned_metric(old_member_2);
        assert_eq!(close_group_shunned.metric_current_group.get(), 0);
        assert_eq!(close_group_shunned.metric_old_group.get(), 2);

        // evict all the members
        close_group_shunned.update_close_group_peers(vec![
            PeerId::random(),
            PeerId::random(),
            PeerId::random(),
            PeerId::random(),
            PeerId::random(),
        ]);

        // the metrics should still remain the same
        assert_eq!(close_group_shunned.metric_current_group.get(), 0);
        assert_eq!(close_group_shunned.metric_old_group.get(), 2);

        // evict 1 more members to cross the threshold
        close_group_shunned.update_close_group_peers(vec![
            close_group_shunned.close_group_peers[1],
            close_group_shunned.close_group_peers[2],
            close_group_shunned.close_group_peers[3],
            close_group_shunned.close_group_peers[4],
            PeerId::random(),
        ]);

        // the metric from the member_1 should be removed
        assert_eq!(close_group_shunned.metric_current_group.get(), 0);
        assert_eq!(close_group_shunned.metric_old_group.get(), 1);
        assert!(!close_group_shunned
            .old_close_group_peers
            .contains(&old_member_1));
        assert!(close_group_shunned
            .old_close_group_peers
            .contains(&old_member_2));

        // evict 1 more member
        close_group_shunned.update_close_group_peers(vec![
            close_group_shunned.close_group_peers[1],
            close_group_shunned.close_group_peers[2],
            close_group_shunned.close_group_peers[3],
            close_group_shunned.close_group_peers[4],
            PeerId::random(),
        ]);

        // the metric from the member_2 should be removed
        assert_eq!(close_group_shunned.metric_current_group.get(), 0);
        assert_eq!(close_group_shunned.metric_old_group.get(), 0);
        assert!(!close_group_shunned
            .old_close_group_peers
            .contains(&old_member_1));

        Ok(())
    }
}
