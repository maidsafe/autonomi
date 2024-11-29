// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::path::{Path, PathBuf};
use std::time::Duration;
use std::fs;

/// Configuration for the bootstrap cache
#[derive(Clone, Debug)]
pub struct BootstrapConfig {
    /// List of bootstrap endpoints to fetch peer information from
    pub endpoints: Vec<String>,
    /// Maximum number of peers to keep in the cache
    pub max_peers: usize,
    /// Path to the bootstrap cache file
    pub cache_file_path: PathBuf,
    /// How often to update the cache (in seconds)
    pub update_interval: Duration,
    /// Request timeout for endpoint queries
    pub request_timeout: Duration,
    /// Maximum retries per endpoint
    pub max_retries: u32,
}

impl Default for BootstrapConfig {
    fn default() -> Self {
        Self {
            endpoints: vec![
                "https://sn-testnet.s3.eu-west-2.amazonaws.com/bootstrap_cache.json".to_string(),
                "https://sn-testnet.s3.eu-west-2.amazonaws.com/network-contacts".to_string(),
                "https://sn-node1.s3.eu-west-2.amazonaws.com/peers".to_string(),
                "https://sn-node2.s3.eu-west-2.amazonaws.com/peers".to_string(),
            ],
            max_peers: 1500,
            cache_file_path: default_cache_path(),
            update_interval: Duration::from_secs(60),
            request_timeout: Duration::from_secs(10),
            max_retries: 3,
        }
    }
}

impl BootstrapConfig {
    /// Creates a new BootstrapConfig with custom endpoints
    pub fn with_endpoints(endpoints: Vec<String>) -> Self {
        Self {
            endpoints,
            ..Default::default()
        }
    }

    /// Creates a new BootstrapConfig with a custom cache file path
    pub fn with_cache_path<P: AsRef<Path>>(path: P) -> Self {
        Self {
            cache_file_path: path.as_ref().to_path_buf(),
            ..Default::default()
        }
    }

    /// Creates a new BootstrapConfig with custom settings
    pub fn new(
        endpoints: Vec<String>,
        max_peers: usize,
        cache_file_path: PathBuf,
        update_interval: Duration,
        request_timeout: Duration,
        max_retries: u32,
    ) -> Self {
        Self {
            endpoints,
            max_peers,
            cache_file_path,
            update_interval,
            request_timeout,
            max_retries,
        }
    }
}

