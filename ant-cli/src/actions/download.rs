// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::get_progress_bar;
use autonomi::{
    chunk::DataMapChunk,
    client::{
        analyze::Analysis,
        files::{archive_private::PrivateArchiveDataMap, archive_public::ArchiveAddress},
        GetError,
    },
    data::DataAddress,
    files::{PrivateArchive, PublicArchive},
    Client,
};
use color_eyre::{
    eyre::{eyre, Context, Result},
    Section,
};
use std::path::PathBuf;

pub async fn download(addr: &str, dest_path: &str, client: &Client) -> Result<()> {
    let try_public_address = DataAddress::from_hex(addr).ok();
    if let Some(public_address) = try_public_address {
        return download_public(addr, public_address, dest_path, client).await;
    }

    let try_private_address = crate::user_data::get_local_private_archive_access(addr).ok();
    if let Some(private_address) = try_private_address {
        return download_private(addr, private_address, dest_path, client).await;
    }

    let try_datamap = DataMapChunk::from_hex(addr).ok();
    if let Some(datamap) = try_datamap {
        return download_from_datamap(addr, datamap, dest_path, client).await;
    }

    Err(eyre!("Failed to parse data address {addr}"))
            .with_suggestion(|| "Public addresses look like this: 0037cfa13eae4393841cbc00c3a33cade0f98b8c1f20826e5c51f8269e7b09d7")
            .with_suggestion(|| "Private addresses look like this: 1358645341480028172")
            .with_suggestion(|| "You can also use a hex encoded DataMap directly here")
            .with_suggestion(|| "Try the `file list` command to get addresses you have access to")
}

async fn download_private(
    addr: &str,
    private_address: PrivateArchiveDataMap,
    dest_path: &str,
    client: &Client,
) -> Result<()> {
    let archive = client
        .archive_get(&private_address)
        .await
        .wrap_err("Failed to fetch Private Archive from address")?;
    download_priv_archive_to_disk(addr, archive, dest_path, client).await
}

async fn download_priv_archive_to_disk(
    addr: &str,
    archive: PrivateArchive,
    dest_path: &str,
    client: &Client,
) -> Result<()> {
    let progress_bar = get_progress_bar(archive.iter().count() as u64)?;
    let mut all_errs = vec![];
    for (path, access, _meta) in archive.iter() {
        progress_bar.println(format!("Fetching file: {path:?}..."));
        let bytes = match client.data_get(access).await {
            Ok(bytes) => bytes,
            Err(e) => {
                let err = format!("Failed to fetch file {path:?}: {e}");
                all_errs.push(err);
                continue;
            }
        };

        let path = PathBuf::from(dest_path).join(path);
        let here = PathBuf::from(".");
        let parent = path.parent().unwrap_or_else(|| &here);
        std::fs::create_dir_all(parent)?;
        std::fs::write(path, bytes)?;
        progress_bar.clone().inc(1);
    }
    progress_bar.finish_and_clear();

    if all_errs.is_empty() {
        info!("Successfully downloaded private data with local address: {addr}");
        println!("Successfully downloaded private data with local address: {addr}");
        Ok(())
    } else {
        let err_no = all_errs.len();
        eprintln!("{err_no} errors while downloading private data with local address: {addr}");
        eprintln!("{all_errs:#?}");
        error!("Errors while downloading private data with local address {addr}: {all_errs:#?}");
        Err(eyre!("Errors while downloading private data"))
    }
}

async fn download_public(
    addr: &str,
    address: ArchiveAddress,
    dest_path: &str,
    client: &Client,
) -> Result<()> {
    let archive = match client.archive_get_public(&address).await {
        Ok(archive) => archive,
        Err(GetError::Deserialization(_)) => {
            info!("Failed to deserialize Public Archive from address {addr}, trying to fetch data assuming it is a single file instead");
            return download_public_single_file(addr, address, dest_path, client)
                .await
                .wrap_err("Failed to fetch public file from address");
        }
        Err(e) => return Err(e).wrap_err("Failed to fetch Public Archive from address")?,
    };
    download_pub_archive_to_disk(addr, archive, dest_path, client).await
}

