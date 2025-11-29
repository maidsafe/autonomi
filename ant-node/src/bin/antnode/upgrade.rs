// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_releases::{AntReleaseRepoActions, AutonomiReleaseInfo};
use color_eyre::{Result, eyre::eyre};
use fs2::FileExt;
use semver::Version;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{debug, info, warn};

const LOCK_TIMEOUT_SECS: u64 = 300; // 5 minutes
const LOCK_RETRY_INTERVAL_MS: u64 = 100;

/// Calculate SHA256 hash of a file.
pub fn calculate_sha256(path: &Path) -> Result<String> {
    debug!("Calculating SHA256 for: {}", path.display());
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let result = hasher.finalize();
    let hash = format!("{result:x}");
    debug!("SHA256 hash: {hash}");
    Ok(hash)
}

/// Verify that a binary's SHA256 hash matches the expected hash.
pub fn verify_binary_hash(path: &Path, expected_hash: &str) -> Result<bool> {
    let actual_hash = calculate_sha256(path)?;
    let matches = actual_hash.eq_ignore_ascii_case(expected_hash);

    if !matches {
        warn!(
            "Hash mismatch for {}: expected {}, got {}.",
            path.display(),
            expected_hash,
            actual_hash
        );
    }

    Ok(matches)
}

/// Get the upgrade directory path in the user's data directory.
pub fn get_upgrade_dir_path() -> Result<PathBuf> {
    let upgrade_dir_path = dirs_next::data_dir()
        .ok_or_else(|| eyre!("Could not determine user data directory"))?
        .join("autonomi")
        .join("upgrades");
    debug!("Upgrade directory: {}", upgrade_dir_path.display());
    Ok(upgrade_dir_path)
}

/// Get the path where a binary for a specific version should be stored.
pub fn get_binary_path_for_version(commit_hash: &str) -> Result<PathBuf> {
    let upgrade_dir = get_upgrade_dir_path()?;
    let binary_name = if cfg!(target_os = "windows") {
        format!("antnode-{commit_hash}.exe")
    } else {
        format!("antnode-{commit_hash}")
    };
    Ok(upgrade_dir.join(binary_name))
}

/// Acquire an exclusive lock on the upgrade directory.
///
/// It's possible two or more processes could try to download and extract a new binary at the same
/// time.
///
/// Returns the lock file handle that must be kept alive to maintain the lock.
fn acquire_upgrade_lock(upgrade_dir: &Path) -> Result<File> {
    let lock_path = upgrade_dir.join(".lock");
    debug!("Acquiring lock at: {}", lock_path.display());

    let lock_file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)?;
    let start = std::time::Instant::now();
    loop {
        match lock_file.try_lock_exclusive() {
            Ok(_) => {
                info!("Lock acquired");
                return Ok(lock_file);
            }
            Err(_) if start.elapsed().as_secs() < LOCK_TIMEOUT_SECS => {
                debug!("Lock busy. Retrying...");
                std::thread::sleep(Duration::from_millis(LOCK_RETRY_INTERVAL_MS));
            }
            Err(_) => {
                warn!(
                    "Lock timeout after {} seconds. Proceeding without lock...",
                    LOCK_TIMEOUT_SECS
                );
                // Proceed without exclusive lock: hash verification after download will catch any
                // corruption if multiple processes race.
                return Ok(lock_file);
            }
        }
    }
}

