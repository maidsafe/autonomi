// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_logging::ReloadHandle;
use ant_node::RunningNode;
use ant_protocol::antnode_proto::{
    ant_node_server::{AntNode, AntNodeServer},
    k_buckets_response, KBucketsRequest, KBucketsResponse, NetworkInfoRequest, NetworkInfoResponse,
    NodeEvent, NodeEventsRequest, NodeInfoRequest, NodeInfoResponse, RecordAddressesRequest,
    RecordAddressesResponse, RestartRequest, RestartResponse, StopRequest, StopResponse,
    UpdateLogLevelRequest, UpdateLogLevelResponse, UpdateRequest, UpdateResponse,
};
use ant_protocol::node_rpc::{NodeCtrl, StopResult};
use eyre::{ErrReport, Result};
use std::{
    collections::HashMap,
    env,
    net::SocketAddr,
    process,
    time::{Duration, Instant},
};
use tokio::sync::mpsc::{self, Sender};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{transport::Server, Code, Request, Response, Status};
use tracing::{debug, info};

// Defining a struct to hold information used by our gRPC service backend
struct SafeNodeRpcService {
    addr: SocketAddr,
    log_dir: String,
    running_node: RunningNode,
    ctrl_tx: Sender<NodeCtrl>,
    started_instant: Instant,
    log_reload_handle: ReloadHandle,
}

// Implementing RPC interface for service defined in .proto
#[tonic::async_trait]
impl AntNode for SafeNodeRpcService {
    type NodeEventsStream = ReceiverStream<Result<NodeEvent, Status>>;

    async fn node_info(
        &self,
        request: Request<NodeInfoRequest>,
    ) -> Result<Response<NodeInfoResponse>, Status> {
        debug!(
            "RPC request received at {}: {:?}",
            self.addr,
            request.get_ref()
        );

        let resp = Response::new(NodeInfoResponse {
            peer_id: self.running_node.peer_id().to_bytes(),
            log_dir: self.log_dir.clone(),
            data_dir: self
                .running_node
                .root_dir_path()
                .to_string_lossy()
                .to_string(),
            pid: process::id(),
            bin_version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_secs: self.started_instant.elapsed().as_secs(),
            wallet_balance: 0, // NB TODO: Implement this using metrics data?
        });

        Ok(resp)
    }

    async fn network_info(
        &self,
        request: Request<NetworkInfoRequest>,
    ) -> Result<Response<NetworkInfoResponse>, Status> {
        debug!(
            "RPC request received at {}: {:?}",
            self.addr,
            request.get_ref()
        );

        let state = self
            .running_node
            .get_swarm_local_state()
            .await
            .expect("failed to get local swarm state");
        let connected_peers = state.connected_peers.iter().map(|p| p.to_bytes()).collect();
        let listeners = state.listeners.iter().map(|m| m.to_string()).collect();

        let resp = Response::new(NetworkInfoResponse {
            connected_peers,
            listeners,
        });

        Ok(resp)
    }

