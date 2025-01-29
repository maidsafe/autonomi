// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::exit_code;
use crate::network::NetworkPeers;
use crate::output::collect_task_summary;
use crate::output::FileUploadOutput;
use crate::wallet::load_wallet;
use autonomi::client::address::addr_to_str;
use autonomi::client::config::ClientOperationConfig;
use autonomi::ResponseQuorum;
use color_eyre::eyre::Context;
use color_eyre::eyre::Result;
use color_eyre::Section;
use std::path::PathBuf;
use std::process;
use std::time::SystemTime;

pub async fn cost(file: &str, peers: NetworkPeers) -> Result<()> {
    let client = crate::actions::connect_to_network(peers, Default::default()).await?;

    println!("Getting upload cost...");
    info!("Calculating cost for file: {file}");
    let cost = client
        .file_cost(&PathBuf::from(file))
        .await
        .wrap_err("Failed to calculate cost for file")?;

    println!("Estimate cost to upload file: {file}");
    println!("Total cost: {cost}");
    info!("Total cost: {cost} for file: {file}");
    Ok(())
}

pub async fn upload(
    file: &str,
    public: bool,
    peers: NetworkPeers,
    verification_quorum: Option<ResponseQuorum>,
    json: bool,
) -> Result<()> {
    let start = SystemTime::now();
    let mut client_operation_config = ClientOperationConfig::default();
    if let Some(verification_quorum) = verification_quorum {
        client_operation_config.chunk_verification_quorum(verification_quorum);
    }
    let mut client = crate::actions::connect_to_network(peers, client_operation_config).await?;

    let wallet = load_wallet(client.get_evm_network())?;
    let event_receiver = client.enable_client_events();
    let (task_summary_thread, stop_summary_collection) = collect_task_summary(event_receiver);

    println!("Uploading data to network...");
    info!(
        "Uploading {} file: {file}",
        if public { "public" } else { "private" }
    );

    let dir_path = PathBuf::from(file);
    let name = dir_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or(file.to_string());

    let mut local_addr = String::new();
    let result = if public {
        client
            .dir_and_archive_upload_public(dir_path, &wallet)
            .await
            .map(|xor_name| {
                local_addr = addr_to_str(xor_name);
                local_addr.clone()
            })
            .inspect_err(|e| error!("Failed to upload file {file:?} : {e}"))
    } else {
        client
            .dir_and_archive_upload(dir_path, &wallet)
            .await
            .map(|private_data_access| {
                local_addr = private_data_access.address();
                private_data_access.to_hex()
            })
            .inspect_err(|e| error!("Failed to upload file {file:?}: {e}"))
    };

    // wait for upload to complete
    if let Err(e) = stop_summary_collection.send(()) {
        error!("Failed to send upload completed event: {e:?}");
    }

    let task_summary = task_summary_thread.await?;
    let upload_output = FileUploadOutput {
        exit_code: result
            .as_ref()
            .map_or_else(exit_code::upload_exit_code, |_| 0),
        file: file.to_string(),
        task_summary,
        start_time: start,
        end_time: SystemTime::now(),
        uploaded_address: result.as_ref().ok().cloned(),
        public,
    };

    if json {
        upload_output.print_json();
    } else {
        upload_output.print();
    }

    let archive = match result {
        Ok(archive) => archive,
        Err(err) => {
            let exit_code = exit_code::upload_exit_code(&err);
            process::exit(exit_code);
        }
    };

    // save to local user data
    let writer = if public {
        crate::user_data::write_local_public_file_archive(archive, &name)
    } else {
        crate::user_data::write_local_private_file_archive(archive, local_addr, &name)
    };
    writer
        .wrap_err("Failed to save file to local user data")
        .with_suggestion(|| "Local user data saves the file address above to disk, without it you need to keep track of the address yourself")?;
    info!("Saved file to local user data");

    Ok(())
}

pub async fn download(
    addr: &str,
    dest_path: &str,
    peers: NetworkPeers,
    read_quorum: Option<ResponseQuorum>,
    json: bool,
) -> Result<()> {
    let mut client_operation_config = ClientOperationConfig::default();
    if let Some(read_quorum) = read_quorum {
        client_operation_config.chunk_read_quorum(read_quorum);
    }
    let mut client = crate::actions::connect_to_network(peers, client_operation_config).await?;

    crate::actions::download(addr, dest_path, &mut client, json).await
}

pub fn list() -> Result<()> {
    // get public file archives
    println!("Retrieving local user data...");
    let file_archives = crate::user_data::get_local_public_file_archives()
        .wrap_err("Failed to get local public file archives")?;

    println!(
        "✅ You have {} public file archive(s):",
        file_archives.len()
    );
    for (addr, name) in file_archives {
        println!("{}: {}", name, addr_to_str(addr));
    }

    // get private file archives
    println!();
    let private_file_archives = crate::user_data::get_local_private_file_archives()
        .wrap_err("Failed to get local private file archives")?;

    println!(
        "✅ You have {} private file archive(s):",
        private_file_archives.len()
    );
    for (addr, name) in private_file_archives {
        println!("{}: {}", name, addr.address());
    }

    println!();
    println!("> Note that private data addresses are not network addresses, they are only used for referring to private data client side.");
    Ok(())
}
