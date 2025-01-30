// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::get_progress_bar;
use crate::{
    exit_code,
    output::{collect_task_summary, FileDownloadOutput},
};
use autonomi::{
    client::{
        address::str_to_addr,
        files::{archive_private::PrivateArchiveAccess, archive_public::ArchiveAddr},
    },
    files::DownloadError,
    Client,
};
use std::{path::PathBuf, process, time::SystemTime};

pub async fn download(
    addr: &str,
    dest_path: &str,
    client: &mut Client,
    json: bool,
) -> Result<(), color_eyre::Report> {
    let start_time = SystemTime::now();
    let event_receiver = client.enable_client_events();
    let (task_summary_thread, stop_summary_collection) = collect_task_summary(event_receiver);

    let public_address = str_to_addr(addr).ok();
    let private_address = crate::user_data::get_local_private_archive_access(addr)
        .inspect_err(|e| error!("Failed to get private archive access: {e}"))
        .ok();

    let public;
    let result = match (public_address, private_address) {
        (Some(public_address), _) => {
            public = true;
            download_public(public_address, dest_path, client).await
        }
        (_, Some(private_address)) => {
            public = false;
            download_private(private_address, dest_path, client).await
        }
        _ => {
            println!("Public addresses look like this: 0037cfa13eae4393841cbc00c3a33cade0f98b8c1f20826e5c51f8269e7b09d7");
            println!("Private addresses look like this: 1358645341480028172");
            println!("Try the `file list` command to get addresses you have access to");
            process::exit(exit_code::INVALID_INPUT_EXIT_CODE);
        }
    };

    // wait for upload to complete
    if let Err(e) = stop_summary_collection.send(()) {
        error!("Failed to send upload completed event: {e:?}");
    }

    let task_summary = task_summary_thread.await?;
    let download_output = FileDownloadOutput {
        exit_code: result
            .as_ref()
            .map_or_else(|(err, _)| exit_code::download_exit_code(err), |_| 0),
        addr: addr.to_string(),
        public,
        task_summary,
        start_time,
        end_time: SystemTime::now(),
        failed_files: result
            .as_ref()
            .map_or_else(|(_, failed_files)| failed_files.clone(), |_| vec![]),
    };

    if json {
        download_output.print_json();
    } else {
        download_output.print();
    }

    if let Err((err, _)) = result {
        process::exit(exit_code::download_exit_code(&err));
    }
    Ok(())
}

async fn download_private(
    private_address: PrivateArchiveAccess,
    dest_path: &str,
    client: &mut Client,
) -> Result<(), (DownloadError, Vec<(String, String)>)> {
    let archive = client
        .archive_get(private_address.clone())
        .await
        .map_err(|err| {
            error!("Failed to fetch archive from address: {private_address:?}");
            (err.into(), vec![])
        })?;

    let progress_bar = get_progress_bar(archive.iter().count() as u64)
        .inspect_err(|err| {
            error!("Failed to create progress bar: {err}");
        })
        .ok();
    let mut all_errs = vec![];
    let mut last_error = None;

    for (path, access, _meta) in archive.iter() {
        let bytes = match client.data_get(access.clone()).await {
            Ok(bytes) => bytes,
            Err(e) => {
                all_errs.push((path.to_string_lossy().into_owned(), format!("{e}")));
                last_error = Some(e);
                continue;
            }
        };

        let path = PathBuf::from(dest_path).join(path);
        let here = PathBuf::from(".");
        let parent = path.parent().unwrap_or_else(|| &here);
        std::fs::create_dir_all(parent).map_err(|err| {
            error!("Failed to create parent directories for {path:?}: {err}");
            (err.into(), all_errs.clone())
        })?;
        std::fs::write(&path, bytes).map_err(|err| {
            error!("Failed to write file {path:?}: {err}");
            (err.into(), all_errs.clone())
        })?;
        if let Some(ref progress_bar) = progress_bar {
            progress_bar.clone().inc(1);
        }
    }
    if let Some(ref progress_bar) = progress_bar {
        progress_bar.finish_and_clear();
    }

    if let Some(e) = last_error {
        Err((e.into(), all_errs))
    } else {
        Ok(())
    }
}

async fn download_public(
    address: ArchiveAddr,
    dest_path: &str,
    client: &mut Client,
) -> Result<(), (DownloadError, Vec<(String, String)>)> {
    let archive = client.archive_get_public(address).await.map_err(|err| {
        error!("Failed to fetch archive from address: {address:?}");
        (err.into(), vec![])
    })?;

    let progress_bar = get_progress_bar(archive.iter().count() as u64)
        .inspect_err(|err| {
            error!("Failed to create progress bar: {err}");
        })
        .ok();
    let mut all_errs = vec![];
    let mut last_error = None;

    for (path, addr, _meta) in archive.iter() {
        let bytes = match client.data_get_public(*addr).await {
            Ok(bytes) => bytes,
            Err(e) => {
                all_errs.push((path.to_string_lossy().into_owned(), format!("{e}")));
                last_error = Some(e);
                continue;
            }
        };

        let path = PathBuf::from(dest_path).join(path);
        let here = PathBuf::from(".");
        let parent = path.parent().unwrap_or_else(|| &here);
        std::fs::create_dir_all(parent).map_err(|err| {
            error!("Failed to create parent directories for {path:?}: {err}");
            (err.into(), all_errs.clone())
        })?;
        std::fs::write(&path, bytes).map_err(|err| {
            error!("Failed to write file {path:?}: {err}");
            (err.into(), all_errs.clone())
        })?;
        if let Some(ref progress_bar) = progress_bar {
            progress_bar.clone().inc(1);
        }
    }
    if let Some(ref progress_bar) = progress_bar {
        progress_bar.finish_and_clear();
    }

    if let Some(e) = last_error {
        Err((e.into(), all_errs))
    } else {
        Ok(())
    }
}