/// Download and extract the upgrade binary to the shared location.
///
/// If the binary already exists with the correct hash, returns immediately.
pub async fn download_and_extract_upgrade_binary(
    release_info: &AutonomiReleaseInfo,
    release_repo: &dyn AntReleaseRepoActions,
) -> Result<(PathBuf, String)> {
    let platform = ant_releases::get_running_platform()?;
    info!("Obtained running platform for upgrade: {:?}", platform);

    let platform_binaries = release_info
        .platform_binaries
        .iter()
        .find(|pb| pb.platform == platform)
        .ok_or_else(|| eyre!("No binaries found for platform: {:?}", platform))?;
    let antnode_binary = platform_binaries
        .binaries
        .iter()
        .find(|b| b.name == "antnode")
        .ok_or_else(|| {
            eyre!(
                "antnode binary not found in release for platform: {:?}",
                platform
            )
        })?;
    info!(
        "Found upgrade binary from release info: version {} with hash {}",
        antnode_binary.version, antnode_binary.sha256
    );

    let target_path = get_binary_path_for_version(&release_info.commit_hash)?;
    // First check (pre-lock): Fast-path optimization for cache hits.
    // This avoids expensive lock acquisition when a valid cached binary already exists.
    // This is the common case (90%+ of upgrade checks) since binaries are cached.
    if target_path.exists() {
        info!(
            "Cached binary already exists at {}. Will now verify hash...",
            target_path.display()
        );
        if let Ok(true) = verify_binary_hash(&target_path, &antnode_binary.sha256) {
            info!("Cached binary verified. Will use this binary for the upgrade.");
            return Ok((target_path, antnode_binary.sha256.clone()));
        }
        warn!("Cached binary verification failed. Will download again...");
    }

    let upgrade_dir_path = get_upgrade_dir_path()?;
    fs::create_dir_all(&upgrade_dir_path)?;

    let _lock = acquire_upgrade_lock(&upgrade_dir_path)?;

    // Second check (post-lock): Handle race condition where another process downloaded
    // the binary while this process was waiting for the lock.
    // This prevents duplicate downloads when multiple antnode instances check for upgrades
    // simultaneously and share the same upgrade cache directory.
    if target_path.exists() {
        match verify_binary_hash(&target_path, &antnode_binary.sha256) {
            Ok(true) => {
                info!(
                    "Cached binary verified after acquiring lock. Will use this binary for the upgrade."
                );
                return Ok((target_path, antnode_binary.sha256.clone()));
            }
            Ok(false) | Err(_) => {
                fs::remove_file(&target_path)?;
            }
        }
    }

    info!("Downloading antnode binary...");
    let temp_download_dir_path = upgrade_dir_path.join("tmp");
    fs::create_dir_all(&temp_download_dir_path)?;

    let version = Version::parse(&antnode_binary.version)?;
    let archive_type = ant_releases::ArchiveType::TarGz;
    let callback: Box<dyn Fn(u64, u64) + Send + Sync> = Box::new(|downloaded, total| {
        if total > 0 && downloaded % (total / 10).max(1) == 0 {
            debug!("Download progress: {}/{} bytes", downloaded, total);
        }
    });

    let archive_path = release_repo
        .download_release_from_s3(
            &ant_releases::ReleaseType::AntNode,
            &version,
            &platform,
            &archive_type,
            &temp_download_dir_path,
            &callback,
        )
        .await?;
    info!("Download complete. Extracting archive...");

    let extracted_path =
        release_repo.extract_release_archive(&archive_path, &temp_download_dir_path)?;
    if !verify_binary_hash(&extracted_path, &antnode_binary.sha256)? {
        return Err(eyre!(
            "Downloaded binary hash does not match expected hash {}",
            antnode_binary.sha256
        ));
    }
    info!("Binary hash verified successfully");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&extracted_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&extracted_path, perms)?;
    }
    fs::rename(&extracted_path, &target_path)?;

    let _ = fs::remove_dir_all(&temp_download_dir_path);
    info!("Upgrade binary prepared at {}", target_path.display());
    Ok((target_path, antnode_binary.sha256.clone()))
}

pub fn replace_current_binary(new_binary_path: &Path, expected_hash: &str) -> Result<()> {
    #[cfg(unix)]
    {
        info!("Starting in-place binary replacement");

        if !verify_binary_hash(new_binary_path, expected_hash)? {
            return Err(eyre!("New binary hash verification failed"));
        }

        let mut current_exe_path = std::env::current_exe()?;
        let current_exe_str = current_exe_path.to_string_lossy();
        if current_exe_str.ends_with(" (deleted)") {
            let cleaned = current_exe_str.trim_end_matches(" (deleted)");
            current_exe_path = PathBuf::from(cleaned);
        }
        info!("Current executable: {}", current_exe_path.display());

        // In this case another process has already upgraded the binary, so the rename that occurs
        // below doesn't need to happen again.
        if current_exe_path.exists()
            && let Ok(current_hash) = calculate_sha256(&current_exe_path)
            && current_hash == expected_hash
        {
            info!("Current binary already matches upgrade hash");
            return Ok(());
        }

        // The reason for this copy is because you cannot *copy* over a running binary, only
        // rename/move. If we didn't do the copy, it would move the cached binary and other
        // processes would need to download it again.
        let temp_path = current_exe_path.with_extension(format!("tmp-{}", std::process::id()));
        fs::copy(new_binary_path, &temp_path)?;

        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&temp_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&temp_path, perms)?;
        fs::rename(&temp_path, &current_exe_path)?;

        info!("Replaced binary at {}", current_exe_path.display());
        Ok(())
    }

    #[cfg(not(unix))]
    {
        Err(eyre!(
            "Automatic upgrade is only supported on Unix platforms (Linux/macOS)"
        ))
    }
}

pub async fn perform_upgrade() -> Result<()> {
    info!("Performing upgrade check...");
    let release_repo = <dyn AntReleaseRepoActions>::default_config();
    let release_info = release_repo.get_latest_autonomi_release_info().await?;
    if release_info
        .commit_hash
        .starts_with(ant_build_info::git_sha())
    {
        return Err(eyre!("Already running latest version"));
    }
    info!("New version detected: {}", release_info.commit_hash);

    let (new_binary_path, expected_hash) =
        download_and_extract_upgrade_binary(&release_info, release_repo.as_ref()).await?;
    replace_current_binary(&new_binary_path, &expected_hash)?;

    Ok(())
}
