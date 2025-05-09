// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_releases::{get_running_platform, AntReleaseRepoActions, ArchiveType, ReleaseType};
use ant_service_management::NodeServiceData;
use color_eyre::{
    eyre::{bail, eyre},
    Result,
};
use indicatif::{ProgressBar, ProgressStyle};
use semver::Version;
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
};

use crate::{add_services::config::PortRange, config, VerbosityLevel};

const MAX_DOWNLOAD_RETRIES: u8 = 3;

// We need deterministic and fix path for the faucet wallet.
// Otherwise the test instances will not be able to find the same faucet instance.
pub fn get_faucet_data_dir() -> PathBuf {
    let mut data_dirs = dirs_next::data_dir().expect("A homedir to exist.");
    data_dirs.push("autonomi");
    data_dirs.push("test_faucet");
    std::fs::create_dir_all(data_dirs.as_path())
        .expect("Faucet test path to be successfully created.");
    data_dirs
}

#[cfg(windows)]
pub async fn configure_winsw(dest_path: &Path, verbosity: VerbosityLevel) -> Result<()> {
    if which::which("winsw.exe").is_ok() {
        debug!("WinSW already installed, which returned Ok");
        return Ok(());
    }

    if !dest_path.exists() {
        if verbosity != VerbosityLevel::Minimal {
            println!("Downloading winsw.exe...");
        }
        debug!("Downloading WinSW to {dest_path:?}");

        let release_repo = <dyn AntReleaseRepoActions>::default_config();

        let mut pb = None;
        let callback = if verbosity != VerbosityLevel::Minimal {
            let progress_bar = Arc::new(ProgressBar::new(0));
            progress_bar.set_style(ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
                .progress_chars("#>-"));
            pb = Some(progress_bar.clone());
            let pb_clone = progress_bar.clone();
            let callback: Box<dyn Fn(u64, u64) + Send + Sync> =
                Box::new(move |downloaded, total| {
                    pb_clone.set_length(total);
                    pb_clone.set_position(downloaded);
                });
            callback
        } else {
            let callback: Box<dyn Fn(u64, u64) + Send + Sync> = Box::new(move |_, _| {});
            callback
        };

        let mut download_attempts = 1;
        loop {
            if download_attempts > MAX_DOWNLOAD_RETRIES {
                error!("Failed to download WinSW after {MAX_DOWNLOAD_RETRIES} tries.");
                bail!("Failed to download WinSW after {MAX_DOWNLOAD_RETRIES} tries.");
            }
            match release_repo.download_winsw(dest_path, &callback).await {
                Ok(_) => break,
                Err(e) => {
                    if verbosity != VerbosityLevel::Minimal {
                        println!("Error downloading WinSW: {e:?}");
                        println!("Trying again {download_attempts}/{MAX_DOWNLOAD_RETRIES}");
                    }
                    error!("Error downloading WinSW. Trying again {download_attempts}/{MAX_DOWNLOAD_RETRIES}: {e:?}");
                    download_attempts += 1;
                    if let Some(pb) = &pb {
                        pb.finish_and_clear();
                    }
                }
            }
        }

        if let Some(pb) = pb {
            pb.finish_and_clear();
        }
    } else {
        debug!("WinSW already installed, dest_path exists: {dest_path:?}");
    }

    info!("WinSW installed at {dest_path:?}. Setting WINSW_PATH environment variable.");

    std::env::set_var("WINSW_PATH", dest_path.to_string_lossy().to_string());

    Ok(())
}

#[cfg(not(windows))]
pub async fn configure_winsw(_dest_path: &Path, _verbosity: VerbosityLevel) -> Result<()> {
    Ok(())
}

