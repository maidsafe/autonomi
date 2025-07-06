// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use chrono::Utc;
use tracing::debug;

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

/// Git information for nightly builds: `<date> / <branch> / <sha>`
pub fn nightly_git_info() -> String {
    format!("{} / {} / {}", nightly_version(), git_branch(), git_sha(),)
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

pub fn full_version_info(
    app_name: &str,
    crate_version: &str,
    protocol_version: Option<&str>,
) -> String {
    let mut info = format!("{app_name} v{crate_version}");

    if let Some(version) = protocol_version {
        info.push_str(&format!("\nNetwork version: {version}"));
    }

    info.push_str(&format!(
        "\nPackage version: {}\nGit info: {}",
        package_version(),
        git_info()
    ));

    info
}

pub fn full_nightly_version_info(app_name: &str, protocol_version: Option<&str>) -> String {
    let mut info = format!("{app_name} -- Nightly Release {}", nightly_version(),);
    if let Some(version) = protocol_version {
        info.push_str(&format!("\nNetwork version: {version}"));
    }
    info.push_str(&format!("\nGit info: {} / {}", git_branch(), git_sha(),));
    info
}

pub fn version_string(
    app_name: &str,
    crate_version: &str,
    protocol_version: Option<&str>,
) -> String {
    if cfg!(feature = "nightly") {
        full_nightly_version_info(app_name, protocol_version)
    } else {
        full_version_info(app_name, crate_version, protocol_version)
    }
}

pub fn log_version_info(crate_version: &str, protocol_version: &str) {
    if cfg!(feature = "nightly") {
        debug!("nightly build info: {}", nightly_git_info());
        debug!("network version: {protocol_version}");
    } else {
        debug!("version: {crate_version}");
        debug!("network version: {protocol_version}");
        debug!("package version: {}", package_version());
        debug!("git info: {}", git_info());
    }
}
