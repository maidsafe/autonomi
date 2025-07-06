// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_service_management::StatusSummary;
use assert_cmd::{assert::OutputAssertExt, cargo::CommandCargoExt};
use color_eyre::{eyre::eyre, Result};
use std::process::Command;

pub async fn get_service_status() -> Result<StatusSummary> {
    let mut cmd = Command::cargo_bin("antctl")?;
    let output = cmd
        .arg("status")
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let output = std::str::from_utf8(&output)?;
    println!("status command output:");
    println!("{output}");

    let status: StatusSummary = match serde_json::from_str(output) {
        Ok(json) => json,
        Err(e) => return Err(eyre!("Failed to parse JSON output: {:?}", e)),
    };
    Ok(status)
}