/// Downloads and extracts a release binary to a temporary location.
///
/// If the URL is supplied, that will be downloaded and extracted, and the binary inside the
/// archive will be used; if the version is supplied, a specific version will be downloaded and
/// used; otherwise the latest version will be downloaded and used.
pub async fn download_and_extract_release(
    release_type: ReleaseType,
    url: Option<String>,
    version: Option<String>,
    release_repo: &dyn AntReleaseRepoActions,
    verbosity: VerbosityLevel,
    download_dir_path: Option<PathBuf>,
) -> Result<(PathBuf, String)> {
    debug!(
        "Downloading and extracting release for {release_type}, url: {url:?}, version: {version:?}"
    );
    let mut pb = None;
    let callback = if verbosity != VerbosityLevel::Minimal {
        let progress_bar = Arc::new(ProgressBar::new(0));
        progress_bar.set_style(ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
            .progress_chars("#>-"));
        pb = Some(progress_bar.clone());
        let pb_clone = progress_bar.clone();
        let callback: Box<dyn Fn(u64, u64) + Send + Sync> = Box::new(move |downloaded, total| {
            pb_clone.set_length(total);
            pb_clone.set_position(downloaded);
        });
        callback
    } else {
        let callback: Box<dyn Fn(u64, u64) + Send + Sync> = Box::new(move |_, _| {});
        callback
    };

    let download_dir_path = if let Some(path) = download_dir_path {
        std::fs::create_dir_all(&path)?;
        path
    } else if url.is_some() {
        create_temp_dir()?
    } else {
        // The node manager path can require root access, or can only be accessed by the service
        // user, which is why we have an optional path for the whole function.
        let path = config::get_node_manager_path()?.join("downloads");
        std::fs::create_dir_all(&path)?;
        path
    };
    debug!("Download directory: {download_dir_path:?}");

    let mut download_attempts = 1;
    let binary_download_path = loop {
        if download_attempts > MAX_DOWNLOAD_RETRIES {
            error!("Failed to download release after {MAX_DOWNLOAD_RETRIES} tries.");
            bail!("Failed to download release after {MAX_DOWNLOAD_RETRIES} tries.");
        }

        if let Some(url) = &url {
            info!("Downloading release from {url}");
            if verbosity != VerbosityLevel::Minimal {
                println!("Retrieving {release_type} from {url}");
            }
            match release_repo
                .download_release(url, &download_dir_path, &callback)
                .await
            {
                Ok(archive_path) => {
                    let binary_download_path = release_repo
                        .extract_release_archive(&archive_path, &download_dir_path)
                        .inspect_err(|err| error!("Error while extracting archive {err:?}"))?;
                    break binary_download_path;
                }
                Err(err) => {
                    error!("Error downloading release: {err:?}");
                    if verbosity != VerbosityLevel::Minimal {
                        println!("Error downloading release: {err:?}");
                        println!("Trying again {download_attempts}/{MAX_DOWNLOAD_RETRIES}");
                    }
                    download_attempts += 1;
                    if let Some(pb) = &pb {
                        pb.finish_and_clear();
                    }
                }
            }
        } else {
            let version = if let Some(version) = version.clone() {
                let version = Version::parse(&version)?;
                info!("Downloading release from S3 for version {version}");
                version
            } else {
                if verbosity != VerbosityLevel::Minimal {
                    println!("Retrieving latest version for {release_type}...");
                }
                let version = release_repo
                    .get_latest_version(&release_type)
                    .await
                    .inspect_err(|err| error!("Error obtaining latest version {err:?}"))?;
                info!("Downloading latest version from S3: {version}");
                version
            };

            let archive_name = format!(
                "{}-{}-{}.{}",
                release_type.to_string().to_lowercase(),
                version,
                &get_running_platform()?,
                &ArchiveType::TarGz
            );
            let archive_path = download_dir_path.join(&archive_name);
            if archive_path.exists() {
                // try extracting it, else download it.
                match release_repo.extract_release_archive(&archive_path, &download_dir_path) {
                    Ok(binary_download_path) => {
                        info!("Using cached {release_type} version {version}...");
                        if verbosity != VerbosityLevel::Minimal {
                            println!("Using cached {release_type} version {version}...");
                        }
                        break binary_download_path;
                    }
                    Err(_) => {
                        info!("Cached {release_type} version {version} is corrupted. Downloading again...");
                        if verbosity != VerbosityLevel::Minimal {
                            println!("Cached {release_type} version {version} is corrupted. Downloading again...");
                        }
                    }
                }
            }

            if verbosity != VerbosityLevel::Minimal {
                println!("Downloading {release_type} version {version}...");
            }
            match release_repo
                .download_release_from_s3(
                    &release_type,
                    &version,
                    &get_running_platform()?,
                    &ArchiveType::TarGz,
                    &download_dir_path,
                    &callback,
                )
                .await
            {
                Ok(archive_path) => {
                    let binary_download_path =
                        release_repo.extract_release_archive(&archive_path, &download_dir_path)?;
                    break binary_download_path;
                }
                Err(err) => {
                    error!("Error while downloading release. Trying again {download_attempts}/{MAX_DOWNLOAD_RETRIES}:  {err:?}");
                    if verbosity != VerbosityLevel::Minimal {
                        println!("Error while downloading release. Trying again {download_attempts}/{MAX_DOWNLOAD_RETRIES}: {err:?}");
                    }
                    download_attempts += 1;
                    if let Some(pb) = &pb {
                        pb.finish_and_clear();
                    }
                }
            }
        };
    };
    if let Some(pb) = pb {
        pb.finish_and_clear();
    }
    info!("Download completed: {binary_download_path:?}");

    if verbosity != VerbosityLevel::Minimal {
        println!("Download completed: {}", &binary_download_path.display());
    }

    // Finally, obtain the version number from the binary by running `--version`. This is useful
    // when the `--url` argument is used, and in any case, ultimately the binary we obtained is the
    // source of truth.
    let bin_version = get_bin_version(&binary_download_path)?;

    Ok((binary_download_path, bin_version))
}

