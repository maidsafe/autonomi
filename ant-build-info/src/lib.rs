// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use chrono::Utc;
use serde::{Deserialize, Serialize};

#[cfg(feature = "nightly")]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VersionInfo {
    pub app_name: String,
    pub nightly_version: String,
    pub network_version: Option<String>,
    pub git_branch: String,
    pub git_sha: String,
}

#[cfg(not(feature = "nightly"))]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VersionInfo {
    pub app_name: String,
    pub crate_version: String,
    pub network_version: Option<String>,
    pub package_info: String,
    pub git_branch: String,
    pub git_sha: String,
    pub build_date: String,
}

impl VersionInfo {
    pub fn pretty_print(&self) {
        println!("{self}");
    }
}

#[cfg(not(feature = "nightly"))]
pub fn get_version_info(
    app_name: &str,
    crate_version: &str,
    protocol_version: Option<&str>,
) -> VersionInfo {
    VersionInfo {
        app_name: app_name.to_string(),
        crate_version: crate_version.to_string(),
        network_version: protocol_version.map(|version| version.to_string()),
        package_info: package_version(),
        git_branch: git_branch().to_string(),
        git_sha: git_sha().to_string(),
        build_date: build_date().to_string(),
    }
}

#[cfg(feature = "nightly")]
pub fn get_version_info(
    app_name: &str,
    _crate_version: &str,
    protocol_version: Option<&str>,
) -> VersionInfo {
    VersionInfo {
        app_name: app_name.to_string(),
        nightly_version: nightly_version(),
        network_version: protocol_version.map(|version| version.to_string()),
        git_branch: git_branch().to_string(),
        git_sha: git_sha().to_string(),
    }
}

#[cfg(not(feature = "nightly"))]
impl std::fmt::Display for VersionInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} v{}", self.app_name, self.crate_version)?;
        if let Some(version) = &self.network_version {
            writeln!(f, "Network version: {version}")?;
        }
        writeln!(f, "Package version: {}", self.package_info)?;
        writeln!(
            f,
            "\nGit info: {} / {} / {}",
            self.git_branch, self.git_sha, self.build_date
        )
    }
}

#[cfg(feature = "nightly")]
impl std::fmt::Display for VersionInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} -- Nightly Release {}",
            self.app_name, self.nightly_version
        )?;
        if let Some(version) = &self.network_version {
            writeln!(f, "Network version: {version}")?;
        }
        writeln!(f, "Git info: {} / {}", self.git_branch, self.git_sha)
    }
}

/// Git information separated by slashes: `<sha> / <branch> / <describe>`
pub const fn git_info() -> &'static str {
    concat!(
        env!("VERGEN_GIT_BRANCH"),
        " / ",
        env!("VERGEN_GIT_SHA"),
        " / ",
        env!("VERGEN_BUILD_DATE")
    )
}

/// Annotated tag description, or fall back to abbreviated commit object.
pub const fn git_describe() -> &'static str {
    env!("VERGEN_GIT_DESCRIBE")
}

/// The current git branch.
pub const fn git_branch() -> &'static str {
    env!("VERGEN_GIT_BRANCH")
}

/// Shortened SHA-1 hash.
pub const fn git_sha() -> &'static str {
    env!("VERGEN_GIT_SHA")
}

/// Nightly version format: YYYY.MM.DD
pub fn nightly_version() -> String {
    let now = Utc::now();
    now.format("%Y.%m.%d").to_string()
}

pub fn package_version() -> String {
    format!(
        "{}.{}.{}.{}",
        env!("RELEASE_YEAR"),
        env!("RELEASE_MONTH"),
        env!("RELEASE_CYCLE"),
        env!("RELEASE_CYCLE_COUNTER")
    )
}

pub fn build_date() -> String {
    env!("VERGEN_BUILD_DATE").to_string()
}
