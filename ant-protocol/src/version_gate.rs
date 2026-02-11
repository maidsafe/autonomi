// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Version gating module for peer version validation.
//!
//! This module provides functionality to parse and compare peer versions from
//! libp2p identify agent strings, enabling the network to enforce minimum
//! version requirements for connecting peers.
//!
//! # Agent String Format
//!
//! The expected agent string format is:
//! - Nodes: `ant/node/{protocol_version}/{node_version}/{network_id}`
//! - Clients: `ant/client/{protocol_version}/{client_version}/{network_id}`
//! - Reachability check peers: `ant/reachability-check-peer/{protocol_version}/{version}/{network_id}`
//!
//! Example: `ant/node/1.0/0.4.13/1`
//!
//! # Phase 2 Enforcement
//!
//! - **Nodes**: Version is enforced - peers below minimum or without version are rejected
//! - **Clients**: Version is NOT enforced - always allowed (metrics only)
//! - **Legacy peers**: Rejected (no grace period)

use std::fmt;

/// Minimum required node version for connecting to the network.
///
/// Nodes running versions below this will be disconnected (not blocklisted).
/// This can be overridden via the `ANT_MIN_NODE_VERSION` environment variable.
///
/// Format: (major, minor, patch)
pub const MIN_NODE_VERSION: (u16, u16, u16) = (0, 4, 15);

/// Get the minimum node version, checking environment variable override first.
///
/// Environment variable format: `ANT_MIN_NODE_VERSION=0.4.14`
pub fn get_min_node_version() -> PeerVersion {
    if let Ok(env_version) = std::env::var("ANT_MIN_NODE_VERSION")
        && let Some(version) = PeerVersion::parse_semver(&env_version)
    {
        return version;
    }
    PeerVersion::new(MIN_NODE_VERSION.0, MIN_NODE_VERSION.1, MIN_NODE_VERSION.2)
}

/// Represents a parsed semantic version from a peer's agent string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PeerVersion {
    /// Major version number
    pub major: u16,
    /// Minor version number
    pub minor: u16,
    /// Patch version number
    pub patch: u16,
}

impl PeerVersion {
    /// Creates a new PeerVersion.
    pub fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Parse version from an agent string.
    ///
    /// Expected formats:
    /// - `ant/node/{protocol_version}/{node_version}/{network_id}`
    /// - `ant/client/{protocol_version}/{client_version}/{network_id}`
    /// - `ant/reachability-check-peer/{protocol_version}/{version}/{network_id}`
    ///
    /// Returns `None` if the agent string doesn't match the expected format
    /// or if the version cannot be parsed.
    pub fn parse_from_agent_string(agent: &str) -> Option<Self> {
        let parts: Vec<&str> = agent.split('/').collect();

        // Expected: ["ant", "node"|"client", protocol_version, package_version, network_id]
        if parts.len() < 5 {
            return None;
        }

        // Verify it's an ant agent
        if parts[0] != "ant" {
            return None;
        }

        // parts[3] is the package version (e.g., "0.4.13")
        Self::parse_semver(parts[3])
    }

    /// Parse a semver string like "0.4.13" or "0.4.13-alpha.1"
    pub fn parse_semver(version_str: &str) -> Option<Self> {
        // Strip any pre-release suffix (e.g., "-alpha.1")
        let version_core = version_str.split('-').next()?;

        let parts: Vec<&str> = version_core.split('.').collect();
        if parts.len() < 3 {
            return None;
        }

        let major = parts[0].parse::<u16>().ok()?;
        let minor = parts[1].parse::<u16>().ok()?;
        let patch = parts[2].parse::<u16>().ok()?;

        Some(Self {
            major,
            minor,
            patch,
        })
    }

    /// Check if this version meets the minimum requirement.
    ///
    /// Returns `true` if `self >= min_version`.
    pub fn meets_minimum(&self, min_version: &PeerVersion) -> bool {
        (self.major, self.minor, self.patch)
            >= (min_version.major, min_version.minor, min_version.patch)
    }
}

