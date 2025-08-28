// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::get_progress_bar;
use crate::exit_code::{self, ExitCodeError, INVALID_INPUT_EXIT_CODE, IO_ERROR};
use autonomi::{
    Client,
    chunk::DataMapChunk,
    client::{GetError, analyze::Analysis, files::archive_private::PrivateArchiveDataMap},
    data::DataAddress,
    files::{PrivateArchive, PublicArchive},
};
use color_eyre::{Section, eyre::eyre};
use std::path::PathBuf;

pub async fn download(addr: &str, dest_path: &str, client: &Client) -> Result<(), ExitCodeError> {
    let try_public_address = DataAddress::from_hex(addr).ok();
    if let Some(public_address) = try_public_address {
        println!("Input supplied was a public address");
        return download_public(addr, public_address, dest_path, client).await;
    }

    let try_local_private_archive = crate::user_data::get_local_private_archive_access(addr).ok();
    if let Some(private_address) = try_local_private_archive {
        println!("Input supplied was a private address");
        return download_private(addr, private_address, dest_path, client).await;
    }

    let try_local_private_file = crate::user_data::get_local_private_file_access(addr).ok();
    if let Some(private_file_datamap) = try_local_private_file {
        println!("Input supplied was a private file datamap");
        return download_from_datamap(addr, private_file_datamap, dest_path, client).await;
    }

    let try_datamap = DataMapChunk::from_hex(addr).ok();
    if let Some(datamap) = try_datamap {
        println!("Input supplied was a datamap Chunk");
        return download_from_datamap(addr, datamap, dest_path, client).await;
    }

    Err((eyre!("Failed to parse data address {addr}")
            .with_suggestion(|| "Public addresses look like this: 0037cfa13eae4393841cbc00c3a33cade0f98b8c1f20826e5c51f8269e7b09d7")
            .with_suggestion(|| "Private addresses look like this: 1358645341480028172")
            .with_suggestion(|| "You can also use a hex encoded DataMap directly here")
            .with_suggestion(|| "Try the `file list` command to get addresses you have access to"),
        INVALID_INPUT_EXIT_CODE
    ))
}

async fn download_private(
    addr: &str,
    private_address: PrivateArchiveDataMap,
    dest_path: &str,
    client: &Client,
) -> Result<(), ExitCodeError> {
    let archive = client.archive_get(&private_address).await.map_err(|e| {
        let exit_code = exit_code::get_error_exit_code(&e);
        (
            eyre!(e).wrap_err("Failed to fetch Private Archive from address"),
            exit_code,
        )
    })?;

    download_priv_archive_to_disk(addr, archive, dest_path, client).await
}

async fn download_priv_archive_to_disk(
    addr: &str,
    archive: PrivateArchive,
    dest_path: &str,
    client: &Client,
) -> Result<(), ExitCodeError> {
    let progress_bar = get_progress_bar(archive.iter().count() as u64).ok();
    let mut all_errs = vec![];
    let mut last_error = None;
    for (path, access, _meta) in archive.iter() {
        if let Some(progress_bar) = &progress_bar {
            progress_bar.println(format!("Fetching file: {path:?}..."));
        }

        let path = PathBuf::from(dest_path).join(path);
        let here = PathBuf::from(".");
        let parent = path.parent().unwrap_or_else(|| &here);
        std::fs::create_dir_all(parent).map_err(|err| (err.into(), IO_ERROR))?;

        if let Err(e) = client.file_download(access, path.clone()).await {
            let err = format!("Failed to fetch file {path:?}: {e}");
            all_errs.push(err);
            last_error = Some(e);
            continue;
        }

        if let Some(progress_bar) = &progress_bar {
            progress_bar.inc(1);
        }
    }
    if let Some(progress_bar) = &progress_bar {
        progress_bar.finish_and_clear();
    }

    match last_error {
        Some(e) => {
            let exit_code = exit_code::get_download_error_exit_code(&e);
            let err_no = all_errs.len();
            eprintln!("{err_no} errors while downloading private data with local address: {addr}");
            eprintln!("{all_errs:#?}");
            error!(
                "Errors while downloading private data with local address {addr}: {all_errs:#?}"
            );
            Err((eyre!("Errors while downloading private data"), exit_code))
        }
        None => {
            info!("Successfully downloaded private data with local address: {addr}");
            println!("Successfully downloaded private data with local address: {addr}");
            Ok(())
        }
    }
}

