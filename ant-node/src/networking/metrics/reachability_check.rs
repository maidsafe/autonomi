// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use prometheus_client::{
    encoding::{EncodeLabelSet, EncodeLabelValue},
    metrics::{family::Family, gauge::Gauge},
};

use crate::ReachabilityStatus;

/// Public input for the reachability check metric.
pub(crate) enum ReachabilityStatusMetric {
    Ongoing,
    Status(ReachabilityStatus),
    NotPerformed,
}

#[derive(EncodeLabelSet, Hash, Clone, Eq, PartialEq, Debug)]
pub(super) struct ReachabilityStatusLabelSet {
    status: ReachabilityStatusLabelValue,
}

#[derive(EncodeLabelValue, Hash, Clone, Eq, PartialEq, Debug)]
pub(super) enum ReachabilityStatusLabelValue {
    NotPerformed,
    Ongoing,
    Reachable,
    NotRoutable,
    UPnPSupported,
}

pub(super) fn get_reachability_status_metric(
    metric: ReachabilityStatusMetric,
) -> Family<ReachabilityStatusLabelSet, Gauge> {
    let family: Family<ReachabilityStatusLabelSet, Gauge> = Family::default();

    let mut not_performed = false;
    let mut ongoing = false;
    let mut reachable = false;
    let mut not_routable = false;
    let mut upnp_supported = false;

    match metric {
        ReachabilityStatusMetric::NotPerformed => not_performed = true,
        ReachabilityStatusMetric::Ongoing => {
            ongoing = true;
        }
        ReachabilityStatusMetric::Status(status) => {
            reachable = status.is_reachable();
            not_routable = status.is_not_reachable();
            upnp_supported = status.upnp_supported();
        }
    }

    let bool_to_int = |b: bool| if b { 1 } else { 0 };

    let _ = family
        .get_or_create(&ReachabilityStatusLabelSet {
            status: ReachabilityStatusLabelValue::NotPerformed,
        })
        .set(bool_to_int(not_performed));
    let _ = family
        .get_or_create(&ReachabilityStatusLabelSet {
            status: ReachabilityStatusLabelValue::Ongoing,
        })
        .set(bool_to_int(ongoing));
    let _ = family
        .get_or_create(&ReachabilityStatusLabelSet {
            status: ReachabilityStatusLabelValue::Reachable,
        })
        .set(bool_to_int(reachable));
    let _ = family
        .get_or_create(&ReachabilityStatusLabelSet {
            status: ReachabilityStatusLabelValue::NotRoutable,
        })
        .set(bool_to_int(not_routable));
    let _ = family
        .get_or_create(&ReachabilityStatusLabelSet {
            status: ReachabilityStatusLabelValue::UPnPSupported,
        })
        .set(bool_to_int(upnp_supported));

    family
}

#[cfg(test)]
mod tests {
    use crate::networking::ReachabilityIssue;

    use super::*;
    use std::net::SocketAddr;

    /// Helper function to verify metric values in the family
    fn verify_metric(
        family: &Family<ReachabilityStatusLabelSet, Gauge>,
        ongoing: bool,
        not_performed: bool,
        reachable: bool,
        not_routable: bool,
        upnp_supported: bool,
    ) {
        let bool_to_int = |b: bool| if b { 1 } else { 0 };

        assert_eq!(
            family
                .get_or_create(&ReachabilityStatusLabelSet {
                    status: ReachabilityStatusLabelValue::Ongoing,
                })
                .get(),
            bool_to_int(ongoing),
            "Ongoing metric mismatch"
        );

        assert_eq!(
            family
                .get_or_create(&ReachabilityStatusLabelSet {
                    status: ReachabilityStatusLabelValue::NotPerformed,
                })
                .get(),
            bool_to_int(not_performed),
            "NotPerformed metric mismatch"
        );

        assert_eq!(
            family
                .get_or_create(&ReachabilityStatusLabelSet {
                    status: ReachabilityStatusLabelValue::Reachable,
                })
                .get(),
            bool_to_int(reachable),
            "Reachable metric mismatch"
        );

        assert_eq!(
            family
                .get_or_create(&ReachabilityStatusLabelSet {
                    status: ReachabilityStatusLabelValue::NotRoutable,
                })
                .get(),
            bool_to_int(not_routable),
            "NotRoutable metric mismatch"
        );

        assert_eq!(
            family
                .get_or_create(&ReachabilityStatusLabelSet {
                    status: ReachabilityStatusLabelValue::UPnPSupported,
                })
                .get(),
            bool_to_int(upnp_supported),
            "UpnPSupported metric mismatch"
        );
    }