pub fn get_bin_version(bin_path: &PathBuf) -> Result<String> {
    debug!("Obtaining version of binary {bin_path:?}");
    let mut cmd = Command::new(bin_path)
        .arg("--version")
        .stdout(Stdio::piped())
        .spawn()
        .inspect_err(|err| error!("The program {bin_path:?} failed to start: {err:?}"))?;

    let mut output = String::new();
    cmd.stdout
        .as_mut()
        .ok_or_else(|| {
            error!("Failed to capture stdout");
            eyre!("Failed to capture stdout")
        })?
        .read_to_string(&mut output)
        .inspect_err(|err| error!("Output contained non utf8 chars: {err:?}"))?;

    // Extract the first line of the output
    let first_line = output.lines().next().ok_or_else(|| {
        error!("No output received from binary");
        eyre!("No output received from binary")
    })?;

    let version = if let Some(v_pos) = first_line.find('v') {
        // Stable binary: Extract version after 'v'
        first_line[v_pos + 1..]
            .split_whitespace()
            .next()
            .map(String::from)
    } else {
        // Nightly binary: Extract the date at the end of the first line
        first_line.split_whitespace().last().map(String::from)
    }
    .ok_or_else(|| {
        error!("Failed to parse version from output");
        eyre!("Failed to parse version from output")
    })?;

    debug!("Obtained version of binary: {version}");

    Ok(version)
}

#[cfg(target_os = "windows")]
pub fn get_username() -> Result<String> {
    Ok(std::env::var("USERNAME")?)
}

#[cfg(not(target_os = "windows"))]
pub fn get_username() -> Result<String> {
    Ok(std::env::var("USER")?)
}

/// There is a `tempdir` crate that provides the same kind of functionality, but it was flagged for
/// a security vulnerability.
pub fn create_temp_dir() -> Result<PathBuf> {
    let temp_dir = std::env::temp_dir();
    let unique_dir_name = uuid::Uuid::new_v4().to_string();
    let new_temp_dir = temp_dir.join(unique_dir_name);
    std::fs::create_dir_all(&new_temp_dir)
        .inspect_err(|err| error!("Failed to crete temp dir: {err:?}"))?;
    Ok(new_temp_dir)
}

/// Get the start port from the `PortRange` if applicable.
pub fn get_start_port_if_applicable(range: Option<PortRange>) -> Option<u16> {
    if let Some(port) = range {
        match port {
            PortRange::Single(val) => return Some(val),
            PortRange::Range(start, _) => return Some(start),
        }
    }
    None
}

/// Increment the port by 1.
pub fn increment_port_option(port: Option<u16>) -> Option<u16> {
    if let Some(port) = port {
        let incremented_port = port + 1;
        return Some(incremented_port);
    }
    None
}

/// Make sure the port is not already in use by another node.
pub fn check_port_availability(port_option: &PortRange, nodes: &[NodeServiceData]) -> Result<()> {
    let mut all_ports = Vec::new();
    for node in nodes {
        if let Some(port) = node.metrics_port {
            all_ports.push(port);
        }
        if let Some(port) = node.node_port {
            all_ports.push(port);
        }
        if let Some(addr) = node.rpc_socket_addr {
            all_ports.push(addr.port());
        }
    }

    match port_option {
        PortRange::Single(port) => {
            if all_ports.iter().any(|p| *p == *port) {
                error!("Port {port} is being used by another service");
                return Err(eyre!("Port {port} is being used by another service"));
            }
        }
        PortRange::Range(start, end) => {
            for i in *start..=*end {
                if all_ports.iter().any(|p| *p == i) {
                    error!("Port {i} is being used by another service");
                    return Err(eyre!("Port {i} is being used by another service"));
                }
            }
        }
    }
    Ok(())
}
