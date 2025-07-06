use ant_service_management::antctl_proto::ant_ctl_client::AntCtlClient;
use ant_service_management::antctl_proto::NodeServiceRestartRequest;
use color_eyre::eyre::bail;
use color_eyre::{eyre::eyre, Result};
use libp2p_identity::PeerId;
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;
use tonic::transport::Channel;
use tonic::Request;

struct DaemonRpcClient {
    addr: SocketAddr,
    rpc: AntCtlClient<Channel>,
}

pub async fn restart_node(
    peer_ids: Vec<String>,
    rpc_server_address: SocketAddr,
    retain_peer_id: bool,
) -> Result<()> {
    for peer_id in peer_ids {
        debug!("Sending NodeServiceRestartRequest to {peer_id:?} at {rpc_server_address:?}");
        let str_bytes = PeerId::from_str(&peer_id)?.to_bytes();

        let mut daemon_client = get_rpc_client(rpc_server_address).await?;

        let _response = daemon_client
            .rpc
            .restart_node_service(Request::new(NodeServiceRestartRequest {
                peer_id: str_bytes,
                delay_millis: 0,
                retain_peer_id,
            }))
            .await
            .map_err(|err| {
                error!("Failed to restart node service with {peer_id:?} at {rpc_server_address:?} with err: {err:?}");
                eyre!(
                    "Failed to restart node service with {peer_id:?} at {:?} with err: {err:?}",
                    daemon_client.addr
                )
            })?;
    }
    Ok(())
}

async fn get_rpc_client(socket_addr: SocketAddr) -> Result<DaemonRpcClient> {
    let endpoint = format!("https://{socket_addr}");
    let mut attempts = 0;
    loop {
        if let Ok(rpc_client) = AntCtlClient::connect(endpoint.clone()).await {
            let rpc_client = DaemonRpcClient {
                addr: socket_addr,
                rpc: rpc_client,
            };
            return Ok(rpc_client);
        }
        attempts += 1;
        error!("Could not connect to rpc {endpoint:?}. Attempts: {attempts:?}/10");
        println!("Could not connect to rpc {endpoint:?}. Attempts: {attempts:?}/10");
        tokio::time::sleep(Duration::from_secs(1)).await;
        if attempts >= 10 {
            error!("Failed to connect to {endpoint:?} even after 10 retries");
            bail!("Failed to connect to {endpoint:?} even after 10 retries");
        }
    }
}