    #[test]
    fn test_reachability_status_not_performed() {
        let family = get_reachability_status_metric(ReachabilityStatusMetric::NotPerformed);

        // Only NotPerformed should be 1, all others 0
        verify_metric(&family, false, true, false, false, false);
    }

    #[test]
    fn test_reachability_status_ongoing() {
        let family = get_reachability_status_metric(ReachabilityStatusMetric::Ongoing);

        // Only Ongoing should be 1, all others 0
        verify_metric(&family, true, false, false, false, false);
    }

    #[test]
    fn test_reachability_status_reachable_without_upnp() {
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let status = ReachabilityStatus::Reachable { addr, upnp: false };
        let family = get_reachability_status_metric(ReachabilityStatusMetric::Status(status));

        // Only Reachable should be 1, UpnP should be 0
        verify_metric(&family, false, false, true, false, false);
    }

    #[test]
    fn test_reachability_status_reachable_with_upnp() {
        let addr: SocketAddr = "192.168.1.100:9090".parse().unwrap();
        let status = ReachabilityStatus::Reachable { addr, upnp: true };
        let family = get_reachability_status_metric(ReachabilityStatusMetric::Status(status));

        // Both Reachable and UpnPSupported should be 1
        verify_metric(&family, false, false, true, false, true);
    }

    #[test]
    fn test_reachability_status_not_routable_without_upnp() {
        let status = ReachabilityStatus::NotReachable {
            upnp: false,
            reason: ReachabilityIssue::NoDialBacks,
        };
        let family = get_reachability_status_metric(ReachabilityStatusMetric::Status(status));

        // Only NotRoutable should be 1, UpnP should be 0
        verify_metric(&family, false, false, false, true, false);
    }

    #[test]
    fn test_reachability_status_not_routable_with_upnp() {
        let status = ReachabilityStatus::NotReachable {
            upnp: true,
            reason: ReachabilityIssue::NoDialBacks,
        };
        let family = get_reachability_status_metric(ReachabilityStatusMetric::Status(status));

        // Both NotRoutable and UpnPSupported should be 1
        verify_metric(&family, false, false, false, true, true);
    }

    #[test]
    fn test_multiple_family_instances_independence() {
        // Test that multiple family instances don't interfere with each other
        let family1 = get_reachability_status_metric(ReachabilityStatusMetric::Ongoing);
        let family2 = get_reachability_status_metric(ReachabilityStatusMetric::NotPerformed);

        // Verify family1 has ongoing=1, all others=0
        verify_metric(&family1, true, false, false, false, false);

        // Verify family2 has not_performed=1, all others=0
        verify_metric(&family2, false, true, false, false, false);
    }

    #[test]
    fn test_upnp_combinations() {
        // Test all combinations of status with UPnP enabled/disabled
        let test_cases = vec![
            (
                ReachabilityStatus::NotReachable {
                    upnp: true,
                    reason: ReachabilityIssue::NoDialBacks,
                },
                "NotRoutable with UPnP",
            ),
            (
                ReachabilityStatus::NotReachable {
                    upnp: false,
                    reason: ReachabilityIssue::NoDialBacks,
                },
                "NotRoutable without UPnP",
            ),
            (
                ReachabilityStatus::Reachable {
                    addr: "10.0.0.1:1234".parse().unwrap(),
                    upnp: true,
                },
                "Reachable with UPnP",
            ),
            (
                ReachabilityStatus::Reachable {
                    addr: "10.0.0.1:1234".parse().unwrap(),
                    upnp: false,
                },
                "Reachable without UPnP",
            ),
        ];

        for (status, description) in test_cases {
            let family =
                get_reachability_status_metric(ReachabilityStatusMetric::Status(status.clone()));

            let expected_upnp = status.upnp_supported();
            let expected_reachable = status.is_reachable();
            let expected_not_routable = status.is_not_reachable();

            verify_metric(
                &family,
                false, // ongoing
                false, // not_performed
                expected_reachable,
                expected_not_routable,
                expected_upnp,
            );

            println!("✓ Verified: {description}");
        }
    }
}
