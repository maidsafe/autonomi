from antnode import AntNode
import os

def print_section(title):
    print(f"\n{'='*20} {title} {'='*20}")

# Example initial peers - note these may not be active
initial_peers = [
    "/ip4/142.93.37.4/udp/40184/quic-v1/p2p/12D3KooWPC8q7QGZsmuTtCYxZ2s3FPXPZcS8LVKkayXkVFkqDEQB",
    "/ip4/157.245.40.2/udp/33698/quic-v1/p2p/12D3KooWNyNNTGfwGf6fYyvrk4zp5EHxPhNDVNB25ZzEt2NXbCq2",
    "/ip4/157.245.40.2/udp/33991/quic-v1/p2p/12D3KooWHPyZVAHqp2ebzKyxxsYzJYS7sNysfcLg2s1JLtbo6vhC"
]

def demonstrate_basic_node_operations():
    print_section("Basic Node Operations")
    
    # Create and start node
    node = AntNode()
    initial_rewards_address = "0x1234567890123456789012345678901234567890"
    print(f"Starting node with rewards address: {initial_rewards_address}")

    node.run(
        rewards_address=initial_rewards_address,
        evm_network="arbitrum_sepolia",
        ip="0.0.0.0",
        port=12000,
        initial_peers=initial_peers,
        local=True,
        root_dir=None,
    )

    # Get node information
    peer_id = node.peer_id()
    print(f"Node peer ID: {peer_id}")
    
    current_address = node.get_rewards_address()
    print(f"Current rewards address: {current_address}")
    
    return node, peer_id



def demonstrate_network_operations(node):
    print_section("Network Operations")
    
    try:
        # Get routing table information
        kbuckets = node.get_kbuckets()
        print("\nRouting table information:")
        for distance, peers in kbuckets:
            print(f"Distance {distance}: {len(peers)} peers")
            for peer in peers[:3]:  # Show first 3 peers at each distance
                print(f"  - {peer}")
    except Exception as e:
        print(f"Network operation failed: {e}")

def demonstrate_directory_management(node, peer_id):
    print_section("Directory Management")
    
    try:
        # Get various directory paths
        root_dir = node.get_root_dir()
        print(f"Current root directory: {root_dir}")
        
        logs_dir = node.get_logs_dir()
        print(f"Logs directory: {logs_dir}")
        
        data_dir = node.get_data_dir()
        print(f"Data directory: {data_dir}")
        
        # Get default directory for current peer
        default_dir = AntNode.get_default_root_dir(peer_id)
        print(f"Default root directory for peer {peer_id}: {default_dir}")
        
        # Demonstrate custom directory
        custom_dir = os.path.join(os.path.expanduser("~"), "antnode-test")
        print(f"\nStarting new node with custom directory: {custom_dir}")
        
        new_node = AntNode()
        new_node.run(
            rewards_address="0x1234567890123456789012345678901234567890",
            evm_network="arbitrum_sepolia",
            ip="0.0.0.0",
            port=12001,
            initial_peers=initial_peers,
            local=True,
            root_dir=custom_dir,
        )
        
        print(f"New node root directory: {new_node.get_root_dir()}")
        
    except Exception as e:
        print(f"Directory operation failed: {e}")

def main():
    try:
        # Basic setup and node operations
        node, peer_id = demonstrate_basic_node_operations()
        
    
        
        # Network operations
        demonstrate_network_operations(node)
        
        # Directory management
        demonstrate_directory_management(node, peer_id)
        
    except Exception as e:
        print(f"Example failed with error: {e}")

if __name__ == "__main__":
    main()
