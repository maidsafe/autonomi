// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::access::{cached_merkle_payments, cached_payments};
use crate::actions::NetworkContext;
use crate::args::max_fee_per_gas::{MaxFeePerGasParam, get_max_fee_per_gas_from_opt_param};
use crate::commands::PaymentFlags;
use crate::exit_code::{ExitCodeError, FEES_ERROR, IO_ERROR, upload_exit_code};
use crate::utils::collect_upload_summary;
use crate::wallet::load_wallet;
use autonomi::client::PutError;
use autonomi::client::analyze::Analysis;
use autonomi::client::config::MERKLE_PAYMENT_THRESHOLD;
use autonomi::client::merkle_payments::MerklePaymentReceipt;
use autonomi::client::payment::{BulkPaymentOption, PaymentOption, Receipt};
use autonomi::files::{UploadError, estimate_directory_chunks};
use autonomi::networking::{Quorum, RetryStrategy};
use autonomi::{AttoTokens, Client, ClientOperatingStrategy, PaymentMode, TransactionConfig};
use color_eyre::Section;
use color_eyre::eyre::{Context, Result, eyre};
use std::path::PathBuf;

const MAX_ADDRESSES_TO_PRINT: usize = 3;

/// How the payment method was selected
#[derive(Debug, Clone)]
pub enum PaymentSelection {
    Forced,
    Auto,
}

/// Represents the payment method to be used
#[derive(Debug, Clone)]
pub enum PaymentMethod {
    /// Merkle tree payments (batched via smart contract)
    Merkle {
        is_resuming: bool,
        selection: PaymentSelection,
    },
    /// Regular per-batch payments
    Regular {
        use_standard_payment: bool,
        is_resuming: bool,
        selection: PaymentSelection,
    },
}

impl PaymentMethod {
    /// Emoji representing this payment method
    pub fn emoji(&self) -> &'static str {
        match self {
            PaymentMethod::Merkle { .. } => "🌳",
            PaymentMethod::Regular {
                use_standard_payment,
                ..
            } => {
                if *use_standard_payment {
                    "💳"
                } else {
                    "🎯"
                }
            }
        }
    }

    /// Mode label for regular payments
    fn mode_label(&self) -> &'static str {
        match self {
            PaymentMethod::Regular {
                use_standard_payment,
                ..
            } => {
                if *use_standard_payment {
                    "standard"
                } else {
                    "single-node"
                }
            }
            PaymentMethod::Merkle { .. } => "",
        }
    }

    /// Generate the display message for this payment method
    pub fn display_message(&self, estimated_chunks: usize) -> String {
        let emoji = self.emoji();
        match self {
            PaymentMethod::Merkle { is_resuming, .. } => {
                let action = if *is_resuming { "Resuming" } else { "Using" };
                format!("{emoji} {action} merkle tree payments (~{estimated_chunks} chunks)")
            }
            PaymentMethod::Regular { is_resuming, .. } => {
                let action = if *is_resuming { "Resuming" } else { "Using" };
                let mode = self.mode_label();
                format!("{emoji} {action} regular payments (~{estimated_chunks} chunks, {mode})")
            }
        }
    }

    /// Describe the method and how it was selected
    pub fn method_label(&self, estimated_chunks: usize) -> String {
        match self {
            PaymentMethod::Merkle { selection, .. } => match selection {
                PaymentSelection::Forced => "merkle (forced)".to_string(),
                PaymentSelection::Auto => format!(
                    "merkle (auto-selected: ~{estimated_chunks} chunks >= {MERKLE_PAYMENT_THRESHOLD} threshold)"
                ),
            },
            PaymentMethod::Regular { selection, .. } => {
                let mode = self.mode_label();
                match selection {
                    PaymentSelection::Forced => format!("regular (forced, {mode})"),
                    PaymentSelection::Auto => format!(
                        "regular (auto-selected: ~{estimated_chunks} chunks < {MERKLE_PAYMENT_THRESHOLD} threshold, {mode})"
                    ),
                }
            }
        }
    }

    /// Build a `BulkPaymentOption` from this payment method
    pub fn into_bulk_payment_option(
        self,
        wallet: autonomi::Wallet,
        cached_merkle: Option<MerklePaymentReceipt>,
        cached_regular: Option<Receipt>,
    ) -> BulkPaymentOption {
        match self {
            PaymentMethod::Merkle { is_resuming, .. } => {
                if is_resuming && let Some(merkle_receipt) = cached_merkle {
                    return BulkPaymentOption::ContinueMerkle(wallet, merkle_receipt);
                }
                BulkPaymentOption::ForceMerkle(wallet)
            }
            PaymentMethod::Regular { is_resuming, .. } => {
                if is_resuming && let Some(receipt) = cached_regular {
                    return BulkPaymentOption::Receipt(receipt);
                }
                BulkPaymentOption::ForceRegular(wallet)
            }
        }
    }
}

