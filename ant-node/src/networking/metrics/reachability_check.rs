// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::ReachabilityStatus;
use prometheus_client::{
    encoding::{EncodeLabelSet, EncodeLabelValue},
    metrics::{family::Family, gauge::Gauge},
};

#[derive(EncodeLabelSet, Hash, Clone, Eq, PartialEq, Debug)]
pub(super) struct ReachabilityAdapterLabelSet {
    mode: ReachabilityAdapterLabelValue,
}

#[derive(EncodeLabelValue, Hash, Clone, Eq, PartialEq, Debug)]
pub(super) enum ReachabilityAdapterLabelValue {
    /// The external address is same as the local adapter address.
    Public,
    /// The external address is different from the local adapter address.
    Private,
    /// UPnP is supported.
    UPnP,
}

/// Used to denote the Reachable / NotReachable status of the node.
/// The modes denote if the external address is the same as the local adapter address and if UPnP is supported.
///
/// If all three are 0, then we are unreachable or reachability is not performed / in progress
/// If any of the three is 1, then we are reachable.
///
/// The progress of the reachability check is tracked via a different metric value.
pub(super) fn get_reachability_adapter_metric(
    metric: &Option<ReachabilityStatus>,
) -> Family<ReachabilityAdapterLabelSet, Gauge> {
    let family: Family<ReachabilityAdapterLabelSet, Gauge> = Family::default();

    let mut public = false;
    let mut private = false;
    let mut upnp_supported = false;

    if let Some(ReachabilityStatus::Reachable {
        local_addr,
        external_addr,
        upnp,
    }) = metric
    {
        public = local_addr == external_addr;
        private = local_addr != external_addr;
        upnp_supported = *upnp;
    }

    let bool_to_int = |b: bool| if b { 1 } else { 0 };

    let _ = family
        .get_or_create(&ReachabilityAdapterLabelSet {
            mode: ReachabilityAdapterLabelValue::Public,
        })
        .set(bool_to_int(public));
    let _ = family
        .get_or_create(&ReachabilityAdapterLabelSet {
            mode: ReachabilityAdapterLabelValue::Private,
        })
        .set(bool_to_int(private));
    let _ = family
        .get_or_create(&ReachabilityAdapterLabelSet {
            mode: ReachabilityAdapterLabelValue::UPnP,
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
        family: &Family<ReachabilityAdapterLabelSet, Gauge>,
        public: bool,
        private: bool,
        upnp: bool,
    ) {
        let bool_to_int = |b: bool| if b { 1 } else { 0 };

        assert_eq!(
            family
                .get_or_create(&ReachabilityAdapterLabelSet {
                    mode: ReachabilityAdapterLabelValue::Public,
                })
                .get(),
            bool_to_int(public),
            "Public metric mismatch"
        );

        assert_eq!(
            family
                .get_or_create(&ReachabilityAdapterLabelSet {
                    mode: ReachabilityAdapterLabelValue::Private,
                })
                .get(),
            bool_to_int(private),
            "Private metric mismatch"
        );

        assert_eq!(
            family
                .get_or_create(&ReachabilityAdapterLabelSet {
                    mode: ReachabilityAdapterLabelValue::UPnP,
                })
                .get(),
            bool_to_int(upnp),
            "UPnP metric mismatch"
        );
    }

    #[test]
    fn test_reachability_status_none() {
        let family = get_reachability_adapter_metric(&None);

        // All metrics should be 0 when no status is provided
        verify_metric(&family, false, false, false);
    }

    #[test]
    fn test_reachability_status_public_without_upnp() {
        let local_addr: SocketAddr = "192.168.1.100:8080".parse().unwrap();
        let external_addr = local_addr; // Same address means public
        let status = ReachabilityStatus::Reachable {
            local_addr,
            external_addr,
            upnp: false,
        };
        let family = get_reachability_adapter_metric(&Some(status));

        // Public should be 1, private 0, upnp 0
        verify_metric(&family, true, false, false);
    }

    #[test]
    fn test_reachability_status_private_without_upnp() {
        let local_addr: SocketAddr = "192.168.1.100:8080".parse().unwrap();
        let external_addr: SocketAddr = "203.0.113.1:8080".parse().unwrap(); // Different address means private
        let status = ReachabilityStatus::Reachable {
            local_addr,
            external_addr,
            upnp: false,
        };
        let family = get_reachability_adapter_metric(&Some(status));

        // Private should be 1, public 0, upnp 0
        verify_metric(&family, false, true, false);
    }

    #[test]
    fn test_reachability_status_private_with_upnp() {
        let local_addr: SocketAddr = "192.168.1.100:9090".parse().unwrap();
        let external_addr: SocketAddr = "203.0.113.1:9090".parse().unwrap();
        let status = ReachabilityStatus::Reachable {
            local_addr,
            external_addr,
            upnp: true,
        };
        let family = get_reachability_adapter_metric(&Some(status));

        // Private and UPnP should be 1, public 0
        verify_metric(&family, false, true, true);
    }

    #[test]
    fn test_reachability_status_not_reachable_without_upnp() {
        let mut reason_map = std::collections::HashMap::new();
        let _ = reason_map.insert(
            "127.0.0.1:8080".parse().unwrap(),
            ReachabilityIssue::NoDialBacks,
        );

        let status = ReachabilityStatus::NotReachable {
            upnp: false,
            reason: reason_map,
        };
        let family = get_reachability_adapter_metric(&Some(status));

        // All should be 0 when not reachable
        verify_metric(&family, false, false, false);
    }

    #[test]
    fn test_reachability_status_not_reachable_with_upnp() {
        let mut reason_map = std::collections::HashMap::new();
        let _ = reason_map.insert(
            "127.0.0.1:8080".parse().unwrap(),
            ReachabilityIssue::NoDialBacks,
        );

        let status = ReachabilityStatus::NotReachable {
            upnp: true,
            reason: reason_map,
        };
        let family = get_reachability_adapter_metric(&Some(status));

        // All should be 0 when not reachable (upnp doesn't matter if not reachable)
        verify_metric(&family, false, false, false);
    }

    #[test]
    fn test_multiple_family_instances_independence() {
        // Test that multiple family instances don't interfere with each other
        let local_addr: SocketAddr = "192.168.1.100:8080".parse().unwrap();
        let external_addr = local_addr;
        let status1 = ReachabilityStatus::Reachable {
            local_addr,
            external_addr,
            upnp: false,
        };
        let family1 = get_reachability_adapter_metric(&Some(status1));
        let family2 = get_reachability_adapter_metric(&None);

        // Verify family1 has public=1, others=0
        verify_metric(&family1, true, false, false);

        // Verify family2 has all=0
        verify_metric(&family2, false, false, false);
    }

    #[test]
    fn test_upnp_combinations() {
        // Test all combinations of status with UPnP enabled/disabled
        let local_addr: SocketAddr = "192.168.1.100:1234".parse().unwrap();
        let external_addr: SocketAddr = "203.0.113.1:1234".parse().unwrap();

        let mut reason_map1 = std::collections::HashMap::new();
        let _ = reason_map1.insert(
            "127.0.0.1:8080".parse().unwrap(),
            ReachabilityIssue::NoDialBacks,
        );
        let mut reason_map2 = std::collections::HashMap::new();
        let _ = reason_map2.insert(
            "127.0.0.1:8080".parse().unwrap(),
            ReachabilityIssue::NoDialBacks,
        );

        let test_cases = vec![
            (
                ReachabilityStatus::NotReachable {
                    upnp: true,
                    reason: reason_map1,
                },
                "NotReachable with UPnP",
                false, // public
                false, // private
                false, // upnp (not set when not reachable)
            ),
            (
                ReachabilityStatus::NotReachable {
                    upnp: false,
                    reason: reason_map2,
                },
                "NotReachable without UPnP",
                false, // public
                false, // private
                false, // upnp
            ),
            (
                ReachabilityStatus::Reachable {
                    local_addr,
                    external_addr: local_addr, // Same address = public
                    upnp: true,
                },
                "Reachable public with UPnP",
                true,  // public
                false, // private
                true,  // upnp
            ),
            (
                ReachabilityStatus::Reachable {
                    local_addr,
                    external_addr: local_addr, // Same address = public
                    upnp: false,
                },
                "Reachable public without UPnP",
                true,  // public
                false, // private
                false, // upnp
            ),
            (
                ReachabilityStatus::Reachable {
                    local_addr,
                    external_addr, // Different address = private
                    upnp: true,
                },
                "Reachable private with UPnP",
                false, // public
                true,  // private
                true,  // upnp
            ),
            (
                ReachabilityStatus::Reachable {
                    local_addr,
                    external_addr, // Different address = private
                    upnp: false,
                },
                "Reachable private without UPnP",
                false, // public
                true,  // private
                false, // upnp
            ),
        ];

        for (status, description, expected_public, expected_private, expected_upnp) in test_cases {
            let family = get_reachability_adapter_metric(&Some(status.clone()));

            verify_metric(&family, expected_public, expected_private, expected_upnp);

            println!("✓ Verified: {description}");
        }
    }
}
