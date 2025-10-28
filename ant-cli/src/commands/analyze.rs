// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::actions::NetworkContext;
use autonomi::{Multiaddr, RewardsAddress, SecretKey, Wallet, client::analyze::AnalysisError,
    networking::NetworkAddress};
use color_eyre::eyre::Result;
use std::str::FromStr;

pub async fn analyze(
    addr: &str,
    closest_nodes: bool,
    verbose: bool,
    network_context: NetworkContext,
) -> Result<()> {
    macro_rules! println_if_verbose {
        ($($arg:tt)*) => {
            if verbose {
                println!($($arg)*);
            }
        };
    }
    println_if_verbose!("Analyzing address: {}", addr);

    // then connect to network and check data
    let client = crate::actions::connect_to_network(network_context)
        .await
        .map_err(|(err, _)| err)?;

    if closest_nodes {
        print_closest_nodes(&client, addr, verbose).await?;
    }

    let analysis = client.analyze_address(addr, verbose).await;
    match analysis {
        Ok(analysis) => {
            println_if_verbose!("Analysis successful");
            println!("{analysis}");
        }
        Err(AnalysisError::UnrecognizedInput) => {
            println!("🚨 Could not identify address type!");
            println_if_verbose!(
                "Provided string was not recognized as a data address, trying other types..."
            );
            try_other_types(addr, verbose);
        }
        Err(e) => {
            println!("Analysis inconclusive: {e}");
        }
    }

    Ok(())
}

fn try_other_types(addr: &str, verbose: bool) {
    macro_rules! println_if_verbose {
        ($($arg:tt)*) => {
            if verbose {
                println!($($arg)*);
            }
        };
    }

    // local reference to private data
    let try_private_address = crate::user_data::get_local_private_archive_access(addr).ok();
    if let Some(data_map) = try_private_address {
        println!(
            "✅ Identified input as a: Local Private Archive's DataMap local address (only works on your own machine)"
        );
        println_if_verbose!(
            "💡 This local address points to a DataMap which is stored on your local machine."
        );
        println_if_verbose!(
            "💡 Using this DataMap you can download your Private Archive from the Network."
        );
        println_if_verbose!(
            "💡 You can use the `file download` command to download the private data from the DataMap"
        );
        println!("DataMap in hex: {}", data_map.to_hex());
        return;
    }

    // cryptographic keys
    let hex_addr = addr.trim_start_matches("0x");
    let maybe_secret_key = SecretKey::from_hex(hex_addr).ok();
    let maybe_eth_sk = Wallet::new_from_private_key(Default::default(), hex_addr).ok();
    if maybe_secret_key.is_some() || maybe_eth_sk.is_some() {
        println!("🚨 Please keep your secret key safe! Don't use it as a data address!");
        println!("✅ Identified input as a: Secret Key");
        println_if_verbose!("💡 A Secret Key is used to sign data or transactions on the Network.");
        return;
    }
    let maybe_eth_address = addr.parse::<RewardsAddress>().ok();
    if maybe_eth_address.is_some() {
        println!("✅ Identified input as an: Ethereum Address");
        println_if_verbose!(
            "💡 An Ethereum address is a cryptographic identifier for a blockchain account. It can be used to receive funds and rewards on the Network."
        );
        return;
    }

    // multiaddrs
    let maybe_multiaddr = Multiaddr::from_str(addr).ok();
    if maybe_multiaddr.is_some() {
        println!("✅ Identified input as a: Multiaddr");
        println_if_verbose!("💡 A Mutliaddr is the url used to connect to a node on the Network.");
        return;
    }

    println!("⚠️ Unrecognized input");
}

async fn print_closest_nodes(client: &autonomi::Client, addr: &str, verbose: bool) -> Result<()> {
    use autonomi::PublicKey;
    use autonomi::chunk::ChunkAddress;
    use autonomi::graph::GraphEntryAddress;

    macro_rules! println_if_verbose {
        ($($arg:tt)*) => {
            if verbose {
                println!($($arg)*);
            }
        };
    }

    let hex_addr = addr.trim_start_matches("0x");

    println_if_verbose!("Querying closest peers to address...");

    // Try parsing as ChunkAddress (XorName) first
    let target_addr = if let Ok(chunk_addr) = ChunkAddress::from_hex(addr) {
        println_if_verbose!("Identified as ChunkAddress");
        NetworkAddress::from(chunk_addr)
    // Try parsing as PublicKey (could be GraphEntry, Pointer, or Scratchpad)
    } else if let Ok(public_key) = PublicKey::from_hex(hex_addr) {
        println_if_verbose!("Identified as PublicKey, using GraphEntryAddress");
        // Default to GraphEntryAddress for public keys
        NetworkAddress::from(GraphEntryAddress::new(public_key))
    } else {
        return Err(color_eyre::eyre::eyre!(
            "Could not parse address. Expected a hex-encoded ChunkAddress or PublicKey"
        ));
    };

    // Get closest group to the target addr
    let peers = client
            .get_closest_to_address(target_addr.clone())
            .await
            .map_err(|e| color_eyre::eyre::eyre!("Failed to get closest peers: {e}"))?;

    // Sort peers by distance to target address
    let mut sorted_peers = peers;
    sorted_peers.sort_by_key(|peer| {
        let peer_addr = NetworkAddress::from(peer.peer_id);
        target_addr.distance(&peer_addr)
    });

    println!("Found {} closest peers to {}:", sorted_peers.len(), addr);
    println!();

    // Check holding status for each peer
    for (i, peer) in sorted_peers.iter().enumerate() {
        let peer_addr = NetworkAddress::from(peer.peer_id);
        let distance = target_addr.distance(&peer_addr);
        
        println!("{}. Peer ID: {} (distance: {distance:?}[{:?}])", i + 1, peer.peer_id, distance.ilog2());
        
        // Query the peer directly to check if it holds the record
        match client.get_record_from_peer(target_addr.clone(), peer.clone()).await {
            Ok(Some(record)) => {
                println!("   Status: ✅ HOLDING record (size: {} bytes)", record.value.len());
            }
            Ok(None) => {
                println!("   Status: ❌ NOT holding record");
            }
            Err(e) => {
                println!("   Status: ⚠️  Failed to query: {e}");
            }
        }
        
        if verbose {
            println!("   Addresses:");
            for addr in &peer.addrs {
                println!("     - {addr}");
            }
        }
        println!();
    }

    // Print 2-D distance matrix among sorted peers
    println!("\n{}", "=".repeat(80));
    println!("Distance Matrix Among Closest Peers:");
    println!("{}", "=".repeat(80));
    println!();

    // Print header row with peer indices
    print!("     ");
    for i in 0..sorted_peers.len() {
        print!("Peer {:2} ", i + 1);
    }
    println!();
    print!("     ");
    for _ in 0..sorted_peers.len() {
        print!("{} ", "-".repeat(7));
    }
    println!();

    // Print each row of the distance matrix
    for (i, peer_i) in sorted_peers.iter().enumerate() {
        print!("P{:2}  ", i + 1);
        let addr_i = NetworkAddress::from(peer_i.peer_id);
        
        for peer_j in &sorted_peers {
            let addr_j = NetworkAddress::from(peer_j.peer_id);
            let distance = addr_i.distance(&addr_j);
            
            // Display distance with ilog2 for readability
            if addr_i == addr_j {
                print!("   -    ");
            } else {
                print!("{:?} ", distance.ilog2());
            }
        }
        println!();
    }

    Ok(())
}
