// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::access::cached_payments;
use crate::actions::NetworkContext;
use crate::exit_code::{upload_exit_code, ExitCodeError, IO_ERROR};
use crate::utils::collect_upload_summary;
use crate::wallet::load_wallet;
use autonomi::client::payment::PaymentOption;
use autonomi::client::PutError;
use autonomi::files::UploadError;
use autonomi::networking::Quorum;
use autonomi::{ClientOperatingStrategy, TransactionConfig};
use color_eyre::eyre::{eyre, Context, Result};
use color_eyre::Section;
use std::path::PathBuf;

pub async fn cost(file: &str, network_context: NetworkContext) -> Result<()> {
    let client = crate::actions::connect_to_network(network_context)
        .await
        .map_err(|(err, _)| err)?;

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
    network_context: NetworkContext,
    max_fee_per_gas: Option<u128>,
) -> Result<(), ExitCodeError> {
    let config = ClientOperatingStrategy::new();

    let mut client =
        crate::actions::connect_to_network_with_config(network_context, config).await?;

    let mut wallet = load_wallet(client.evm_network()).map_err(|err| (err, IO_ERROR))?;

    if let Some(max_fee_per_gas) = max_fee_per_gas {
        wallet.set_transaction_config(TransactionConfig::new(max_fee_per_gas))
    }

    let payment = if let Ok(Some(receipt)) = cached_payments::load_payment_for_file(file) {
        println!("Using cached payment: no need to re-pay");
        PaymentOption::Receipt(receipt)
    } else {
        PaymentOption::Wallet(wallet)
    };

    let event_receiver = client.enable_client_events();
    let (upload_summary_thread, upload_completed_tx) = collect_upload_summary(event_receiver);

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

    // upload dir
    let local_addr;
    let archive = if public {
        let result = client.dir_upload_public(dir_path, payment.clone()).await;
        match result {
            Ok((_cost, xor_name)) => {
                local_addr = xor_name.to_hex();
                local_addr.clone()
            }
            Err(UploadError::PutError(PutError::Batch(upload_state))) => {
                let res = cached_payments::save_payment(file, &upload_state);
                println!("Cached payment to local disk for {file}: {res:?}");
                let exit_code =
                    upload_exit_code(&UploadError::PutError(PutError::Batch(Default::default())));
                return Err((
                    eyre!(UploadError::PutError(PutError::Batch(upload_state)))
                        .wrap_err("Failed to upload file".to_string()),
                    exit_code,
                ));
            }
            Err(err) => {
                let exit_code = upload_exit_code(&err);
                return Err((
                    eyre!(err).wrap_err("Failed to upload file".to_string()),
                    exit_code,
                ));
            }
        }
    } else {
        let result = client.dir_upload(dir_path, payment).await;
        match result {
            Ok((_cost, private_data_access)) => {
                local_addr = private_data_access.address();
                private_data_access.to_hex()
            }
            Err(UploadError::PutError(PutError::Batch(upload_state))) => {
                let res = crate::access::cached_payments::save_payment(file, &upload_state);
                println!("Cached payment to local disk for {file}: {res:?}");
                let exit_code =
                    upload_exit_code(&UploadError::PutError(PutError::Batch(Default::default())));
                return Err((
                    eyre!(UploadError::PutError(PutError::Batch(upload_state)))
                        .wrap_err("Failed to upload file".to_string()),
                    exit_code,
                ));
            }
            Err(err) => {
                let exit_code = upload_exit_code(&err);
                return Err((
                    eyre!(err).wrap_err("Failed to upload file".to_string()),
                    exit_code,
                ));
            }
        }
    };

    // wait for upload to complete
    if let Err(e) = upload_completed_tx.send(()) {
        error!("Failed to send upload completed event: {e:?}");
        eprintln!("Failed to send upload completed event: {e:?}");
    }

    // get summary
    let summary = upload_summary_thread
        .await
        .map_err(|err| (eyre!(err), IO_ERROR))?;
    if summary.records_paid == 0 {
        println!("All chunks already exist on the network.");
    } else {
        println!("Successfully uploaded: {file}");
        println!("At address: {local_addr}");
        info!("Successfully uploaded: {file} at address: {local_addr}");
        println!("Number of chunks uploaded: {}", summary.records_paid);
        println!(
            "Number of chunks already paid/uploaded: {}",
            summary.records_already_paid
        );
        println!("Total cost: {} AttoTokens", summary.tokens_spent);
    }
    info!("Summary for upload of file {file} at {local_addr:?}: {summary:?}");

    // save to local user data
    let writer = if public {
        crate::user_data::write_local_public_file_archive(archive, &name)
    } else {
        crate::user_data::write_local_private_file_archive(archive, local_addr, &name)
    };
    writer
        .wrap_err("Failed to save file to local user data")
        .with_suggestion(|| "Local user data saves the file address above to disk, without it you need to keep track of the address yourself")
        .map_err(|err| (err, IO_ERROR))?;

    info!("Saved file to local user data");

    Ok(())
}

pub async fn download(
    addr: &str,
    dest_path: &str,
    network_context: NetworkContext,
    quorum: Option<Quorum>,
) -> Result<(), ExitCodeError> {
    let mut config = ClientOperatingStrategy::new();
    if let Some(quorum) = quorum {
        config.chunks.get_quorum = quorum;
    }
    let client = crate::actions::connect_to_network_with_config(network_context, config).await?;
    crate::actions::download(addr, dest_path, &client).await
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
        println!("{}: {}", name, addr.to_hex());
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