/// Determine the payment method based on flags, cached receipts, and chunk count.
///
/// Selection priority (first match wins):
/// 1. `--merkle` flag: force merkle, resume if cached merkle receipts exist.
/// 2. `--regular` flag: force regular, resume if cached regular receipts exist.
/// 3. Cached regular receipts: resume previous regular upload.
/// 4. Cached merkle receipts: resume previous merkle upload.
/// 5. Auto-select merkle if `estimated_chunks >= MERKLE_PAYMENT_THRESHOLD`.
/// 6. Auto-select regular otherwise.
///
/// The `--disable-single-node-payment` flag controls the regular payment mode
/// ("standard" vs "single-node") but does not affect merkle payments.
fn determine_payment_method(
    estimated_chunks: usize,
    force_merkle: bool,
    force_regular: bool,
    use_standard_payment: bool,
    has_cached_regular: bool,
    has_cached_merkle: bool,
) -> PaymentMethod {
    if force_merkle {
        PaymentMethod::Merkle {
            is_resuming: has_cached_merkle,
            selection: PaymentSelection::Forced,
        }
    } else if force_regular {
        PaymentMethod::Regular {
            use_standard_payment,
            is_resuming: has_cached_regular,
            selection: PaymentSelection::Forced,
        }
    } else if has_cached_regular {
        // Resume cached regular payment
        PaymentMethod::Regular {
            use_standard_payment,
            is_resuming: true,
            selection: PaymentSelection::Auto,
        }
    } else if has_cached_merkle {
        // Resume cached merkle payment
        PaymentMethod::Merkle {
            is_resuming: true,
            selection: PaymentSelection::Auto,
        }
    } else if estimated_chunks >= MERKLE_PAYMENT_THRESHOLD {
        // Auto-select merkle
        PaymentMethod::Merkle {
            is_resuming: false,
            selection: PaymentSelection::Auto,
        }
    } else {
        // Auto-select regular
        PaymentMethod::Regular {
            use_standard_payment,
            is_resuming: false,
            selection: PaymentSelection::Auto,
        }
    }
}

/// Add archive cost to base cost if needed
async fn add_archive_cost_if_needed(
    client: &Client,
    base_cost: AttoTokens,
    path: &PathBuf,
    include_archive: bool,
    is_public: bool,
) -> Result<AttoTokens> {
    if include_archive {
        let archive_cost = client
            .estimate_archive_cost(path, is_public)
            .await
            .wrap_err("Failed to calculate archive cost")?;
        base_cost
            .checked_add(archive_cost)
            .ok_or_else(|| eyre!("Cost overflow when adding archive cost"))
    } else {
        Ok(base_cost)
    }
}

pub async fn cost(
    file: &str,
    is_public: bool,
    include_archive: bool,
    network_context: NetworkContext,
    payment_flags: PaymentFlags,
) -> Result<()> {
    let PaymentFlags {
        disable_single_node_payment: use_standard_payment,
        merkle: force_merkle,
        regular: force_regular,
    } = payment_flags;

    let mut client = crate::actions::connect_to_network(network_context)
        .await
        .map_err(|(err, _)| err)?;

    let path = PathBuf::from(file);
    let visibility = if is_public { "public" } else { "private" };
    let archive_info = if include_archive {
        "with archive"
    } else {
        "without archive"
    };

    // Estimate chunks for consistent method determination
    let estimated_chunks =
        estimate_directory_chunks(&path).wrap_err("Failed to estimate chunk count")?;

    // Determine payment method (no cached receipts for cost estimation)
    let method = determine_payment_method(
        estimated_chunks,
        force_merkle,
        force_regular,
        use_standard_payment,
        false, // has_cached_regular
        false, // has_cached_merkle
    );

    // Configure payment mode - set Standard if user requested it
    // This is a no-op for merkle payments, but ensures regular payments honor the user's preference
    if use_standard_payment {
        client = client.with_payment_mode(PaymentMode::Standard);
    }

    // Print initial status
    println!("{}", method.display_message(estimated_chunks));
    println!("Getting upload cost ({visibility}, {archive_info})...");
    info!(
        "Calculating cost for file: {file} (public={is_public}, include_archive={include_archive})"
    );

    // Calculate cost based on method
    let total_cost = if force_merkle {
        // Forced merkle mode
        let content_cost = client
            .file_cost_merkle(path.clone(), is_public)
            .await
            .wrap_err("Failed to calculate merkle cost for file")?;
        add_archive_cost_if_needed(&client, content_cost, &path, include_archive, is_public).await?
    } else if force_regular {
        // Forced regular mode
        let content_cost = client
            .file_cost_regular(&path, is_public)
            .await
            .wrap_err("Failed to calculate regular cost for file")?;
        add_archive_cost_if_needed(&client, content_cost, &path, include_archive, is_public).await?
    } else {
        // Auto mode - let client decide based on its internal threshold logic
        client
            .file_cost(&path, is_public, include_archive)
            .await
            .wrap_err("Failed to calculate cost for file")?
    };

    // Print results
    let method_label = method.method_label(estimated_chunks);
    println!("Estimate cost to upload file: {file}");
    println!("Total cost: {total_cost}");
    println!("Method: {method_label}");
    info!("Total cost: {total_cost} for file: {file}");

    Ok(())
}

