// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Registry-based screen rendering tests
//!
//! Tests that validate screen rendering with actual mock node registry files.
//! These tests ensure the complete end-to-end flow from registry to UI rendering.

use ant_service_management::ServiceStatus;
use color_eyre::eyre;
use node_launchpad::{
    mode::Scene,
    test_utils::{JourneyBuilder, MockNodeRegistry, create_test_node_service_data},
};

#[tokio::test]
async fn test_registry_mixed_node_states() -> Result<(), eyre::Report> {
    let states = [
        ServiceStatus::Running,
        ServiceStatus::Stopped,
        ServiceStatus::Added,
        ServiceStatus::Running,
        ServiceStatus::Stopped,
    ];
    let mut registry = MockNodeRegistry::empty().expect("Failed to create empty registry");
    for (i, status) in states.iter().enumerate() {
        let node = create_test_node_service_data(i as u64, status.clone());
        registry
            .add_node(node)
            .expect("Failed to add node to registry");
    }

    JourneyBuilder::new_with_registry("Mixed Node States", registry)
        .await
        .expect("Failed to create journey")
        .start_from(Scene::Status)
        .expect_scene(Scene::Status)
        .expect_node_count_in_registry(5)?
        .expect_registry_contains("antnode-1")?
        .expect_registry_node_status("antnode-1", ServiceStatus::Running)?
        .expect_registry_contains("antnode-2")?
        .expect_registry_node_status("antnode-2", ServiceStatus::Stopped)?
        .expect_registry_contains("antnode-3")?
        .expect_registry_node_status("antnode-3", ServiceStatus::Added)?
        .expect_registry_contains("antnode-4")?
        .expect_registry_node_status("antnode-4", ServiceStatus::Running)?
        .expect_registry_contains("antnode-5")?
        .expect_registry_node_status("antnode-5", ServiceStatus::Stopped)?
        .expect_text("Nodes (5)")
        .run()
        .await
        .expect("Mixed states test failed");

    Ok(())
}

#[tokio::test]
async fn test_node_lifecycle_operations() -> Result<(), eyre::Report> {
    let registry = MockNodeRegistry::empty().expect("Failed to create empty registry");

    JourneyBuilder::new_with_registry("Node Lifecycle", registry)
        .await
        .expect("Failed to create journey")
        .start_from(Scene::Status)
        .expect_scene(Scene::Status)
        .expect_node_count_in_registry(0)?
        .step()
        // Add a node via registry
        .add_node_to_registry(create_test_node_service_data(0, ServiceStatus::Added))?
        .expect_node_count_in_registry(1)?
        .expect_registry_contains("antnode-1")?
        .expect_registry_node_status("antnode-1", ServiceStatus::Added)?
        .step()
        // Update node status to Running
        .update_registry_node_status("antnode-1", ServiceStatus::Running)?
        .expect_registry_node_status("antnode-1", ServiceStatus::Running)?
        .step()
        // Stop the node
        .update_registry_node_status("antnode-1", ServiceStatus::Stopped)?
        .expect_registry_node_status("antnode-1", ServiceStatus::Stopped)?
        .step()
        // Remove the node
        .remove_node_from_registry("antnode-1")?
        .expect_node_count_in_registry(0)?
        .expect_registry_not_contains("antnode-1")?
        .step()
        // Reset registry (should already be empty)
        .reset_registry()?
        .expect_registry_is_empty()?
        .run()
        .await
        .expect("Node lifecycle test failed");

    Ok(())
}
