// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod error;
mod release_cache;

pub use error::{Result, UpgradeError};

use ant_node::RunningNode;
use ant_releases::{AntReleaseRepoActions, AutonomiReleaseInfo};
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
const DEFAULT_NETWORK_SIZE: usize = 100_000;

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
        .ok_or_else(|| UpgradeError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Could not determine user data directory"
        )))?
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

/// Acquire an exclusive lock on a file.
///
/// Returns the lock file handle that must be kept alive to maintain the lock.
/// If the lock cannot be acquired within LOCK_TIMEOUT_SECS, a warning is logged
/// and the file handle is returned anyway.
pub fn acquire_exclusive_lock(lock_path: &Path, lock_name: &str) -> Result<File> {
    debug!("Acquiring {} at: {}", lock_name, lock_path.display());

    let lock_file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)?;
    let start = std::time::Instant::now();
    loop {
        match lock_file.try_lock_exclusive() {
            Ok(_) => {
                info!("{} acquired", lock_name);
                return Ok(lock_file);
            }
            Err(_) if start.elapsed().as_secs() < LOCK_TIMEOUT_SECS => {
                debug!("{} busy. Retrying...", lock_name);
                std::thread::sleep(Duration::from_millis(LOCK_RETRY_INTERVAL_MS));
            }
            Err(e) => {
                warn!(
                    "{} timeout after {} seconds: {}. Proceeding without lock...",
                    lock_name, LOCK_TIMEOUT_SECS, e
                );
                // Proceed without exclusive lock: verification after operations will catch any
                // corruption if multiple processes race.
                return Ok(lock_file);
            }
        }
    }
}

/// Acquire an exclusive lock on the upgrade directory.
///
/// It's possible two or more processes could try to download and extract a new binary at the same
/// time.
///
/// Returns the lock file handle that must be kept alive to maintain the lock.
fn acquire_upgrade_lock(upgrade_dir: &Path) -> Result<File> {
    let lock_path = upgrade_dir.join(".lock");
    acquire_exclusive_lock(&lock_path, "Upgrade lock")
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
        .ok_or_else(|| UpgradeError::PlatformBinariesNotFound(format!("{platform:?}")))?;
    let antnode_binary = platform_binaries
        .binaries
        .iter()
        .find(|b| b.name == "antnode")
        .ok_or_else(|| {
            UpgradeError::PlatformBinariesNotFound(format!(
                "antnode binary not found in release for platform: {platform:?}"
            ))
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
        return Err(UpgradeError::HashVerificationFailed);
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
            return Err(UpgradeError::HashVerificationFailed);
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
        Err(UpgradeError::BinaryReplacementFailed(
            "Automatic upgrade is only supported on Unix platforms (Linux/macOS)".to_string()
        ))
    }
}

pub async fn perform_upgrade() -> Result<()> {
    info!("Performing upgrade check...");

    let release_info = match release_cache::read_cached_release_info(None) {
        Ok(Some(cached)) => {
            info!("Using cached release info");
            cached.release_info
        }
        Ok(None) => {
            info!("No valid cache, fetching from API...");
            fetch_and_cache_release_info().await?
        }
        Err(e) => {
            warn!("Error reading cache ({}), fetching from API...", e);
            fetch_and_cache_release_info().await?
        }
    };

    if release_info
        .commit_hash
        .starts_with(ant_build_info::git_sha())
    {
        return Err(UpgradeError::AlreadyLatest);
    }
    info!("New version detected: {}", release_info.commit_hash);

    let release_repo = <dyn AntReleaseRepoActions>::default_config();
    let (new_binary_path, expected_hash) =
        download_and_extract_upgrade_binary(&release_info, release_repo.as_ref()).await?;
    replace_current_binary(&new_binary_path, &expected_hash)?;

    Ok(())
}

/// Fetch release info from API and cache it
async fn fetch_and_cache_release_info() -> Result<AutonomiReleaseInfo> {
    let release_repo = <dyn AntReleaseRepoActions>::default_config();
    let release_info = release_repo.get_latest_autonomi_release_info().await?;

    if let Err(e) = release_cache::write_cached_release_info(&release_info, None) {
        warn!("Failed to cache release info: {}", e);
    }

    Ok(release_info)
}

/// Calculates a deterministic restart delay based on peer ID and network size.
///
/// This algorithm ensures that node upgrades are staggered across the network to prevent
/// many nodes from restarting simultaneously.
///
/// The delay is calculated as follows:
/// 1. Hash the node's peer ID using SHA256 to get a deterministic value unique to this node
/// 2. Calculate a time range in hours based on network size: min(72, network_size/100_000 + 1)
///    - Smaller networks (< 100k nodes) get 1-2 hour windows
///    - Larger networks get progressively longer windows, capped at 72 hours (3 days)
/// 3. Use the hash modulo the time range to assign this node a specific delay
///
/// This approach provides:
/// - Deterministic delays: Same peer ID always gets the same delay for a given network size
/// - Even distribution: Hash modulo spreads nodes uniformly across the time window
/// - Network-aware scaling: Larger networks get longer upgrade windows
///
/// # Arguments
/// * `running_node` - The node to calculate the restart delay for
///
/// # Returns
/// A `Duration` representing when this node should restart for an upgrade
pub async fn calculate_restart_delay(running_node: &RunningNode) -> Duration {
    let peer_id = running_node.peer_id();
    let est_network_size = running_node
        .get_estimated_network_size()
        .await
        .unwrap_or(DEFAULT_NETWORK_SIZE);

    let mut hasher = Sha256::new();
    hasher.update(peer_id.to_bytes());
    let hash_result = hasher.finalize();

    // Convert first 8 bytes of hash to u64 for modulo operation
    let hash_value = u64::from_be_bytes([
        hash_result[0], hash_result[1], hash_result[2], hash_result[3],
        hash_result[4], hash_result[5], hash_result[6], hash_result[7],
    ]);

    let time_range_hours = std::cmp::min(72, (est_network_size / DEFAULT_NETWORK_SIZE) + 1);
    let time_range_seconds = time_range_hours * 3600;
    let upgrade_time_seconds = (hash_value as usize) % time_range_seconds;

    Duration::from_secs(upgrade_time_seconds as u64)
}