impl fmt::Display for PeerVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The result of checking a peer's version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionCheckResult {
    /// Version meets minimum requirements.
    Accepted {
        /// The detected version
        version: PeerVersion,
    },
    /// Version is below minimum (with detected version).
    Rejected {
        /// The detected version that was rejected
        detected: PeerVersion,
        /// The minimum required version
        minimum: PeerVersion,
    },
    /// No version detected - legacy peer without version in agent string.
    Legacy,
    /// Could not parse version string.
    ParseError {
        /// The agent string that failed to parse
        agent_string: String,
    },
}

impl VersionCheckResult {
    /// Returns `true` if the version check passed (either accepted or legacy during grace period).
    pub fn is_allowed(&self, allow_legacy: bool) -> bool {
        match self {
            VersionCheckResult::Accepted { .. } => true,
            VersionCheckResult::Legacy => allow_legacy,
            VersionCheckResult::Rejected { .. } | VersionCheckResult::ParseError { .. } => false,
        }
    }
}

/// Identifies the type of peer from the agent string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PeerType {
    /// A network node
    Node,
    /// A client
    Client,
    /// A reachability check client (used for NAT traversal checks)
    ReachabilityCheckClient,
    /// Unknown peer type
    Unknown,
}

impl PeerType {
    /// Parse peer type from agent string.
    pub fn from_agent_string(agent: &str) -> Self {
        if agent.contains("/node/") {
            PeerType::Node
        } else if agent.contains("reachability-check-peer") {
            PeerType::ReachabilityCheckClient
        } else if agent.contains("/client/") || agent.contains("client") {
            PeerType::Client
        } else {
            PeerType::Unknown
        }
    }
}