/// Returns the default path for the bootstrap cache file
fn default_cache_path() -> PathBuf {
    tracing::info!("Determining default cache path");
    let system_path = if cfg!(target_os = "macos") {
        tracing::debug!("OS: macOS");
        // Try user's Library first, then fall back to system Library
        if let Some(home) = dirs::home_dir() {
            let user_library = home.join("Library/Application Support/Safe/bootstrap_cache.json");
            tracing::info!("Attempting to use user's Library path: {:?}", user_library);
            if let Some(parent) = user_library.parent() {
                tracing::debug!("Creating directory: {:?}", parent);
                match fs::create_dir_all(parent) {
                    Ok(_) => {
                        tracing::debug!("Successfully created directory structure");
                        // Check if we can write to the directory
                        match tempfile::NamedTempFile::new_in(parent) {
                            Ok(temp_file) => {
                                temp_file.close().ok();
                                tracing::info!("Successfully verified write access to {:?}", parent);
                                return user_library;
                            }
                            Err(e) => {
                                tracing::warn!("Cannot write to user's Library: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to create user's Library directory: {}", e);
                    }
                }
            }
        }
        // Fall back to system Library
        tracing::info!("Falling back to system Library path");
        PathBuf::from("/Library/Application Support/Safe/bootstrap_cache.json")
    } else if cfg!(target_os = "linux") {
        tracing::debug!("OS: Linux");
        // On Linux, try /var/lib/safe first, then fall back to /var/safe
        let primary_path = PathBuf::from("/var/lib/safe/bootstrap_cache.json");
        tracing::info!("Attempting to use primary Linux path: {:?}", primary_path);
        if let Some(parent) = primary_path.parent() {
            tracing::debug!("Creating directory: {:?}", parent);
            match fs::create_dir_all(parent) {
                Ok(_) => {
                    tracing::debug!("Successfully created directory structure");
                    // Check if we can write to the directory
                    match tempfile::NamedTempFile::new_in(parent) {
                        Ok(temp_file) => {
                            temp_file.close().ok();
                            tracing::info!("Successfully verified write access to {:?}", parent);
                            return primary_path;
                        }
                        Err(e) => {
                            tracing::warn!("Cannot write to {:?}: {}", parent, e);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to create Linux primary directory: {}", e);
                }
            }
        }
        tracing::info!("Falling back to secondary Linux path: /var/safe");
        PathBuf::from("/var/safe/bootstrap_cache.json")
    } else if cfg!(target_os = "windows") {
        tracing::debug!("OS: Windows");
        // On Windows, try LocalAppData first, then fall back to ProgramData
        if let Some(local_app_data) = dirs::data_local_dir() {
            let local_path = local_app_data.join("Safe").join("bootstrap_cache.json");
            tracing::info!("Attempting to use Windows LocalAppData path: {:?}", local_path);
            if let Some(parent) = local_path.parent() {
                tracing::debug!("Creating directory: {:?}", parent);
                if fs::create_dir_all(parent).is_ok() {
                    // Check if we can write to the directory
                    if let Ok(temp_file) = tempfile::NamedTempFile::new_in(parent) {
                        temp_file.close().ok();
                        tracing::info!("Successfully created and verified Windows LocalAppData path");
                        return local_path;
                    }
                }
            }
        }
        tracing::info!("Falling back to Windows ProgramData path");
        PathBuf::from(r"C:\ProgramData\Safe\bootstrap_cache.json")
    } else {
        tracing::debug!("Unknown OS, using current directory");
        PathBuf::from("bootstrap_cache.json")
    };

    // Try to create the system directory first
    if let Some(parent) = system_path.parent() {
        tracing::debug!("Attempting to create system directory: {:?}", parent);
        if fs::create_dir_all(parent).is_ok() {
            // Check if we can write to the directory
            match tempfile::NamedTempFile::new_in(parent) {
                Ok(temp_file) => {
                    temp_file.close().ok();
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        match fs::set_permissions(parent, fs::Permissions::from_mode(0o755)) {
                            Ok(_) => tracing::debug!("Successfully set directory permissions"),
                            Err(e) => tracing::warn!("Failed to set cache directory permissions: {}", e),
                        }
                    }
                    tracing::info!("Successfully created and verified system directory");
                    return system_path;
                }
                Err(e) => {
                    tracing::warn!("Cannot write to system directory: {}", e);
                }
            }
        } else {
            tracing::warn!("Failed to create system directory");
        }
    }

    // If system directory is not writable, fall back to user's home directory
    if let Some(home) = dirs::home_dir() {
        let user_path = home.join(".safe").join("bootstrap_cache.json");
        tracing::info!("Attempting to use home directory fallback: {:?}", user_path);
        if let Some(parent) = user_path.parent() {
            tracing::debug!("Creating home directory: {:?}", parent);
            if fs::create_dir_all(parent).is_ok() {
                tracing::info!("Successfully created home directory");
                return user_path;
            }
        }
    }

    // Last resort: use current directory
    tracing::warn!("All directory attempts failed, using current directory");
    PathBuf::from("bootstrap_cache.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_default_config() {
        let config = BootstrapConfig::default();
        assert_eq!(config.endpoints.len(), 4);
        assert_eq!(
            config.endpoints[0],
            "https://sn-testnet.s3.eu-west-2.amazonaws.com/bootstrap_cache.json"
        );
        assert_eq!(
            config.endpoints[1],
            "https://sn-testnet.s3.eu-west-2.amazonaws.com/network-contacts"
        );
        assert_eq!(
            config.endpoints[2],
            "https://sn-node1.s3.eu-west-2.amazonaws.com/peers"
        );
        assert_eq!(
            config.endpoints[3],
            "https://sn-node2.s3.eu-west-2.amazonaws.com/peers"
        );
        assert_eq!(config.max_peers, 1500);
        assert_eq!(config.update_interval, Duration::from_secs(60));
        assert_eq!(config.request_timeout, Duration::from_secs(10));
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn test_custom_endpoints() {
        let endpoints = vec!["http://custom.endpoint/cache".to_string()];
        let config = BootstrapConfig::with_endpoints(endpoints.clone());
        assert_eq!(config.endpoints, endpoints);
    }

    #[test]
    fn test_custom_cache_path() {
        let path = PathBuf::from("/custom/path/cache.json");
        let config = BootstrapConfig::with_cache_path(&path);
        assert_eq!(config.cache_file_path, path);
    }

    #[test]
    fn test_new_config() {
        let endpoints = vec!["http://custom.endpoint/cache".to_string()];
        let path = PathBuf::from("/custom/path/cache.json");
        let config = BootstrapConfig::new(
            endpoints.clone(),
            2000,
            path.clone(),
            Duration::from_secs(120),
            Duration::from_secs(5),
            5,
        );

        assert_eq!(config.endpoints, endpoints);
        assert_eq!(config.max_peers, 2000);
        assert_eq!(config.cache_file_path, path);
        assert_eq!(config.update_interval, Duration::from_secs(120));
        assert_eq!(config.request_timeout, Duration::from_secs(5));
        assert_eq!(config.max_retries, 5);
    }
}