pub async fn upload(
    file: &str,
    public: bool,
    no_archive: bool,
    network_context: NetworkContext,
    max_fee_per_gas_param: Option<MaxFeePerGasParam>,
    retry_failed: u64,
    payment_flags: PaymentFlags,
) -> Result<(), ExitCodeError> {
    let PaymentFlags {
        disable_single_node_payment: use_standard_payment,
        merkle: force_merkle,
        regular: force_regular,
    } = payment_flags;

    let config = ClientOperatingStrategy::new();

    let mut client =
        crate::actions::connect_to_network_with_config(network_context, config).await?;

    // Configure client with retry_failed setting
    if retry_failed != 0 {
        client = client.with_retry_failed(retry_failed);
        println!(
            "🔄 Retry mode enabled - will retry failed chunks until successful or exceeds the limit."
        );
    }

    // Configure payment mode - default is SingleNode, only override if Standard is requested
    if use_standard_payment && !force_merkle {
        client = client.with_payment_mode(PaymentMode::Standard);
    }

    let mut wallet = load_wallet(client.evm_network()).map_err(|err| (err, IO_ERROR))?;

    let max_fee_per_gas =
        get_max_fee_per_gas_from_opt_param(max_fee_per_gas_param, client.evm_network())
            .map_err(|err| (err, FEES_ERROR))?;
    wallet.set_transaction_config(TransactionConfig { max_fee_per_gas });

    let event_receiver = client.enable_client_events();
    let (upload_summary_thread, upload_completed_tx) = collect_upload_summary(event_receiver);

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
    let not_single_file = !dir_path.is_file();
    let (archive_addr, local_addr) = match upload_dir_standard(
        &client,
        dir_path.clone(),
        public,
        no_archive,
        file,
        wallet,
        force_merkle,
        force_regular,
        use_standard_payment,
    )
    .await
    {
        Ok((a, l)) => (a, l),
        Err(UploadError::PutError(PutError::Batch(upload_state))) => {
            let res = cached_payments::save_payment(file, &upload_state);
            println!("Cached regular payment to local disk for {file}: {res:?}");
            let exit_code =
                upload_exit_code(&UploadError::PutError(PutError::Batch(Default::default())));
            return Err((
                eyre!(UploadError::PutError(PutError::Batch(upload_state)))
                    .wrap_err("Failed to upload file".to_string()),
                exit_code,
            ));
        }
        Err(UploadError::MerkleUpload(merkle_err)) => {
            if let Some(receipt) = &merkle_err.receipt {
                let res = cached_merkle_payments::save_merkle_payment(file, receipt);
                println!("Cached merkle payment to local disk for {file}: {res:?}");
            }
            let error_msg = format!("{merkle_err}");
            let exit_code = upload_exit_code(&UploadError::MerkleUpload(merkle_err));
            return Err((
                eyre!(error_msg).wrap_err("Failed to upload file with merkle payment".to_string()),
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

    // save archive to local user data
    if !no_archive && not_single_file {
        let writer = if public {
            crate::user_data::write_local_public_file_archive(archive_addr.clone(), &name)
        } else {
            crate::user_data::write_local_private_file_archive(
                archive_addr.clone(),
                local_addr.clone(),
                &name,
            )
        };
        writer
            .wrap_err("Failed to save file to local user data")
            .with_suggestion(|| "Local user data saves the file address above to disk, without it you need to keep track of the address yourself")
            .map_err(|err| (err, IO_ERROR))?;
        info!("Saved file to local user data");
    }

    // save single private files to local user data
    if !not_single_file && !public {
        let writer = crate::user_data::write_local_private_file(
            archive_addr.clone(),
            local_addr.clone(),
            &name,
        );
        writer
            .wrap_err("Failed to save private file to local user data")
            .with_suggestion(|| "Local user data saves the file address above to disk, without it you need to keep track of the address yourself")
            .map_err(|err| (err, IO_ERROR))?;
        info!("Saved private file to local user data");
    }

    // save single public files to local user data
    if !not_single_file && public {
        let writer = crate::user_data::write_local_public_file(local_addr.to_owned(), &name);
        writer
            .wrap_err("Failed to save public file to local user data")
            .with_suggestion(|| "Local user data saves the file address above to disk, without it you need to keep track of the address yourself")
            .map_err(|err| (err, IO_ERROR))?;
        info!("Saved public file to local user data");
    }

    Ok(())
}

/// Uploads a file or directory to the network using standard payment.
/// Single files are uploaded without an archive, directories are uploaded with an archive.
/// The no_archive argument can be used to skip the archive upload.
/// Returns the archive address if any and the address to access the data.
/// If more than [`MAX_ADDRESSES_TO_PRINT`] addresses are found, returns "multiple addresses" as a placeholder instead.
#[allow(clippy::too_many_arguments)]
async fn upload_dir_standard(
    client: &Client,
    dir_path: PathBuf,
    public: bool,
    no_archive: bool,
    file: &str,
    wallet: autonomi::Wallet,
    force_merkle: bool,
    force_regular: bool,
    use_standard_payment: bool,
) -> Result<(String, String), UploadError> {
    let is_single_file = dir_path.is_file();

    // Load cached receipts
    let cached_regular = cached_payments::load_payment_for_file(file).ok().flatten();
    let cached_merkle = cached_merkle_payments::load_merkle_payment_for_file(file)
        .ok()
        .flatten();

    // Estimate chunks upfront for consistent messaging
    let estimated_chunks = estimate_directory_chunks(&dir_path)?;

    // Determine payment method using shared logic
    let method = determine_payment_method(
        estimated_chunks,
        force_merkle,
        force_regular,
        use_standard_payment,
        cached_regular.is_some(),
        cached_merkle.is_some(),
    );

    // Print any ignored cache warnings
    if force_merkle && cached_regular.is_some() {
        println!("Ignoring cached regular payment (--merkle specified)");
    }
    if force_regular && cached_merkle.is_some() {
        println!("Ignoring cached merkle payment (--regular specified)");
    }

    // Print the payment method message
    println!("{}", method.display_message(estimated_chunks));

    // Build payment option from method and cached receipts
    let payment_option =
        method.into_bulk_payment_option(wallet.clone(), cached_merkle, cached_regular);

    if public {
        let (_, public_archive) = client
            .dir_content_upload_public(dir_path, payment_option.clone())
            .await?;

        let mut addrs = vec![];
        for (file_path, addr, _meta) in public_archive.iter() {
            println!("  - {file_path:?}: {:?}", addr.to_hex());
            addrs.push(addr.to_hex());
        }

        if no_archive || is_single_file {
            if addrs.len() > MAX_ADDRESSES_TO_PRINT {
                Ok(("no-archive".to_string(), "multiple addresses".to_string()))
            } else {
                Ok(("no-archive".to_string(), addrs.join(", ")))
            }
        } else {
            let (_, addr) = client
                .archive_put_public(&public_archive, PaymentOption::Wallet(wallet.clone()))
                .await?;
            Ok((addr.to_hex(), addr.to_hex()))
        }
    } else {
        let (_, private_archive) = client
            .dir_content_upload(dir_path, payment_option.clone())
            .await?;

        let mut addrs = vec![];
        for (file_path, private_datamap, _meta) in private_archive.iter() {
            println!("  - {file_path:?}: {:?}", private_datamap.to_hex());
            addrs.push(private_datamap.to_hex());
        }

        if no_archive || is_single_file {
            if addrs.len() > MAX_ADDRESSES_TO_PRINT {
                Ok(("no-archive".to_string(), "multiple addresses".to_string()))
            } else if is_single_file && addrs.len() == 1 {
                // For single private files, return both full hex and short address
                if let Some((_, private_datamap, _)) = private_archive.iter().next() {
                    Ok((private_datamap.to_hex(), private_datamap.address()))
                } else {
                    Ok(("no-archive".to_string(), addrs.join(", ")))
                }
            } else {
                Ok(("no-archive".to_string(), addrs.join(", ")))
            }
        } else {
            let (_, private_datamap) = client
                .archive_put(&private_archive, PaymentOption::Wallet(wallet))
                .await?;
            Ok((private_datamap.to_hex(), private_datamap.address()))
        }
    }
}

pub async fn download(
    addr: &str,
    dest_path: &str,
    network_context: NetworkContext,
    quorum: Option<Quorum>,
    retries: Option<usize>,
    cache_chunks: bool,
    cache_dir: Option<&PathBuf>,
) -> Result<(), ExitCodeError> {
    let mut config = ClientOperatingStrategy::new();

    if let Some(quorum) = quorum {
        config.chunks.get_quorum = quorum;
    }

    if let Some(retries) = retries {
        config.chunks.get_retry = RetryStrategy::N(retries);
    }

    // Enable chunk caching in config (enabled by default unless disabled)
    if cache_chunks {
        config.chunk_cache_enabled = true;
        config.chunk_cache_dir = cache_dir.cloned();
        // Only print message if custom cache dir is specified
        if let Some(dir) = cache_dir {
            println!("Using custom cache directory: {}", dir.display());
        }
    } else {
        config.chunk_cache_enabled = false;
        println!("Chunk caching disabled");
    }

    let client = crate::actions::connect_to_network_with_config(network_context, config).await?;

    crate::actions::download(addr, dest_path, &client).await
}

pub async fn list(network_context: NetworkContext, verbose: bool) -> Result<(), ExitCodeError> {
    let mut config = ClientOperatingStrategy::new();
    config.chunks.get_quorum = Quorum::One;
    config.chunks.get_retry = RetryStrategy::None;

    let maybe_client = if verbose {
        match crate::actions::connect_to_network_with_config(network_context, config).await {
            Ok(client) => Some(client),
            Err((mut err, code)) => {
                err = err.with_suggestion(|| "Try running without --verbose, -v");
                return Err((err, code));
            }
        }
    } else {
        None
    };

    // get public file archives
    println!("Retrieving local user data...");
    let file_archives = crate::user_data::get_local_public_file_archives()
        .wrap_err("Failed to get local public file archives")
        .map_err(|err| (err, IO_ERROR))?;

    println!(
        "✅ You have {} public file archive(s):",
        file_archives.len()
    );
    for (addr, name) in file_archives {
        println!("{}: {}", name, addr.to_hex());
        if let (true, Some(client)) = (verbose, maybe_client.as_ref()) {
            if let Ok(Analysis::PublicArchive { archive, .. }) =
                client.analyze_address(&addr.to_string(), false).await
            {
                for (file_path, data_addr, _meta) in archive.iter() {
                    println!("  - {file_path:?}: {data_addr:?}");
                }
            } else {
                println!("  - Not found on network");
            }
        }
    }

    // get public files
    println!();
    let public_files = crate::user_data::get_local_public_files()
        .wrap_err("Failed to get local public files")
        .map_err(|err| (err, IO_ERROR))?;

    println!("✅ You have {} public file(s):", public_files.len());
    for (addr, name) in public_files {
        println!("{}: {}", name, addr.to_hex());
        if let (true, Some(client)) = (verbose, maybe_client.as_ref()) {
            if let Ok(file_bytes) = client.data_get_public(&addr).await {
                println!("  - File size: {} bytes", file_bytes.len());
            } else {
                println!("  - Not found on network");
            }
        }
    }

    // get private file archives
    println!();
    let private_file_archives = crate::user_data::get_local_private_file_archives()
        .wrap_err("Failed to get local private file archives")
        .map_err(|err| (err, IO_ERROR))?;

    println!(
        "✅ You have {} private file archive(s):",
        private_file_archives.len()
    );
    for (addr, name) in private_file_archives {
        println!("{}: {}", name, addr.address());
        if let (true, Some(client)) = (verbose, maybe_client.as_ref()) {
            if let Ok(Analysis::PrivateArchive(private_archive)) =
                client.analyze_address(&addr.to_string(), false).await
            {
                for (file_path, _data_addr, _meta) in private_archive.iter() {
                    println!("  - {file_path:?}");
                }
            } else {
                println!("  - Not found on network");
            }
        }
    }

    // get private files
    println!();
    let private_files = crate::user_data::get_local_private_files()
        .wrap_err("Failed to get local private files")
        .map_err(|err| (err, IO_ERROR))?;

    println!("✅ You have {} private file(s):", private_files.len());
    for (addr, name) in private_files {
        println!("{}: {}", name, addr.address());
        if let (true, Some(client)) = (verbose, maybe_client.as_ref()) {
            if let Ok(file_bytes) = client.data_get(&addr).await {
                println!("  - File size: {} bytes", file_bytes.len());
            } else {
                println!("  - Not found on network");
            }
        }
    }

    println!();
    println!(
        "> Note that private data addresses are not network addresses, they are only used for referring to private data client side."
    );
    Ok(())
}
