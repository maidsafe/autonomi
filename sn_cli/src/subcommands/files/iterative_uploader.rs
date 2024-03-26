use crate::subcommands::files::{self, ChunkManager, FilesUploadOptions};
use color_eyre::{eyre::eyre, Result};
use indicatif::ProgressBar;
use sn_client::{
    transfers::{NanoTokens, TransferError, WalletError},
    Client, Error as ClientError, UploadEvent, UploadSummary, Uploader,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Instant,
};
use tokio::{sync::mpsc::Receiver, task::JoinHandle};
use xor_name::XorName;

pub(crate) struct IterativeUploader {
    chunk_manager: ChunkManager,
    client: Client,
    wallet_dir: PathBuf,
}

impl IterativeUploader {
    pub(crate) fn new(chunk_manager: ChunkManager, client: Client, wallet_dir: PathBuf) -> Self {
        Self {
            chunk_manager,
            client,
            wallet_dir,
        }
    }
}

impl IterativeUploader {
    /// Given an iterator over files, upload them.
    /// Optionally verify if the data was stored successfully.
    pub(crate) async fn iterate_upload(
        self,
        chunks_to_upload: Vec<(XorName, PathBuf)>,
        files_path: &PathBuf,
        options: FilesUploadOptions,
    ) -> Result<()> {
        //
        let FilesUploadOptions {
            make_data_public,
            verify_store,
            batch_size,
            retry_strategy,
        } = options;
        let mut uploader = Uploader::new(self.client, self.wallet_dir)
            .set_batch_size(batch_size)
            .set_verify_store(verify_store)
            .set_retry_strategy(retry_strategy);
        let progress_bar = files::get_progress_bar(chunks_to_upload.len() as u64)?;
        let total_existing_chunks = Arc::new(AtomicU64::new(0));
        let map_join_handle_that_contains_resulting_file_upload_events =
            spawn_file_upload_events_handler(
                self.chunk_manager,
                make_data_public,
                progress_bar,
                uploader.get_event_receiver(),
                total_existing_chunks.clone(),
            );
        let current_instant = Instant::now();

        msg_begin_messages(
            files_path,
            &batch_size,
            &verify_store,
            make_data_public,
            &chunks_to_upload.len(),
        );

        let upload_summary =
            IterativeUploader::upload_chunk_vector(chunks_to_upload.clone(), uploader).await?;

        map_join_handle_that_contains_resulting_file_upload_events
            .await?
            .map_err(|err| eyre!("Failed to write uploaded files with err: {err:?}"))?;

        msg_end_messages(
            chunks_to_upload.len(),
            current_instant,
            total_existing_chunks,
            upload_summary,
        );

        Ok(())
    }

    async fn upload_chunk_vector(
        chunks_to_upload: Vec<(XorName, PathBuf)>,
        mut uploader: Uploader,
    ) -> Result<UploadSummary> {
        uploader.insert_chunk_paths(chunks_to_upload);
        match uploader.start_upload().await {
            Ok(summary) => Ok(summary),
            Err(ClientError::Wallet(WalletError::Transfer(TransferError::NotEnoughBalance(
                available,
                required,
            )))) => Err(eyre!(
                "Not enough balance in wallet to pay for chunk. \
            We have {available:?} but need {required:?} to pay for the chunk"
            )),
            Err(err) => Err(eyre!("Failed to upload chunk batch: {err}")),
        }
    }
}

///////////////// Associative Functions /////////////////

fn spawn_file_upload_events_handler(
    mut chunk_manager: ChunkManager,
    make_data_public: bool,
    progress_bar: ProgressBar,
    mut upload_event_rx: Receiver<UploadEvent>,
    total_existing_chunks: Arc<AtomicU64>,
) -> JoinHandle<Result<(), ClientError>> {
    tokio::spawn(async move {
        let mut upload_terminated_with_error = false;
        // The loop is guaranteed to end, as the channel will be
        // closed when the upload completes or errors out.
        while let Some(event) = upload_event_rx.recv().await {
            match event {
                UploadEvent::ChunkUploaded(addr) => {
                    progress_bar.clone().inc(1);

                    if let Err(err) = chunk_manager.mark_completed(std::iter::once(*addr.xorname()))
                    {
                        error!("Failed to mark chunk {addr:?} as completed: {err:?}");
                    }
                }
                UploadEvent::ChunkAlreadyExistsInNetwork(addr) => {
                    let _ = total_existing_chunks.fetch_add(1, Ordering::Relaxed);
                    progress_bar.clone().inc(1);
                    if let Err(err) = chunk_manager.mark_completed(std::iter::once(*addr.xorname()))
                    {
                        error!("Failed to mark chunk {addr:?} as completed: {err:?}");
                    }
                }
                UploadEvent::Error => {
                    upload_terminated_with_error = true;
                }
                UploadEvent::RegisterUploaded { .. }
                | UploadEvent::RegisterUpdated { .. }
                | UploadEvent::PaymentMade { .. } => {}
            }
        }
        progress_bar.finish_and_clear();

        // this check is to make sure that we don't partially write to the uploaded_files file if the upload process
        // terminates with an error. This race condition can happen as we bail on `upload_result` before we await the
        // handler.
        if upload_terminated_with_error {
            error!("Got FileUploadEvent::Error inside upload event loop");
        } else {
            msg_check_incomplete_files(&mut chunk_manager);

            // log uploaded file information
            msg_uploaded_files_banner();
            if !make_data_public {
                msg_not_public_by_default_banner();
            }
            msg_star_line();
            msg_chunk_manager_upload_complete(chunk_manager);
        }

        Ok::<_, ClientError>(())
    })
}