    async fn node_events(
        &self,
        request: Request<NodeEventsRequest>,
    ) -> Result<Response<Self::NodeEventsStream>, Status> {
        debug!(
            "RPC request received at {}: {:?}",
            self.addr,
            request.get_ref()
        );

        let (client_tx, client_rx) = mpsc::channel(4);

        let mut events_rx = self.running_node.node_events_channel().subscribe();
        let _handle = tokio::spawn(async move {
            while let Ok(event) = events_rx.recv().await {
                let event_bytes = match event.to_bytes() {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        debug!(
                            "Error {err:?} while converting NodeEvent to bytes, ignoring the error"
                        );
                        continue;
                    }
                };

                let event = NodeEvent { event: event_bytes };

                if let Err(err) = client_tx.send(Ok(event)).await {
                    debug!(
                        "Dropping stream sender to RPC client due to failure in \
                        last attempt to notify an event: {err}"
                    );
                    break;
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(client_rx)))
    }

    async fn record_addresses(
        &self,
        request: Request<RecordAddressesRequest>,
    ) -> Result<Response<RecordAddressesResponse>, Status> {
        debug!(
            "RPC request received at {}: {:?}",
            self.addr,
            request.get_ref()
        );

        let addresses = self
            .running_node
            .get_all_record_addresses()
            .await
            .expect("failed to get record addresses")
            .into_iter()
            .map(|addr| addr.as_bytes())
            .collect();

        Ok(Response::new(RecordAddressesResponse { addresses }))
    }

    async fn k_buckets(
        &self,
        request: Request<KBucketsRequest>,
    ) -> Result<Response<KBucketsResponse>, Status> {
        debug!(
            "RPC request received at {}: {:?}",
            self.addr,
            request.get_ref()
        );

        let kbuckets: HashMap<u32, k_buckets_response::Peers> = self
            .running_node
            .get_kbuckets()
            .await
            .expect("failed to get k-buckets")
            .into_iter()
            .map(|(ilog2_distance, peers)| {
                let peers = peers.into_iter().map(|peer| peer.to_bytes()).collect();
                let peers = k_buckets_response::Peers { peers };
                (ilog2_distance, peers)
            })
            .collect();

        Ok(Response::new(KBucketsResponse { kbuckets }))
    }

    async fn stop(&self, request: Request<StopRequest>) -> Result<Response<StopResponse>, Status> {
        debug!(
            "RPC request received at {}: {:?}",
            self.addr,
            request.get_ref()
        );

        let cause = if let Some(addr) = request.remote_addr() {
            ErrReport::msg(format!(
                "Node has been stopped by an RPC request from {addr}."
            ))
        } else {
            ErrReport::msg("Node has been stopped by an RPC request from an unknown address.")
        };

        let delay = Duration::from_millis(request.get_ref().delay_millis);
        match self
            .ctrl_tx
            .send(NodeCtrl::Stop {
                delay,
                result: StopResult::Success(cause.to_string()),
            })
            .await
        {
            Ok(()) => Ok(Response::new(StopResponse {})),
            Err(err) => Err(Status::new(
                Code::Internal,
                format!("Failed to stop the node: {err}"),
            )),
        }
    }

    async fn restart(
        &self,
        request: Request<RestartRequest>,
    ) -> Result<Response<RestartResponse>, Status> {
        debug!(
            "RPC request received at {}: {:?}",
            self.addr,
            request.get_ref()
        );

        let delay = Duration::from_millis(request.get_ref().delay_millis);
        match self
            .ctrl_tx
            .send(NodeCtrl::Restart {
                delay,
                retain_peer_id: request.get_ref().retain_peer_id,
            })
            .await
        {
            Ok(()) => Ok(Response::new(RestartResponse {})),
            Err(err) => Err(Status::new(
                Code::Internal,
                format!("Failed to restart the node: {err}"),
            )),
        }
    }

    async fn update(
        &self,
        request: Request<UpdateRequest>,
    ) -> Result<Response<UpdateResponse>, Status> {
        debug!(
            "RPC request received at {}: {:?}",
            self.addr,
            request.get_ref()
        );

        let delay = Duration::from_millis(request.get_ref().delay_millis);
        match self.ctrl_tx.send(NodeCtrl::Update(delay)).await {
            Ok(()) => Ok(Response::new(UpdateResponse {})),
            Err(err) => Err(Status::new(
                Code::Internal,
                format!("Failed to update the node: {err}"),
            )),
        }
    }

    async fn update_log_level(
        &self,
        request: Request<UpdateLogLevelRequest>,
    ) -> Result<Response<UpdateLogLevelResponse>, Status> {
        debug!(
            "RPC request received at {}: {:?}",
            self.addr,
            request.get_ref()
        );

        match self
            .log_reload_handle
            .modify_log_level(&request.get_ref().log_level)
        {
            Ok(()) => Ok(Response::new(UpdateLogLevelResponse {})),
            Err(err) => Err(Status::new(
                Code::Internal,
                format!("Failed to update node's log level: {err:?}"),
            )),
        }
    }
}

pub(crate) fn start_rpc_service(
    addr: SocketAddr,
    log_dir_path: &str,
    running_node: RunningNode,
    ctrl_tx: Sender<NodeCtrl>,
    started_instant: Instant,
    log_reload_handle: ReloadHandle,
) {
    // creating a service
    let service = SafeNodeRpcService {
        addr,
        log_dir: log_dir_path.to_string(),
        running_node,
        ctrl_tx,
        started_instant,
        log_reload_handle,
    };
    info!("RPC Server listening on {addr}");
    println!("RPC Server listening on {addr}");

    let _handle = tokio::spawn(async move {
        // adding our service to our server.
        if let Err(e) = Server::builder()
            .add_service(AntNodeServer::new(service))
            .serve(addr)
            .await
        {
            error!("RPC Server failed to start: {e:?}");
        }
    });
}