async fn download_public(
    addr: &str,
    address: DataAddress,
    dest_path: &str,
    client: &Client,
) -> Result<(), ExitCodeError> {
    let path = PathBuf::from(dest_path);
    let here = PathBuf::from(".");
    let parent = path.parent().unwrap_or_else(|| &here);
    std::fs::create_dir_all(parent).map_err(|err| (err.into(), IO_ERROR))?;

    let data = match client.data_get_public(&address).await {
        Ok(data) => data,
        Err(GetError::TooLargeForMemory) => {
            println!("Detected large file at: {addr}, downloading via streaming");
            info!("Detected large file at: {addr}, downloading via streaming");
            client
                .file_download_public(&address, path)
                .await
                .map_err(|e| {
                    let exit_code = exit_code::get_download_error_exit_code(&e);
                    (
                        eyre!(e).wrap_err("Failed to fetch data from address"),
                        exit_code,
                    )
                })?;
            println!("Successfully downloaded file at: {addr}");
            return Ok(());
        }
        Err(e) => {
            let exit_code = exit_code::get_error_exit_code(&e);
            return Err((
                eyre!(e).wrap_err("Failed to fetch data from address"),
                exit_code,
            ));
        }
    };

    // Try to deserialize as archive
    match PublicArchive::from_bytes(data.clone()) {
        Ok(archive) => {
            println!("Successfully deserialized as Public Archive at: {addr}");
            info!("Successfully deserialized as Public Archive at: {addr}");
            download_pub_archive_to_disk(addr, archive, dest_path, client).await
        }
        Err(_) => {
            info!(
                "Failed to deserialize as Public Archive from address {addr}, treating as single file"
            );
            // Write the raw data as a file
            std::fs::write(path, data).map_err(|err| (err.into(), IO_ERROR))?;
            info!("Successfully downloaded file at: {addr}");
            println!("Successfully downloaded file at: {addr}");
            Ok(())
        }
    }
}

async fn download_pub_archive_to_disk(
    addr: &str,
    archive: PublicArchive,
    dest_path: &str,
    client: &Client,
) -> Result<(), ExitCodeError> {
    let progress_bar = get_progress_bar(archive.iter().count() as u64).ok();
    let mut all_errs = vec![];
    let mut last_error = None;
    for (path, addr, _meta) in archive.iter() {
        if let Some(progress_bar) = &progress_bar {
            progress_bar.println(format!("Fetching file: {path:?}..."));
        }

        let path = PathBuf::from(dest_path).join(path);
        let here = PathBuf::from(".");
        let parent = path.parent().unwrap_or_else(|| &here);
        std::fs::create_dir_all(parent).map_err(|err| (err.into(), IO_ERROR))?;

        if let Err(e) = client.file_download_public(addr, path.clone()).await {
            let err = format!("Failed to fetch file {path:?}: {e}");
            all_errs.push(err);
            last_error = Some(e);
            continue;
        };

        if let Some(progress_bar) = &progress_bar {
            progress_bar.inc(1);
        }
    }
    if let Some(progress_bar) = &progress_bar {
        progress_bar.finish_and_clear();
    }

    match last_error {
        Some(e) => {
            let exit_code = exit_code::get_download_error_exit_code(&e);
            let err_no = all_errs.len();
            eprintln!("{err_no} errors while downloading data at: {addr}");
            eprintln!("{all_errs:#?}");
            error!("Errors while downloading data at {addr}: {all_errs:#?}");
            Err((eyre!("Errors while downloading data"), exit_code))
        }
        None => {
            info!("Successfully downloaded data at: {addr}");
            println!("Successfully downloaded data at: {addr}");
            Ok(())
        }
    }
}

// The `addr` string here could be the entire datamap chunk hexed content.
async fn download_from_datamap(
    addr: &str,
    datamap: DataMapChunk,
    dest_path: &str,
    client: &Client,
) -> Result<(), ExitCodeError> {
    let datamap_addr = datamap.address();

    info!("Analyzing datamap at: {datamap_addr}");
    println!("Analyzing datamap at: {datamap_addr}");

    match client.analyze_address(&datamap.to_hex(), true).await {
        Ok(Analysis::RawDataMap { data, .. }) => {
            let path = PathBuf::from(dest_path);
            let here = PathBuf::from(".");
            let parent = path.parent().unwrap_or_else(|| &here);
            std::fs::create_dir_all(parent).map_err(|err| (err.into(), IO_ERROR))?;

            if let Some(data) = data {
                std::fs::write(path, data).map_err(|err| (err.into(), IO_ERROR))?;
            } else if let Err(e) = client.file_download(&datamap, path).await {
                let exit_code = exit_code::get_download_error_exit_code(&e);
                return Err((
                    eyre!("Errors while downloading from {datamap_addr:?}"),
                    exit_code,
                ));
            }

            info!("Successfully downloaded file from datamap at: {datamap_addr}");
            println!("Successfully downloaded file from datamap at: {datamap_addr}");
            Ok(())
        }
        Ok(Analysis::PublicArchive { archive, .. }) => {
            info!("Detected public archive at: {datamap_addr}");
            download_pub_archive_to_disk(addr, archive, dest_path, client).await
        }
        Ok(Analysis::PrivateArchive(private_archive)) => {
            info!("Detected private archive at: {datamap_addr}");
            download_priv_archive_to_disk(addr, private_archive, dest_path, client).await
        }
        Ok(a) => {
            let err = format!("Unexpected data type found at {datamap_addr}: {a}");
            Err((
                eyre!(err).wrap_err("Failed to fetch file from address"),
                INVALID_INPUT_EXIT_CODE,
            ))
        }
        Err(e) => {
            let exit_code = exit_code::analysis_exit_code(&e);
            Err((
                eyre!(e).wrap_err(format!("Failed to fetch file at {datamap_addr}")),
                exit_code,
            ))
        }
    }
}