/////////////////  Messages  /////////////////

/// Function to format elapsed time into a string
fn msg_format_elapsed_time(elapsed_time: std::time::Duration) -> String {
    let elapsed_minutes = elapsed_time.as_secs() / 60;
    let elapsed_seconds = elapsed_time.as_secs() % 60;
    if elapsed_minutes > 0 {
        format!("{elapsed_minutes} minutes {elapsed_seconds} seconds")
    } else {
        format!("{elapsed_seconds} seconds")
    }
}

fn msg_check_incomplete_files(chunk_manager: &mut ChunkManager) {
    for file_name in chunk_manager.incomplete_files() {
        if let Some(file_name) = file_name.to_str() {
            println!("Unverified file \"{file_name}\", suggest to re-upload again.");
            info!("Unverified {file_name}");
        } else {
            println!("Unverified file \"{file_name:?}\", suggest to re-upload again.");
            info!("Unverified file {file_name:?}");
        }
    }
}

fn msg_begin_messages(
    files_path: &PathBuf,
    batch_size: &usize,
    verify_store: &bool,
    make_data_public: bool,
    chunks_to_upload_len: &usize,
) {
    debug!(
        "Uploading file(s) from {files_path:?}, \
    batch size {batch_size:?} will verify?: {verify_store}"
    );

    if make_data_public {
        info!("{files_path:?} will be made public and linkable");
        println!("{files_path:?} will be made public and linkable");
    }

    println!("Splitting and uploading {files_path:?} into {chunks_to_upload_len} chunks",);
}

fn msg_end_messages(
    chunks_to_upload_amount: usize,
    time_since_mark: Instant,
    total_existing_chunks: Arc<AtomicU64>,
    upload_summary: UploadSummary,
) {
    let total_existing_chunks = total_existing_chunks.load(Ordering::Relaxed);
    let uploaded_chunks = chunks_to_upload_amount - total_existing_chunks as usize;
    let time_since_mark_formatted = msg_format_elapsed_time(time_since_mark.elapsed());

    msg_chunks_found_existed(
        chunks_to_upload_amount,
        &time_since_mark_formatted,
        total_existing_chunks,
        uploaded_chunks,
    );
    msg_chunks_found_existed_info(
        chunks_to_upload_amount,
        &time_since_mark_formatted,
        total_existing_chunks,
        uploaded_chunks,
    );
    let storage_cost = upload_summary.storage_cost;
    msg_payment_details(
        storage_cost,
        upload_summary.royalty_fees,
        upload_summary.final_balance,
        uploaded_chunks,
    );

    msg_made_payment_info(storage_cost, uploaded_chunks);
}

pub fn msg_chunk_manager_upload_complete(chunk_manager: ChunkManager) {
    for (file_name, addr) in chunk_manager.completed_files() {
        let hex_addr = addr.to_hex();
        if let Some(file_name) = file_name.to_str() {
            println!("\"{file_name}\" {hex_addr}");
            info!("Uploaded {file_name} to {hex_addr}");
        } else {
            println!("\"{file_name:?}\" {hex_addr}");
            info!("Uploaded {file_name:?} to {hex_addr}");
        }
    }
}
fn msg_made_payment_info(total_storage_cost: NanoTokens, uploaded_chunks: usize) {
    info!("Made payment of {total_storage_cost} for {uploaded_chunks} chunks");
}

fn msg_chunks_found_existed_info(
    chunks_to_upload_len: usize,
    elapsed: &String,
    total_existing_chunks: u64,
    uploaded_chunks: usize,
) {
    info!(
        "Among {chunks_to_upload_len} chunks, found \
    {total_existing_chunks} already existed in network, \
    uploaded the leftover {uploaded_chunks} chunks in {elapsed}"
    );
}

fn msg_chunks_found_existed(
    chunks_to_upload_len: usize,
    elapsed: &String,
    total_existing_chunks: u64,
    uploaded_chunks: usize,
) {
    println!(
        "Among {chunks_to_upload_len} chunks, found \
    {total_existing_chunks} already existed in network, \
    uploaded the leftover {uploaded_chunks} chunks in {elapsed}"
    );
}

fn msg_payment_details(
    total_storage_cost: NanoTokens,
    total_royalty_fees: NanoTokens,
    final_balance: NanoTokens,
    uploaded_chunks: usize,
) {
    println!("**************************************");
    println!("*          Payment Details           *");
    println!("**************************************");
    println!("Made payment of {total_storage_cost} for {uploaded_chunks} chunks");
    println!("Made payment of {total_royalty_fees} for royalties fees");
    println!("New wallet balance: {final_balance}");
}
fn msg_star_line() {
    println!("**************************************");
}

fn msg_not_public_by_default_banner() {
    println!("*                                    *");
    println!("*  These are not public by default.  *");
    println!("*     Reupload with `-p` option      *");
    println!("*      to publish the datamaps.      *");
}

fn msg_uploaded_files_banner() {
    println!("**************************************");
    println!("*          Uploaded Files            *");
}
