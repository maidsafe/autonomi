// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod chunk_manager;
mod estimate;

pub(crate) mod download;
pub(crate) mod iterative_uploader;
pub(crate) mod upload;

pub(crate) use chunk_manager::ChunkManager;

use crate::subcommands::files::iterative_uploader::IterativeUploader;
use clap::Parser;
use color_eyre::{
    eyre::{bail, eyre},
    Help, Result,
};
use indicatif::{ProgressBar, ProgressStyle};
use rand::prelude::SliceRandom;
use rand::thread_rng;
use sn_client::protocol::storage::{Chunk, ChunkAddress, RetryStrategy};
use sn_client::{Client, FilesApi, BATCH_SIZE};
use std::time::Duration;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};
use upload::{FilesUploadOptions, UploadedFile, UPLOADED_FILES};
use walkdir::{DirEntry, WalkDir};
use xor_name::XorName;

#[derive(Parser, Debug)]
pub enum FilesCmds {
    Estimate {
        /// The location of the file(s) to upload. Can be a file or a directory.
        #[clap(name = "path", value_name = "PATH")]
        path: PathBuf,
        /// Should the file be made accessible to all. (This is irreversible)
        #[clap(long, name = "make_public", default_value = "false", short = 'p')]
        make_data_public: bool,
    },
    Upload {
        /// The location of the file(s) to upload.
        ///
        /// Can be a file or a directory.
        #[clap(name = "path", value_name = "PATH")]
        file_path: PathBuf,
        /// The batch_size to split chunks into parallel handling batches
        /// during payment and upload processing.
        #[clap(long, default_value_t = BATCH_SIZE, short='b')]
        batch_size: usize,
        /// Should the file be made accessible to all. (This is irreversible)
        #[clap(long, name = "make_public", default_value = "false", short = 'p')]
        make_data_public: bool,
        /// Set the strategy to use on chunk upload failure. Does not modify the spend failure retry attempts yet.
        ///
        /// Choose a retry strategy based on effort level, from 'quick' (least effort), through 'balanced',
        /// to 'persistent' (most effort).
        #[clap(long, default_value_t = RetryStrategy::Balanced, short = 'r', help = "Sets the retry strategy on upload failure. Options: 'quick' for minimal effort, 'balanced' for moderate effort, or 'persistent' for maximum effort.")]
        retry_strategy: RetryStrategy,
    },
    Download {
        /// The name to apply to the downloaded file.
        ///
        /// If the name argument is used, the address argument must also be supplied.
        ///
        /// If neither are, all the files uploaded by the current user will be downloaded again.
        #[clap(name = "name")]
        file_name: Option<OsString>,
        /// The hex address of a file.
        ///
        /// If the address argument is used, the name argument must also be supplied.
        ///
        /// If neither are, all the files uploaded by the current user will be downloaded again.
        #[clap(name = "address")]
        file_addr: Option<String>,
        /// Flagging whether to show the holders of the uploaded chunks.
        /// Default to be not showing.
        #[clap(long, name = "show_holders", default_value = "false")]
        show_holders: bool,
        /// The batch_size for parallel downloading
        #[clap(long, default_value_t = BATCH_SIZE , short='b')]
        batch_size: usize,
        /// Set the strategy to use on downloads failure.
        ///
        /// Choose a retry strategy based on effort level, from 'quick' (least effort), through 'balanced',
        /// to 'persistent' (most effort).
        #[clap(long, default_value_t = RetryStrategy::Quick, short = 'r', help = "Sets the retry strategy on download failure. Options: 'quick' for minimal effort, 'balanced' for moderate effort, or 'persistent' for maximum effort.")]
        retry_strategy: RetryStrategy,
    },
}

