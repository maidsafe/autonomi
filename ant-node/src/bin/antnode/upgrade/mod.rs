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
use ant_releases::{
    AntReleaseRepoActions, ArchiveType, AutonomiReleaseInfo, Platform, ReleaseType,
};
use fs2::FileExt;
use once_cell::sync::OnceCell;
use semver::Version;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};

const LOCK_TIMEOUT_SECS: u64 = 300; // 5 minutes
const LOCK_RETRY_INTERVAL_MS: u64 = 100;
const DEFAULT_NETWORK_SIZE: usize = 100_000;

/// Cached SHA256 hash of the running binary.
///
/// The running binary's hash is constant for the process lifetime (it's loaded in memory),
/// so we calculate it once and cache it. This also avoids issues with the " (deleted)" suffix
/// that Linux appends to `/proc/self/exe` when the on-disk binary is replaced by another process.
static RUNNING_BINARY_HASH: OnceCell<String> = OnceCell::new();

/// Get the SHA256 hash of the currently running binary.
///
/// The hash is calculated once on first call and cached for subsequent calls.
/// Handles the " (deleted)" suffix that Linux appends when the binary has been replaced.
///
/// This function should be called early in the process lifetime (at startup) to ensure
/// the hash reflects the actual running binary, not a replacement that another process
/// may have installed.
pub fn get_running_binary_hash() -> Result<&'static str> {
    RUNNING_BINARY_HASH
        .get_or_try_init(|| {
            let mut current_exe_path = std::env::current_exe()?;
            let current_exe_str = current_exe_path.to_string_lossy();
            if current_exe_str.ends_with(" (deleted)") {
                let cleaned = current_exe_str.trim_end_matches(" (deleted)");
                current_exe_path = PathBuf::from(cleaned);
            }
            debug!(
                "Calculating and caching hash for running binary: {}",
                current_exe_path.display()
            );
            calculate_sha256(&current_exe_path)
        })
        .map(|s| s.as_str())
}

/// Get the autonomi.com download URL for a given platform.
///
/// These URLs redirect to the actual binary download location.
fn get_autonomi_download_url(platform: &Platform) -> &'static str {
    match platform {
        Platform::LinuxMusl => "https://downloads.autonomi.com/node/linux-x64",
        Platform::LinuxMuslAarch64 => "https://downloads.autonomi.com/node/linux-arm64",
        Platform::LinuxMuslArm => "https://downloads.autonomi.com/node/linux-arm",
        Platform::LinuxMuslArmV7 => "https://downloads.autonomi.com/node/linux-armv7",
        Platform::MacOs => "https://downloads.autonomi.com/node/mac-intel",
        Platform::MacOsAarch64 => "https://downloads.autonomi.com/node/mac-apple-silicon",
        Platform::Windows => "https://downloads.autonomi.com/node/windows",
    }
}

