// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_service_management::{ServiceStatus, StatusSummary};
use assert_cmd::Command;
use libp2p_identity::PeerId;
use std::path::PathBuf;

/// These tests need to execute as the root user.
///
/// They are intended to run on a CI-based environment with a fresh build agent because they will
/// create real services and user accounts, and will not attempt to clean themselves up.
///
/// They are assuming the existence of a `antnode` binary produced by the release process, and a
/// running local network, with ANT_PEERS set to a local node.
const CI_USER: &str = "runner";
#[cfg(unix)]
const ANTNODE_BIN_NAME: &str = "antnode";
#[cfg(windows)]
const ANTNODE_BIN_NAME: &str = "antnode.exe";

/// The default behaviour is for the service to run as the `safe` user, which gets created during
/// the process. However, there seems to be some sort of issue with adding user accounts on the GHA
/// build agent, so we will just tell it to use the `runner` user, which is the account for the
/// build agent.
#[test]
fn cross_platform_service_install_and_control() {
    let antnode_path = PathBuf::from("..")
        .join("target")
        .join("release")
        .join(ANTNODE_BIN_NAME);
    let mut cmd = Command::cargo_bin("antctl").unwrap();
    cmd.arg("add")
        .arg("--local")
        .arg("--user")
        .arg(CI_USER)
        .arg("--count")
        .arg("3")
        .arg("--path")
        .arg(antnode_path.to_string_lossy().to_string())
        .assert()
        .success();

    let registry = get_status();
    assert_eq!(registry.nodes[0].service_name, "antnode1");
    assert_eq!(registry.nodes[0].peer_id, None);
    assert_eq!(registry.nodes[0].status, ServiceStatus::Added);
    assert_eq!(registry.nodes[1].service_name, "antnode2");
    assert_eq!(registry.nodes[1].peer_id, None);
    assert_eq!(registry.nodes[1].status, ServiceStatus::Added);
    assert_eq!(registry.nodes[2].service_name, "antnode3");
    assert_eq!(registry.nodes[2].peer_id, None);
    assert_eq!(registry.nodes[2].status, ServiceStatus::Added);

    // Start each of the three services.
    let mut cmd = Command::cargo_bin("antctl").unwrap();
    cmd.arg("start").assert().success();

    // After `start`, all services should be running with valid peer IDs assigned.
    let registry = get_status();
    assert_eq!(registry.nodes[0].service_name, "antnode1");
    assert_eq!(registry.nodes[0].status, ServiceStatus::Running);
    assert_eq!(registry.nodes[1].service_name, "antnode2");
    assert_eq!(registry.nodes[1].status, ServiceStatus::Running);
    assert_eq!(registry.nodes[2].service_name, "antnode3");
    assert_eq!(registry.nodes[2].status, ServiceStatus::Running);

    // The three peer IDs should persist throughout the rest of the test.
    let peer_ids = registry
        .nodes
        .iter()
        .map(|n| n.peer_id)
        .collect::<Vec<Option<PeerId>>>();

    // Stop each of the three services.
    let mut cmd = Command::cargo_bin("antctl").unwrap();
    cmd.arg("stop").assert().success();

    // After `stop`, all services should be stopped with peer IDs retained.
    let registry = get_status();
    assert_eq!(registry.nodes[0].service_name, "antnode1");
    assert_eq!(registry.nodes[0].status, ServiceStatus::Stopped);
    assert_eq!(registry.nodes[0].peer_id, peer_ids[0]);
    assert_eq!(registry.nodes[1].service_name, "antnode2");
    assert_eq!(registry.nodes[1].status, ServiceStatus::Stopped);
    assert_eq!(registry.nodes[1].peer_id, peer_ids[1]);
    assert_eq!(registry.nodes[2].service_name, "antnode3");
    assert_eq!(registry.nodes[2].status, ServiceStatus::Stopped);
    assert_eq!(registry.nodes[2].peer_id, peer_ids[2]);

    // Start each of the three services again.
    let mut cmd = Command::cargo_bin("antctl").unwrap();
    cmd.arg("start").assert().success();

    // Peer IDs again should be retained after restart.
    let registry = get_status();
    assert_eq!(registry.nodes[0].service_name, "antnode1");
    assert_eq!(registry.nodes[0].status, ServiceStatus::Running);
    assert_eq!(registry.nodes[0].peer_id, peer_ids[0]);
    assert_eq!(registry.nodes[1].service_name, "antnode2");
    assert_eq!(registry.nodes[1].status, ServiceStatus::Running);
    assert_eq!(registry.nodes[1].peer_id, peer_ids[1]);
    assert_eq!(registry.nodes[2].service_name, "antnode3");
    assert_eq!(registry.nodes[2].status, ServiceStatus::Running);
    assert_eq!(registry.nodes[2].peer_id, peer_ids[2]);

    // Stop two nodes by peer ID.
    let mut cmd = Command::cargo_bin("antctl").unwrap();
    cmd.arg("stop")
        .arg("--peer-id")
        .arg(registry.nodes[0].peer_id.unwrap().to_string())
        .arg("--peer-id")
        .arg(registry.nodes[2].peer_id.unwrap().to_string())
        .assert()
        .success();

    // Peer IDs again should be retained after restart.
    let registry = get_status();
    assert_eq!(registry.nodes[0].service_name, "antnode1");
    assert_eq!(registry.nodes[0].status, ServiceStatus::Stopped);
    assert_eq!(registry.nodes[0].peer_id, peer_ids[0]);
    assert_eq!(registry.nodes[1].service_name, "antnode2");
    assert_eq!(registry.nodes[1].status, ServiceStatus::Running);
    assert_eq!(registry.nodes[1].peer_id, peer_ids[1]);
    assert_eq!(registry.nodes[2].service_name, "antnode3");
    assert_eq!(registry.nodes[2].status, ServiceStatus::Stopped);
    assert_eq!(registry.nodes[2].peer_id, peer_ids[2]);

    // Now restart the stopped nodes by service name.
    let mut cmd = Command::cargo_bin("antctl").unwrap();
    cmd.arg("start")
        .arg("--service-name")
        .arg(registry.nodes[0].service_name.clone())
        .arg("--service-name")
        .arg(registry.nodes[2].service_name.clone())
        .assert()
        .success();

    // The stopped nodes should now be running again.
    let registry = get_status();
    assert_eq!(registry.nodes[0].service_name, "antnode1");
    assert_eq!(registry.nodes[0].status, ServiceStatus::Running);
    assert_eq!(registry.nodes[0].peer_id, peer_ids[0]);
    assert_eq!(registry.nodes[1].service_name, "antnode2");
    assert_eq!(registry.nodes[1].status, ServiceStatus::Running);
    assert_eq!(registry.nodes[1].peer_id, peer_ids[1]);
    assert_eq!(registry.nodes[2].service_name, "antnode3");
    assert_eq!(registry.nodes[2].status, ServiceStatus::Running);
    assert_eq!(registry.nodes[2].peer_id, peer_ids[2]);

    // Finally, stop each of the three services.
    let mut cmd = Command::cargo_bin("antctl").unwrap();
    cmd.arg("stop").assert().success();

    // After `stop`, all services should be stopped with peer IDs retained.
    let registry = get_status();
    assert_eq!(registry.nodes[0].service_name, "antnode1");
    assert_eq!(registry.nodes[0].status, ServiceStatus::Stopped);
    assert_eq!(registry.nodes[0].peer_id, peer_ids[0]);
    assert_eq!(registry.nodes[1].service_name, "antnode2");
    assert_eq!(registry.nodes[1].status, ServiceStatus::Stopped);
    assert_eq!(registry.nodes[1].peer_id, peer_ids[1]);
    assert_eq!(registry.nodes[2].service_name, "antnode3");
    assert_eq!(registry.nodes[2].status, ServiceStatus::Stopped);
    assert_eq!(registry.nodes[2].peer_id, peer_ids[2]);

    // Remove two nodes.
    let mut cmd = Command::cargo_bin("antctl").unwrap();
    cmd.arg("remove")
        .arg("--service-name")
        .arg(registry.nodes[0].service_name.clone())
        .arg("--service-name")
        .arg(registry.nodes[1].service_name.clone())
        .assert()
        .success();
    let registry = get_status();
    assert_eq!(
        1,
        registry
            .nodes
            .iter()
            .filter(|n| n.status != ServiceStatus::Removed)
            .count()
    );
}

fn get_status() -> StatusSummary {
    let output = Command::cargo_bin("antctl")
        .unwrap()
        .arg("status")
        .arg("--json")
        .output()
        .expect("Could not retrieve service status");
    let output = String::from_utf8_lossy(&output.stdout).to_string();
    serde_json::from_str(&output).unwrap()
}
