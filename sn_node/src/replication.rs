// Copyright 2023 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::node::Node;
use crate::{error::Result, log_markers::Marker};
use libp2p::{
    kad::{Record, RecordKey, K_VALUE},
    PeerId,
};
use sn_networking::{sort_peers_by_address, GetQuorum, CLOSE_GROUP_SIZE};
use sn_protocol::{
    messages::{Cmd, Query, QueryResponse, Request, Response},
    NetworkAddress, PrettyPrintKBucketKey, PrettyPrintRecordKey,
};
use std::collections::BTreeMap;
use tokio::task::JoinHandle;

// To reduce the number of messages exchanged, patch max 500 replication keys into one request.
const MAX_REPLICATION_KEYS_PER_REQUEST: usize = 500;

impl Node {
    /// When there is PeerAdded or PeerRemoved, trigger replication, and replication target to be:
    /// 1, For PeerAdded(X), replicate any record that is now having X in its close_group
    ///    (from our knowledge) to that X
    /// 2, For PeerRemoved(X), replicate any record that previously having X in its close_group,
    ///    to that record's new close_group's farthest peer
    pub(crate) async fn try_trigger_replication(
        &self,
        peer_id: PeerId,
        is_removal: bool,
    ) -> Result<()> {
        // Already contains self_peer_id
        let mut all_peers = self.network.get_all_local_peers().await?;

        // Do not carry out replication if not many peers present.
        if all_peers.len() < K_VALUE.into() {
            trace!(
                "Not having enough peers to start replication: {:?}/{K_VALUE:?}",
                all_peers.len()
            );
            return Ok(());
        }

        self.record_metrics(Marker::ReplicationTriggered);

        let our_peer_id = self.network.peer_id;
        let our_address = NetworkAddress::from_peer(our_peer_id);

        let all_records = self.network.get_all_local_record_addresses().await?;

        trace!(
            "Replication triggered by the churning of {peer_id:?}, we have #{:?} records",
            all_records.len()
        );

        let mut replicate_to: BTreeMap<PeerId, Vec<NetworkAddress>> = Default::default();

        if is_removal {
            all_peers.push(peer_id);
        }

        for key in all_records {
            let sorted_based_on_key =
                sort_peers_by_address(&all_peers, &key, CLOSE_GROUP_SIZE + 1)?;
            let sorted_peers_pretty_print: Vec<_> = sorted_based_on_key
                .iter()
                .map(|&peer_id| {
                    format!(
                        "{peer_id:?}({:?})",
                        PrettyPrintKBucketKey(NetworkAddress::from_peer(*peer_id).as_kbucket_key())
                    )
                })
                .collect();

            if sorted_based_on_key.contains(&&peer_id) {
                trace!("replication: close for {key:?} are: {sorted_peers_pretty_print:?}");
                let target_peer = if is_removal {
                    // For dead peer, only replicate to farthest close_group peer,
                    // when the dead peer was one of the close_group peers to the record.
                    if let Some(&farthest_peer) = sorted_based_on_key.last() {
                        if *farthest_peer != peer_id {
                            *farthest_peer
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    }
                } else {
                    // For new peer, always replicate to it when it is close_group of the record.
                    if Some(&&peer_id) != sorted_based_on_key.last() {
                        peer_id
                    } else {
                        continue;
                    }
                };

                let keys_to_replicate = replicate_to.entry(target_peer).or_default();
                keys_to_replicate.push(key.clone());
            }
        }

        for (peer_id, keys) in replicate_to {
            let (_left, mut remaining_keys) = keys.split_at(0);
            while remaining_keys.len() > MAX_REPLICATION_KEYS_PER_REQUEST {
                let (left, right) = remaining_keys.split_at(MAX_REPLICATION_KEYS_PER_REQUEST);
                remaining_keys = right;
                self.send_replicate_cmd_without_wait(&our_address, &peer_id, left.to_vec())?;
            }
            self.send_replicate_cmd_without_wait(&our_address, &peer_id, remaining_keys.to_vec())?;
        }

        Ok(())
    }

    /// Add a list of keys to the Replication fetcher.
    /// These keys are later fetched from network.
    pub(crate) fn add_keys_to_replication_fetcher(
        &self,
        holder: PeerId,
        keys: Vec<NetworkAddress>,
    ) -> Result<()> {
        self.network.add_keys_to_replication_fetcher(holder, keys)?;
        Ok(())
    }

    /// Get the Record from a peer or from the network without waiting.
    pub(crate) fn fetch_replication_keys_without_wait(
        &self,
        keys_to_fetch: Vec<(PeerId, RecordKey)>,
    ) -> Result<()> {
        for (holder, key) in keys_to_fetch {
            let node = self.clone();
            let requester = NetworkAddress::from_peer(self.network.peer_id);
            let _handle: JoinHandle<Result<()>> = tokio::spawn(async move {
                let pretty_key = PrettyPrintRecordKey::from(key.clone());
                trace!("Fetching record {pretty_key:?} from node {holder:?}");
                let req = Request::Query(Query::GetReplicatedRecord {
                    requester,
                    key: NetworkAddress::from_record_key(key.clone()),
                });
                let record_opt = if let Ok(resp) = node.network.send_request(req, holder).await {
                    match resp {
                        Response::Query(QueryResponse::GetReplicatedRecord(result)) => match result
                        {
                            Ok((_holder, record_content)) => Some(record_content),
                            Err(err) => {
                                trace!("Failed fetch record {pretty_key:?} from node {holder:?}, with error {err:?}");
                                None
                            }
                        },
                        other => {
                            trace!("Cannot fetch record {pretty_key:?} from node {holder:?}, with response {other:?}");
                            None
                        }
                    }
                } else {
                    None
                };

                let record = if let Some(record_content) = record_opt {
                    Record::new(key, record_content)
                } else {
                    trace!(
                        "Can not fetch record {pretty_key:?} from node {holder:?}, fetching from the network"
                    );
                    node.network
                        .get_record_from_network(
                            key,
                            None,
                            GetQuorum::Majority,
                            false,
                            Default::default(),
                        )
                        .await?
                };

                trace!(
                    "Got Replication Record {pretty_key:?} from network, validating and storing it"
                );
                let _ = node.store_prepaid_record(record).await?;

                Ok(())
            });
        }
        Ok(())
    }

    // Utility to send `Cmd::Replicate` without awaiting for the `Response` at the call site.
    fn send_replicate_cmd_without_wait(
        &self,
        our_address: &NetworkAddress,
        peer_id: &PeerId,
        keys: Vec<NetworkAddress>,
    ) -> Result<()> {
        trace!(
            "Sending a replication list of {} keys to {peer_id:?} keys: {keys:?}",
            keys.len()
        );
        let request = Request::Cmd(Cmd::Replicate {
            holder: our_address.clone(),
            keys,
        });

        self.network.send_req_ignore_reply(request, *peer_id)?;

        Ok(())
    }
}