/// Download a file from a URL that may redirect to the actual binary.
///
/// Returns the path to the downloaded archive file.
async fn download_from_autonomi_url(
    platform: &Platform,
    dest_dir: &Path,
    callback: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<PathBuf> {
    let url = get_autonomi_download_url(platform);
    info!("Downloading from autonomi.com URL: {}", url);

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| UpgradeError::DownloadFailed(format!("Failed to create HTTP client: {e}")))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| UpgradeError::DownloadFailed(format!("HTTP request failed: {e}")))?;

    if !response.status().is_success() {
        return Err(UpgradeError::DownloadFailed(format!(
            "HTTP request failed with status: {}",
            response.status()
        )));
    }

    // Get the final URL after redirects to extract the filename
    let final_url = response.url().to_string();
    debug!("Final download URL after redirects: {}", final_url);

    // Extract filename from the final URL
    let filename = final_url
        .split('/')
        .next_back()
        .ok_or_else(|| {
            UpgradeError::DownloadFailed("Could not extract filename from URL".to_string())
        })?
        .to_string();

    let total_size = response
        .headers()
        .get("content-length")
        .and_then(|ct_len| ct_len.to_str().ok())
        .and_then(|ct_len| ct_len.parse::<u64>().ok())
        .unwrap_or(0);

    let dest_path = dest_dir.join(&filename);
    let mut out_file = tokio::fs::File::create(&dest_path)
        .await
        .map_err(UpgradeError::Io)?;

    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();

    use futures::StreamExt;
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result
            .map_err(|e| UpgradeError::DownloadFailed(format!("Failed to read chunk: {e}")))?;
        downloaded += chunk.len() as u64;
        out_file.write_all(&chunk).await.map_err(UpgradeError::Io)?;
        callback(downloaded, total_size);
    }

    out_file.flush().await.map_err(UpgradeError::Io)?;
    info!("Downloaded {} bytes to {}", downloaded, dest_path.display());

    Ok(dest_path)
}

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
        .ok_or_else(|| {
            UpgradeError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine user data directory",
            ))
        })?
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
/// Downloads from autonomi.com URLs which redirect to the actual binary location.
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

    let callback: Box<dyn Fn(u64, u64) + Send + Sync> = Box::new(|downloaded, total| {
        if total > 0 && downloaded % (total / 10).max(1) == 0 {
            debug!("Download progress: {}/{} bytes", downloaded, total);
        }
    });

    // Try downloading from autonomi.com first, fall back to S3 if it fails
    let archive_path =
        match download_from_autonomi_url(&platform, &temp_download_dir_path, &callback).await {
            Ok(path) => {
                info!("Successfully downloaded from autonomi.com");
                path
            }
            Err(e) => {
                warn!(
                    "Failed to download from autonomi.com: {}. Falling back to S3...",
                    e
                );
                let version = Version::parse(&antnode_binary.version)?;
                let archive_type = ArchiveType::TarGz;
                release_repo
                    .download_release_from_s3(
                        &ReleaseType::AntNode,
                        &version,
                        &platform,
                        &archive_type,
                        &temp_download_dir_path,
                        &callback,
                    )
                    .await
                    .map_err(|e| {
                        UpgradeError::DownloadFailed(format!("S3 fallback download failed: {e}"))
                    })?
            }
        };
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
            "Automatic upgrade is only supported on Unix platforms (Linux/macOS)".to_string(),
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

    // Check if the antnode binary has changed by comparing SHA256 hashes.
    let platform = ant_releases::get_running_platform()?;
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

    let current_hash = get_running_binary_hash()?;
    if current_hash.eq_ignore_ascii_case(&antnode_binary.sha256) {
        info!("Current antnode binary hash matches latest release. No upgrade needed.");
        return Err(UpgradeError::AlreadyLatest);
    }

    info!(
        "New antnode binary available (current: {}; latest: {}). Proceeding with upgrade...",
        &current_hash[..8],
        &antnode_binary.sha256[..8]
    );

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
        hash_result[0],
        hash_result[1],
        hash_result[2],
        hash_result[3],
        hash_result[4],
        hash_result[5],
        hash_result[6],
        hash_result[7],
    ]);

    let time_range_hours = std::cmp::min(72, (est_network_size / DEFAULT_NETWORK_SIZE) + 1);
    let time_range_seconds = time_range_hours * 3600;
    let upgrade_time_seconds = (hash_value as usize) % time_range_seconds;

    Duration::from_secs(upgrade_time_seconds as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ant_releases::AntReleaseRepoActions;
    use tempfile::TempDir;

    /// Test downloading and extracting binaries from all platform URLs
    #[tokio::test]
    async fn test_download_and_extract_all_platforms() {
        let platforms = [
            Platform::LinuxMusl,
            Platform::LinuxMuslAarch64,
            Platform::LinuxMuslArm,
            Platform::LinuxMuslArmV7,
            Platform::MacOs,
            Platform::MacOsAarch64,
            Platform::Windows,
        ];

        for platform in platforms {
            println!("\n=== Testing platform: {platform:?} ===");

            let temp_dir = TempDir::new().expect("Failed to create temp dir");
            let dest_dir = temp_dir.path();

            // Use a no-op callback for tests
            let callback: Box<dyn Fn(u64, u64) + Send + Sync> = Box::new(|_, _| {});

            // Use our download function directly
            let archive_path = download_from_autonomi_url(&platform, dest_dir, &callback)
                .await
                .unwrap_or_else(|_| panic!("Failed to download for platform {platform:?}"));

            // Verify the archive file exists
            assert!(
                archive_path.exists(),
                "Archive file does not exist: {}",
                archive_path.display()
            );

            // Verify filename has valid archive extension
            let filename = archive_path
                .file_name()
                .and_then(|n| n.to_str())
                .expect("Could not get filename");
            assert!(
                filename.ends_with(".zip") || filename.ends_with(".tar.gz"),
                "Invalid archive extension for {platform:?}: {filename}"
            );

            // Verify file was downloaded and has reasonable size (at least 1MB for a binary)
            let metadata = std::fs::metadata(&archive_path).expect("Failed to get file metadata");
            let file_len = metadata.len();
            assert!(
                file_len > 1_000_000,
                "Downloaded file for {platform:?} is too small: {file_len} bytes"
            );

            println!(
                "Platform: {platform:?} - Downloaded {file_len} bytes to {}",
                archive_path.display()
            );

            // Test extraction using release_repo
            let release_repo = <dyn AntReleaseRepoActions>::default_config();
            let extracted_path = release_repo
                .extract_release_archive(&archive_path, dest_dir)
                .unwrap_or_else(|_| panic!("Failed to extract archive for {platform:?}"));

            // Verify extracted file exists
            assert!(
                extracted_path.exists(),
                "Extracted file does not exist for {platform:?}: {}",
                extracted_path.display()
            );

            // Verify extracted file has reasonable size
            let extracted_metadata =
                std::fs::metadata(&extracted_path).expect("Failed to get extracted file metadata");
            let extracted_len = extracted_metadata.len();
            assert!(
                extracted_len > 1_000_000,
                "Extracted file for {platform:?} is too small: {extracted_len} bytes"
            );

            println!(
                "Platform: {platform:?} - Extracted to {} ({extracted_len} bytes)",
                extracted_path.display()
            );

            // On Unix, verify the file is executable or can be made executable
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = extracted_metadata.permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&extracted_path, perms)
                    .expect("Failed to set executable permissions");
            }

            println!("Platform: {platform:?} - SUCCESS");
        }
    }
}