pub(crate) async fn files_cmds(
    cmds: FilesCmds,
    client: &Client,
    root_dir: &Path,
    verify_store: bool,
) -> Result<()> {
    let files_api = FilesApi::build(client.clone(), root_dir.to_path_buf())?;
    let mut chunk_manager = ChunkManager::new(root_dir);

    match cmds {
        FilesCmds::Estimate {
            path,
            make_data_public,
        } => {
            estimate::Estimator::new(chunk_manager, files_api)
                .estimate_cost(path, make_data_public, root_dir)
                .await?
        }
        FilesCmds::Upload {
            file_path,
            batch_size,
            retry_strategy,
            make_data_public,
        } => {
            let total_files = chunk_manager.chunk_with_iter(
                WalkDir::new(&file_path).into_iter().flatten(),
                true,
                make_data_public,
            )?;

            if total_files == 0 {
                if file_path.is_dir() {
                    bail!(
                        "The directory specified for upload is empty. \
                    Please verify the provided path."
                    );
                } else {
                    bail!("The provided file path is invalid. Please verify the path.");
                }
            } else {
                let chunks_to_upload = chunks_to_upload(
                    &files_api,
                    &mut chunk_manager,
                    &file_path,
                    batch_size,
                    make_data_public,
                )
                .await?;

                IterativeUploader::new(chunk_manager, files_api)
                    .iterate_upload(
                        chunks_to_upload,
                        &file_path,
                        FilesUploadOptions {
                            make_data_public,
                            verify_store,
                            batch_size,
                            retry_strategy,
                        },
                    )
                    .await?;
            }
        }
        FilesCmds::Download {
            file_name,
            file_addr,
            show_holders,
            batch_size,
            retry_strategy,
        } => {
            if (file_name.is_some() && file_addr.is_none())
                || (file_addr.is_some() && file_name.is_none())
            {
                return Err(
                    eyre!("Both the name and address must be supplied if either are used")
                        .suggestion(
                        "Please run the command again in the form 'files upload <name> <address>'",
                    ),
                );
            }

            let mut download_dir = root_dir.to_path_buf();
            let mut download_file_name = file_name.clone();
            if let Some(file_name) = file_name {
                // file_name may direct the downloaded data to:
                //
                // the current directory (just a filename)
                // eg safe files download myfile.txt ADDRESS
                //
                // a directory relative to the current directory (relative filename)
                // eg safe files download my/relative/path/myfile.txt ADDRESS
                //
                // a directory relative to root of the filesystem (absolute filename)
                // eg safe files download /home/me/mydir/myfile.txt ADDRESS
                let file_name_path = Path::new(&file_name);
                if file_name_path.is_dir() {
                    return Err(eyre!("Cannot download file to path: {:?}", file_name));
                }
                let file_name_dir = file_name_path.parent();
                if file_name_dir.is_none() {
                    // just a filename, use the current_dir
                    download_dir = std::env::current_dir().unwrap_or(root_dir.to_path_buf());
                } else if file_name_path.is_relative() {
                    // relative to the current directory. Make the relative path
                    // into an absolute path by joining it to current_dir
                    if let Some(relative_dir) = file_name_dir {
                        let current_dir = std::env::current_dir().unwrap_or(root_dir.to_path_buf());
                        download_dir = current_dir.join(relative_dir);
                        if !download_dir.exists() {
                            return Err(eyre!("Directory does not exist: {:?}", download_dir));
                        }
                        if let Some(path_file_name) = file_name_path.file_name() {
                            download_file_name = Some(OsString::from(path_file_name));
                        }
                    }
                } else {
                    // absolute dir
                    download_dir = file_name_dir.unwrap_or(root_dir).to_path_buf();
                }
            }
            let files_api: FilesApi = FilesApi::new(client.clone(), download_dir.clone());

            match (download_file_name, file_addr) {
                (Some(download_file_name), Some(address_provided)) => {
                    let bytes =
                        hex::decode(&address_provided).expect("Input address is not a hex string");
                    let xor_name_provided = XorName(
                        bytes
                            .try_into()
                            .expect("Failed to parse XorName from hex string"),
                    );
                    // try to read the data_map if it exists locally.
                    let uploaded_files_path = root_dir.join(UPLOADED_FILES);
                    let expected_data_map_location = uploaded_files_path.join(address_provided);
                    let local_data_map = {
                        if expected_data_map_location.exists() {
                            let uploaded_file_metadata =
                                UploadedFile::read(&expected_data_map_location)?;

                            uploaded_file_metadata.data_map.map(|bytes| Chunk {
                                address: ChunkAddress::new(xor_name_provided),
                                value: bytes,
                            })
                        } else {
                            None
                        }
                    };

                    download::download_file(
                        files_api,
                        xor_name_provided,
                        (download_file_name, local_data_map),
                        &download_dir,
                        show_holders,
                        batch_size,
                        retry_strategy,
                    )
                    .await
                }
                _ => {
                    println!("Attempting to download all files uploaded by the current user...");
                    download::download_files(
                        &files_api,
                        root_dir,
                        show_holders,
                        batch_size,
                        retry_strategy,
                    )
                    .await?
                }
            }
        }
    }
    Ok(())
}

