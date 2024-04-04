// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use lazy_static::lazy_static;

/// Set this env variable to provide custom network versioning. If it is set to 'restricted', then the git branch name
/// is used as the version string. Else we directly use the passed in string as the version.
const NETWORK_VERSION_MODE_ENV_VARIABLE: &str = "NETWORK_VERSION_MODE";

lazy_static! {
    /// The node version used during Identify Behaviour.
    pub static ref IDENTIFY_NODE_VERSION_STR: String =
        format!(
            "safe{}/node/{}",
            write_network_version_with_slash(),
            get_truncate_version_str()
        );

    /// The client version used during Identify Behaviour.
    pub static ref IDENTIFY_CLIENT_VERSION_STR: String =
        format!(
            "safe{}/client/{}",
            write_network_version_with_slash(),
            get_truncate_version_str()
        );

    /// / first version for the req/response protocol
    pub static ref REQ_RESPONSE_VERSION_STR: String =
        format!(
            "/safe{}/node/{}",
            write_network_version_with_slash(),
            get_truncate_version_str()
        );


    /// The identify protocol version
    pub static ref IDENTIFY_PROTOCOL_STR: String =
        format!(
            "safe{}/{}",
            write_network_version_with_slash(),
            get_truncate_version_str()
        );


}

/// Get the network version string.
/// If the network version mode env variable is set to `restricted`, then the git branch is used as the version.
/// Else any non empty string is used as the version string.
/// If the env variable is empty or not set, then we do not apply any network versioning.
pub fn get_network_version() -> String {
    match std::env::var(NETWORK_VERSION_MODE_ENV_VARIABLE) {
        Ok(value) if !value.is_empty() => {
            if value == "restricted" {
                sn_build_info::git_branch().to_string()
            } else {
                value
            }
        }
        _ => "".to_string(),
    }
}

/// Helper to write the network version with `/` appended if it is not empty
fn write_network_version_with_slash() -> String {
    let version = get_network_version();
    if version.is_empty() {
        version
    } else {
        format!("/{version}")
    }
}

// Protocol support shall be downward compatible for patch only version update.
// i.e. versions of `A.B.X` shall be considered as a same protocol of `A.B`
fn get_truncate_version_str() -> &'static str {
    let version_str = env!("CARGO_PKG_VERSION");
    if version_str.matches('.').count() == 2 {
        match version_str.rfind('.') {
            Some(pos) => &version_str[..pos],
            None => version_str,
        }
    } else {
        version_str
    }
}
