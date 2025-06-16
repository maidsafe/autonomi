// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

pub mod auditor;
pub mod daemon;
pub mod faucet;
pub mod local;
pub mod nat_detection;
pub mod node;

use crate::{
    helpers::{download_and_extract_release, get_bin_version},
    print_banner, VerbosityLevel,
};
use ant_releases::{AntReleaseRepoActions, ReleaseType};
use ant_service_management::UpgradeResult;
use color_eyre::{eyre::eyre, Result};
use colored::Colorize;
use semver::Version;
use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

pub async fn download_and_get_upgrade_bin_path(
    custom_bin_path: Option<PathBuf>,
    release_type: ReleaseType,
    url: Option<String>,
    version: Option<String>,
    verbosity: VerbosityLevel,
) -> Result<(PathBuf, Version)> {
    if let Some(path) = custom_bin_path {
        debug!(
            "Using the supplied custom binary at {}",
            path.to_string_lossy()
        );
        println!(
            "Using the supplied custom binary at {}",
            path.to_string_lossy()
        );
        let bin_version = get_bin_version(&path)?;
        return Ok((path, bin_version.parse()?));
    }

    let release_repo = <dyn AntReleaseRepoActions>::default_config();
    if let Some(version) = version {
        debug!("Downloading provided version {version} of {release_type}");
        let (upgrade_bin_path, version) = download_and_extract_release(
            release_type,
            None,
            Some(version),
            &*release_repo,
            verbosity,
            None,
        )
        .await?;
        Ok((upgrade_bin_path, Version::parse(&version)?))
    } else if let Some(url) = url {
        debug!("Downloading {release_type} from url: {url}");
        let (upgrade_bin_path, version) = download_and_extract_release(
            release_type,
            Some(url),
            None,
            &*release_repo,
            verbosity,
            None,
        )
        .await?;
        Ok((upgrade_bin_path, Version::parse(&version)?))
    } else {
        if verbosity != VerbosityLevel::Minimal {
            println!("Retrieving latest version of {release_type}...");
        }
        debug!("Retrieving latest version of {release_type}...");
        let latest_version = release_repo.get_latest_version(&release_type).await?;
        if verbosity != VerbosityLevel::Minimal {
            println!("Latest version is {latest_version}");
        }
        debug!("Download latest version {latest_version} of {release_type}");

        let (upgrade_bin_path, _) = download_and_extract_release(
            release_type,
            None,
            Some(latest_version.to_string()),
            &*release_repo,
            verbosity,
            None,
        )
        .await?;
        Ok((upgrade_bin_path, latest_version))
    }
}

pub fn print_upgrade_summary(upgrade_summary: Vec<(String, UpgradeResult)>) {
    println!("Upgrade summary:");
    for (service_name, upgrade_result) in upgrade_summary {
        match upgrade_result {
            UpgradeResult::NotRequired => {
                println!("- {} did not require an upgrade", service_name);
            }
            UpgradeResult::Upgraded(previous_version, new_version) => {
                println!(
                    "{} {} upgraded from {previous_version} to {new_version}",
                    "✓".green(),
                    service_name
                );
            }
            UpgradeResult::UpgradedButNotStarted(previous_version, new_version, _) => {
                println!(
                    "{} {} was upgraded from {previous_version} to {new_version} but it did not start",
                    "✕".red(),
                    service_name
                );
            }
            UpgradeResult::Forced(previous_version, target_version) => {
                println!(
                    "{} Forced {} version change from {previous_version} to {target_version}.",
                    "✓".green(),
                    service_name
                );
            }
            UpgradeResult::Error(msg) => {
                println!("{} {} was not upgraded: {}", "✕".red(), service_name, msg);
            }
        }
    }
}

pub async fn get_bin_path(
    build: bool,
    path: Option<PathBuf>,
    release_type: ReleaseType,
    version: Option<String>,
    release_repo: &dyn AntReleaseRepoActions,
    verbosity: VerbosityLevel,
) -> Result<PathBuf> {
    if build {
        debug!("Obtaining bin path for {release_type:?} by building");
        let target_dir = build_binary(&release_type)?;
        Ok(target_dir.join(release_type.to_string()))
    } else if let Some(path) = path {
        debug!("Using the supplied custom binary for {release_type:?}: {path:?}");
        Ok(path)
    } else {
        debug!("Downloading {release_type:?} binary with version {version:?}");
        let (download_path, _) = download_and_extract_release(
            release_type,
            None,
            version,
            release_repo,
            verbosity,
            None,
        )
        .await?;
        Ok(download_path)
    }
}

// Returns the target dir after building the binary
fn build_binary(bin_type: &ReleaseType) -> Result<PathBuf> {
    debug!("Building {bin_type} binary");
    let mut args = vec!["build", "--release"];
    let bin_name = bin_type.to_string();
    args.push("--bin");
    args.push(&bin_name);

    // Keep features consistent to avoid recompiling.
    if cfg!(feature = "chaos") {
        println!("*** Building testnet with CHAOS enabled. Watch out. ***");
        args.push("--features");
        args.push("chaos");
    }
    if cfg!(feature = "statemap") {
        args.extend(["--features", "statemap"]);
    }
    if cfg!(feature = "otlp") {
        args.extend(["--features", "otlp"]);
    }
    if cfg!(feature = "websockets") {
        args.extend(["--features", "websockets"]);
    }
    if cfg!(feature = "open-metrics") {
        args.extend(["--features", "open-metrics"]);
    }

    print_banner(&format!("Building {} binary", bin_name));

    let mut target_dir = PathBuf::new();
    let mut build_result = Command::new("cargo");
    let _ = build_result.args(args.clone());

    if let Ok(val) = std::env::var("CARGO_TARGET_DIR") {
        let _ = build_result.env("CARGO_TARGET_DIR", val.clone());
        target_dir.push(val);
    } else {
        target_dir.push("target");
    }
    let target_dir = target_dir.join("release");

    let build_result = build_result
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()?;

    if !build_result.status.success() {
        error!("Failed to build binaries {bin_name}");
        return Err(eyre!("Failed to build binaries"));
    }

    Ok(target_dir)
}