pub async fn chunks_to_upload(
    files_api: &FilesApi,
    chunk_manager: &mut ChunkManager,
    files_path: &Path,
    batch_size: usize,
    make_data_public: bool,
) -> Result<Vec<(XorName, PathBuf)>> {
    chunks_to_upload_with_iter(
        files_api,
        chunk_manager,
        WalkDir::new(files_path).into_iter().flatten(),
        false,
        batch_size,
        make_data_public,
    )
    .await
}

pub async fn chunks_to_upload_with_iter(
    files_api: &FilesApi,
    chunk_manager: &mut ChunkManager,
    entries_iter: impl Iterator<Item = DirEntry>,
    read_cache: bool,
    batch_size: usize,
    make_data_public: bool,
) -> Result<Vec<(XorName, PathBuf)>> {
    let chunks_to_upload = if chunk_manager.is_chunks_empty() {
        chunk_manager.chunk_with_iter(entries_iter, read_cache, make_data_public)?;

        let chunks = chunk_manager.get_chunks();

        let failed_chunks = files_api
            .client()
            .verify_uploaded_chunks(&chunks, batch_size)
            .await?;

        chunk_manager.mark_completed(
            chunks
                .into_iter()
                .filter(|c| !failed_chunks.contains(c))
                .map(|(xor, _)| xor),
        )?;

        if failed_chunks.is_empty() {
            msg_files_already_uploaded_verified();
            if !make_data_public {
                msg_not_public_by_default();
            }
            msg_star_line();
            if chunk_manager.completed_files().is_empty() {
                msg_chk_mgr_no_verified_file_nor_re_upload();
            }
            iterative_uploader::msg_chunk_manager_upload_complete(chunk_manager.clone());

            return Ok(vec![]);
        }
        msg_unverified_chunks_reattempted(&failed_chunks.len());
        failed_chunks
    } else {
        let mut chunks = chunk_manager.get_chunks();
        let mut rng = thread_rng();
        chunks.shuffle(&mut rng);
        chunks
    };
    Ok(chunks_to_upload)
}

pub fn get_progress_bar(length: u64) -> Result<ProgressBar> {
    let progress_bar = ProgressBar::new(length);
    progress_bar.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len}")?
            .progress_chars("#>-"),
    );
    progress_bar.enable_steady_tick(Duration::from_millis(100));
    Ok(progress_bar)
}

fn msg_files_already_uploaded_verified() {
    println!("All files were already uploaded and verified");
    println!("**************************************");
    println!("*          Uploaded Files            *");
}

fn msg_chk_mgr_no_verified_file_nor_re_upload() {
    println!("chunk_manager doesn't have any verified_files, nor any failed_chunks to re-upload.");
}

fn msg_not_public_by_default() {
    println!("*                                    *");
    println!("*  These are not public by default.  *");
    println!("*     Reupload with `-p` option      *");
    println!("*      to publish the datamaps.      *");
}

fn msg_star_line() {
    println!("**************************************");
}

fn msg_unverified_chunks_reattempted(failed_amount: &usize) {
    println!(
        "{failed_amount} chunks were uploaded in the past but failed to verify. Will attempt to upload them again..."
    );
}
