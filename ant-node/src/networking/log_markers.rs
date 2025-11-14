// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_evm::QuotingMetrics;
use libp2p::PeerId;
// this gets us to_string easily enough
use strum::Display;

/// Public Markers for generating log output,
/// These generate appropriate log level output and consistent strings.
///
/// Changing these log markers is a breaking change.
#[derive(Debug, Clone, Display, Copy)]
pub(crate) enum Marker<'a> {
    /// Close records held (Used in VDash)
    CloseRecordsLen(usize),
    /// Quoting metrics
    QuotingMetrics { quoting_metrics: &'a QuotingMetrics },
    /// The peer has been considered as bad
    PeerConsideredAsBad { bad_peer: &'a PeerId },
    /// We have been flagged as a bad node by a peer.
    FlaggedAsBadNode { flagged_by: &'a PeerId },
    /// Replicate candidates obtained
    ReplicateCandidatesObtained {
        length: usize,
        within_responsible_distance: bool,
    },
    /// Replication sender range check result
    ReplicationSenderInRange {
        sender: &'a PeerId,
        keys_count: usize,
        in_range: bool,
        network_under_load: bool,
    },
    /// Incoming replication keys statistics
    IncomingReplicationKeysStats {
        holder: PeerId,
        total_keys: usize,
        new_keys: usize,
        out_of_range_keys: usize,
    },
}

impl Marker<'_> {
    /// Returns the string representation of the LogMarker.
    pub(crate) fn log(&self) {
        if let Marker::IncomingReplicationKeysStats {
            holder: _,
            total_keys: _,
            new_keys,
            out_of_range_keys,
        } = self
        {
            if *out_of_range_keys > 0 || *new_keys > 0 {
                info!("{self:?}");
            }
        } else {
            info!("{self:?}");
        }
    }
}