async fn download_pub_archive_to_disk(
    addr: &str,
    archive: PublicArchive,
    dest_path: &str,
    client: &Client,
) -> Result<()> {
    let progress_bar = get_progress_bar(archive.iter().count() as u64)?;
    let mut all_errs = vec![];
    for (path, addr, _meta) in archive.iter() {
        progress_bar.println(format!("Fetching file: {path:?}..."));
        let bytes = match client.data_get_public(addr).await {
            Ok(bytes) => bytes,
            Err(e) => {
                let err = format!("Failed to fetch file {path:?}: {e}");
                all_errs.push(err);
                continue;
            }
        };

        let path = PathBuf::from(dest_path).join(path);
        let here = PathBuf::from(".");
        let parent = path.parent().unwrap_or_else(|| &here);
        std::fs::create_dir_all(parent)?;
        std::fs::write(path, bytes)?;
        progress_bar.clone().inc(1);
    }
    progress_bar.finish_and_clear();

    if all_errs.is_empty() {
        info!("Successfully downloaded data at: {addr}");
        println!("Successfully downloaded data at: {addr}");
        Ok(())
    } else {
        let err_no = all_errs.len();
        eprintln!("{err_no} errors while downloading data at: {addr}");
        eprintln!("{all_errs:#?}");
        error!("Errors while downloading data at {addr}: {all_errs:#?}");
        Err(eyre!("Errors while downloading data"))
    }
}

async fn download_public_single_file(
    addr: &str,
    address: DataAddress,
    dest_path: &str,
    client: &Client,
) -> Result<()> {
    let bytes = match client.data_get_public(&address).await {
        Ok(bytes) => bytes,
        Err(e) => {
            let err = format!("Failed to fetch file at {addr:?}: {e}");
            return Err(eyre!(err)).wrap_err("Failed to fetch file content from address");
        }
    };

    let path = PathBuf::from(dest_path);
    let here = PathBuf::from(".");
    let parent = path.parent().unwrap_or_else(|| &here);
    std::fs::create_dir_all(parent)?;
    std::fs::write(path, bytes)?;
    info!("Successfully downloaded file at: {addr}");
    println!("Successfully downloaded file at: {addr}");
    Ok(())
}

async fn download_from_datamap(
    addr: &str,
    datamap: DataMapChunk,
    dest_path: &str,
    client: &Client,
) -> Result<()> {
    match client.analyze_address(&datamap.to_hex(), false).await {
        Ok(Analysis::RawDataMap { data, .. }) => {
            let path = PathBuf::from(dest_path);
            let here = PathBuf::from(".");
            let parent = path.parent().unwrap_or_else(|| &here);
            std::fs::create_dir_all(parent)?;
            std::fs::write(path, data)?;
            info!("Successfully downloaded file from datamap at: {addr}");
            println!("Successfully downloaded file from datamap at: {addr}");
            Ok(())
        }
        Ok(Analysis::PublicArchive { archive, .. }) => {
            info!("Detected public archive at: {addr}");
            download_pub_archive_to_disk(addr, archive, dest_path, client).await
        }
        Ok(Analysis::PrivateArchive(private_archive)) => {
            info!("Detected private archive at: {addr}");
            download_priv_archive_to_disk(addr, private_archive, dest_path, client).await
        }
        Ok(a) => {
            let err = format!("Unexpected data type found at {addr:?}: {a}");
            Err(eyre!(err)).wrap_err("Failed to fetch file from address")
        }
        Err(e) => {
            let err = format!("Failed to fetch file {addr:?}: {e}");
            Err(eyre!(err)).wrap_err("Failed to fetch file content from address")
        }
    }
}