impl fmt::Display for PeerType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Use UpperCamelCase for metric tag values
        match self {
            PeerType::Node => write!(f, "Node"),
            PeerType::Client => write!(f, "Client"),
            PeerType::ReachabilityCheckClient => write!(f, "ReachabilityCheckClient"),
            PeerType::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Check a peer's version against the minimum requirement.
///
/// # Arguments
/// * `agent_string` - The peer's agent version string from libp2p identify
/// * `min_version` - The minimum required version (if `None`, all versions are accepted)
///
/// # Returns
/// A `VersionCheckResult` indicating whether the peer should be allowed to connect.
pub fn check_peer_version(
    agent_string: &str,
    min_version: Option<&PeerVersion>,
) -> VersionCheckResult {
    // If no minimum version is set, accept all peers
    let Some(min_version) = min_version else {
        // Try to parse the version for metrics even if we're not enforcing
        if let Some(version) = PeerVersion::parse_from_agent_string(agent_string) {
            return VersionCheckResult::Accepted { version };
        }
        return VersionCheckResult::Legacy;
    };

    // Try to parse the version from the agent string
    match PeerVersion::parse_from_agent_string(agent_string) {
        Some(version) => {
            if version.meets_minimum(min_version) {
                VersionCheckResult::Accepted { version }
            } else {
                VersionCheckResult::Rejected {
                    detected: version,
                    minimum: *min_version,
                }
            }
        }
        None => {
            // Check if this looks like a legacy agent string (starts with "ant/")
            if agent_string.starts_with("ant/") {
                // It's an ant peer but we couldn't parse the version
                // This could be a legacy node or a malformed agent string
                VersionCheckResult::Legacy
            } else {
                VersionCheckResult::ParseError {
                    agent_string: agent_string.to_string(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_node_version() {
        let agent = "ant/node/1.0/0.4.13/1";
        let version = PeerVersion::parse_from_agent_string(agent).unwrap();
        assert_eq!(version, PeerVersion::new(0, 4, 13));
    }

    #[test]
    fn test_parse_client_version() {
        let agent = "ant/client/1.0/0.9.0/1";
        let version = PeerVersion::parse_from_agent_string(agent).unwrap();
        assert_eq!(version, PeerVersion::new(0, 9, 0));
    }

    #[test]
    fn test_parse_version_with_prerelease() {
        let version = PeerVersion::parse_semver("0.4.13-alpha.1").unwrap();
        assert_eq!(version, PeerVersion::new(0, 4, 13));
    }

    #[test]
    fn test_parse_legacy_agent() {
        // Old format without version
        let agent = "ant/1.0/1";
        assert!(PeerVersion::parse_from_agent_string(agent).is_none());
    }

    #[test]
    fn test_parse_invalid_agent() {
        let agent = "some-other-agent/1.0.0";
        assert!(PeerVersion::parse_from_agent_string(agent).is_none());
    }

    #[test]
    fn test_version_comparison() {
        let v1 = PeerVersion::new(0, 4, 10);
        let v2 = PeerVersion::new(0, 4, 13);
        let v3 = PeerVersion::new(0, 5, 0);
        let v4 = PeerVersion::new(1, 0, 0);

        assert!(v2.meets_minimum(&v1)); // 0.4.13 >= 0.4.10
        assert!(!v1.meets_minimum(&v2)); // 0.4.10 < 0.4.13
        assert!(v3.meets_minimum(&v2)); // 0.5.0 >= 0.4.13
        assert!(v4.meets_minimum(&v3)); // 1.0.0 >= 0.5.0
    }

    #[test]
    fn test_version_display() {
        let version = PeerVersion::new(0, 4, 13);
        assert_eq!(version.to_string(), "0.4.13");
    }

    #[test]
    fn test_check_peer_version_accepted() {
        let agent = "ant/node/1.0/0.4.13/1";
        let min = PeerVersion::new(0, 4, 10);
        let result = check_peer_version(agent, Some(&min));
        assert!(
            matches!(result, VersionCheckResult::Accepted { version } if version == PeerVersion::new(0, 4, 13))
        );
    }

    #[test]
    fn test_check_peer_version_rejected() {
        let agent = "ant/node/1.0/0.4.9/1";
        let min = PeerVersion::new(0, 4, 10);
        let result = check_peer_version(agent, Some(&min));
        assert!(
            matches!(result, VersionCheckResult::Rejected { detected, minimum }
            if detected == PeerVersion::new(0, 4, 9) && minimum == PeerVersion::new(0, 4, 10))
        );
    }

    #[test]
    fn test_check_peer_version_legacy() {
        let agent = "ant/1.0/1"; // Old format
        let min = PeerVersion::new(0, 4, 10);
        let result = check_peer_version(agent, Some(&min));
        assert!(matches!(result, VersionCheckResult::Legacy));
    }

    #[test]
    fn test_check_peer_version_no_minimum() {
        let agent = "ant/node/1.0/0.4.13/1";
        let result = check_peer_version(agent, None);
        assert!(matches!(result, VersionCheckResult::Accepted { .. }));
    }

    #[test]
    fn test_peer_type_detection() {
        assert_eq!(
            PeerType::from_agent_string("ant/node/1.0/0.4.13/1"),
            PeerType::Node
        );
        assert_eq!(
            PeerType::from_agent_string("ant/client/1.0/0.9.0/1"),
            PeerType::Client
        );
        assert_eq!(
            PeerType::from_agent_string("ant/reachability-check-peer/1.0/0.1.0/1"),
            PeerType::ReachabilityCheckClient
        );
        assert_eq!(
            PeerType::from_agent_string("unknown-agent"),
            PeerType::Unknown
        );
    }

    #[test]
    fn test_is_allowed() {
        let accepted = VersionCheckResult::Accepted {
            version: PeerVersion::new(0, 4, 13),
        };
        assert!(accepted.is_allowed(true));
        assert!(accepted.is_allowed(false));

        let legacy = VersionCheckResult::Legacy;
        assert!(legacy.is_allowed(true));
        assert!(!legacy.is_allowed(false));

        let rejected = VersionCheckResult::Rejected {
            detected: PeerVersion::new(0, 4, 9),
            minimum: PeerVersion::new(0, 4, 10),
        };
        assert!(!rejected.is_allowed(true));
        assert!(!rejected.is_allowed(false));
    }

    #[test]
    fn test_get_min_node_version() {
        // Without env var, should return the constant
        let min_version = get_min_node_version();
        assert_eq!(
            min_version,
            PeerVersion::new(MIN_NODE_VERSION.0, MIN_NODE_VERSION.1, MIN_NODE_VERSION.2)
        );
    }

    #[test]
    fn test_min_version_constant() {
        // Verify the constant is set correctly
        let (major, minor, patch) = MIN_NODE_VERSION;
        assert_eq!(major, 0);
        assert_eq!(minor, 4);
        assert_eq!(patch, 15);
    }

    // ============ Release Candidate (RC) Version Tests ============

    #[test]
    fn test_parse_rc_version_semver() {
        // RC versions should be parsed, stripping the -rc.X suffix
        let version = PeerVersion::parse_semver("0.4.14-rc.1").unwrap();
        assert_eq!(version, PeerVersion::new(0, 4, 14));

        let version = PeerVersion::parse_semver("0.4.14-rc.2").unwrap();
        assert_eq!(version, PeerVersion::new(0, 4, 14));

        let version = PeerVersion::parse_semver("1.0.0-rc.1").unwrap();
        assert_eq!(version, PeerVersion::new(1, 0, 0));
    }

    #[test]
    fn test_parse_rc_version_from_agent_string() {
        // RC versions in agent strings should be parsed correctly
        let agent = "ant/node/1.0/0.4.14-rc.1/1";
        let version = PeerVersion::parse_from_agent_string(agent).unwrap();
        assert_eq!(version, PeerVersion::new(0, 4, 14));

        let agent = "ant/node/1.0/0.5.0-rc.3/1";
        let version = PeerVersion::parse_from_agent_string(agent).unwrap();
        assert_eq!(version, PeerVersion::new(0, 5, 0));
    }

    #[test]
    fn test_rc_version_comparison() {
        // RC versions should compare based on their numeric part only
        let min_version = PeerVersion::new(0, 4, 10);

        // 0.4.14-rc.1 -> 0.4.14 >= 0.4.10 (should pass)
        let rc_version = PeerVersion::parse_semver("0.4.14-rc.1").unwrap();
        assert!(rc_version.meets_minimum(&min_version));

        // 0.4.9-rc.1 -> 0.4.9 < 0.4.10 (should fail)
        let old_rc_version = PeerVersion::parse_semver("0.4.9-rc.1").unwrap();
        assert!(!old_rc_version.meets_minimum(&min_version));

        // 0.4.10-rc.1 -> 0.4.10 >= 0.4.10 (should pass - equal)
        let exact_rc_version = PeerVersion::parse_semver("0.4.10-rc.1").unwrap();
        assert!(exact_rc_version.meets_minimum(&min_version));
    }

    #[test]
    fn test_check_peer_version_with_rc_accepted() {
        // Peer running RC version above minimum should be accepted
        let agent = "ant/node/1.0/0.4.14-rc.1/1";
        let min = PeerVersion::new(0, 4, 10);
        let result = check_peer_version(agent, Some(&min));

        // Should be accepted with version 0.4.14 (stripped of -rc.1)
        assert!(
            matches!(result, VersionCheckResult::Accepted { version } if version == PeerVersion::new(0, 4, 14))
        );
    }

    #[test]
    fn test_check_peer_version_with_rc_rejected() {
        // Peer running RC version below minimum should be rejected
        let agent = "ant/node/1.0/0.4.9-rc.2/1";
        let min = PeerVersion::new(0, 4, 10);
        let result = check_peer_version(agent, Some(&min));

        // Should be rejected with detected version 0.4.9
        assert!(
            matches!(result, VersionCheckResult::Rejected { detected, minimum }
                if detected == PeerVersion::new(0, 4, 9) && minimum == PeerVersion::new(0, 4, 10))
        );
    }

    #[test]
    fn test_various_prerelease_formats() {
        // Test various pre-release version formats
        assert_eq!(
            PeerVersion::parse_semver("0.4.14-rc.1").unwrap(),
            PeerVersion::new(0, 4, 14)
        );
        assert_eq!(
            PeerVersion::parse_semver("0.4.14-alpha.1").unwrap(),
            PeerVersion::new(0, 4, 14)
        );
        assert_eq!(
            PeerVersion::parse_semver("0.4.14-beta.2").unwrap(),
            PeerVersion::new(0, 4, 14)
        );
        assert_eq!(
            PeerVersion::parse_semver("0.4.14-dev").unwrap(),
            PeerVersion::new(0, 4, 14)
        );
        assert_eq!(
            PeerVersion::parse_semver("0.4.14-SNAPSHOT").unwrap(),
            PeerVersion::new(0, 4, 14)
        );
        // Multiple hyphens - only first part before hyphen is used
        assert_eq!(
            PeerVersion::parse_semver("0.4.14-rc.1-build.123").unwrap(),
            PeerVersion::new(0, 4, 14)
        );
    }
}
