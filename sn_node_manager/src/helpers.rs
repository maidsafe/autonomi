// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use color_eyre::{
    eyre::{bail, eyre},
    Result,
};
use indicatif::{ProgressBar, ProgressStyle};
use sn_releases::{get_running_platform, ArchiveType, ReleaseType, SafeReleaseRepositoryInterface};
use std::{
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    sync::Arc,
};

const MAX_DOWNLOAD_RETRIES: u8 = 3;

/// Downloads and extracts a release binary to a temporary location.
pub async fn download_and_extract_release(
    release_type: ReleaseType,
    url: Option<String>,
    version: Option<String>,
    release_repo: &dyn SafeReleaseRepositoryInterface,
) -> Result<(PathBuf, String)> {
    let pb = Arc::new(ProgressBar::new(0));
    pb.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
        .progress_chars("#>-"));
    let pb_clone = pb.clone();
    let callback: Box<dyn Fn(u64, u64) + Send + Sync> = Box::new(move |downloaded, total| {
        pb_clone.set_length(total);
        pb_clone.set_position(downloaded);
    });

    let temp_dir_path = create_temp_dir()?;

    let mut download_attempts = 1;
    let archive_path = loop {
        if download_attempts > MAX_DOWNLOAD_RETRIES {
            bail!("Failed to download release after {MAX_DOWNLOAD_RETRIES} tries.");
        }

        if let Some(url) = &url {
            println!("Retrieving {release_type} from {url}");
            match release_repo
                .download_release(url, &temp_dir_path, &callback)
                .await
            {
                Ok(archive_path) => break archive_path,
                Err(err) => {
                    println!("Error while downloading release. Trying again {download_attempts}/{MAX_DOWNLOAD_RETRIES}: {err:?}");
                    download_attempts += 1;
                    pb.finish_and_clear();
                }
            }
        } else {
            let version = if let Some(version) = version.clone() {
                version
            } else {
                println!("Retrieving latest version for {release_type}...");
                release_repo.get_latest_version(&release_type).await?
            };

            println!("Downloading {release_type} version {version}...");
            match release_repo
                .download_release_from_s3(
                    &release_type,
                    &version,
                    &get_running_platform()?,
                    &ArchiveType::TarGz,
                    &temp_dir_path,
                    &callback,
                )
                .await
            {
                Ok(archive_path) => break archive_path,
                Err(err) => {
                    println!("Error while downloading release. Trying again {download_attempts}/{MAX_DOWNLOAD_RETRIES}: {err:?}");
                    download_attempts += 1;
                    pb.finish_and_clear();
                }
            }
        };
    };
    pb.finish_and_clear();

    let safenode_download_path =
        release_repo.extract_release_archive(&archive_path, &temp_dir_path)?;

    println!("Download completed: {}", &safenode_download_path.display());

    // Finally, obtain the version number from the binary by running `--version`. This is useful
    // when the `--url` argument is used, and in any case, ultimately the binary we obtained is the
    // source of truth.
    let bin_version = get_bin_version(&safenode_download_path)?;

    Ok((safenode_download_path, bin_version))
}

pub fn get_bin_version(bin_path: &PathBuf) -> Result<String> {
    let mut cmd = Command::new(bin_path)
        .arg("--version")
        .stdout(Stdio::piped())
        .spawn()?;

    let mut output = String::new();
    cmd.stdout
        .as_mut()
        .ok_or_else(|| eyre!("Failed to capture stdout"))?
        .read_to_string(&mut output)?;

    let version = output
        .split_whitespace()
        .last()
        .ok_or_else(|| eyre!("Failed to parse version"))?
        .to_string();

    Ok(version)
}

/// There is a `tempdir` crate that provides the same kind of functionality, but it was flagged for
/// a security vulnerability.
fn create_temp_dir() -> Result<PathBuf> {
    let temp_dir = std::env::temp_dir();
    let unique_dir_name = uuid::Uuid::new_v4().to_string();
    let new_temp_dir = temp_dir.join(unique_dir_name);
    std::fs::create_dir_all(&new_temp_dir)?;
    Ok(new_temp_dir)
}
