// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use color_eyre::{
    eyre::{eyre, Context, Result},
    Section,
};
use std::path::PathBuf;

pub fn get_client_data_dir_path() -> Result<PathBuf> {
    let mut home_dirs = dirs_next::data_dir()
        .ok_or_else(|| eyre!("Failed to obtain data dir, your OS might not be supported."))?;
    home_dirs.push("autonomi");
    home_dirs.push("client");
    std::fs::create_dir_all(home_dirs.as_path())
        .wrap_err("Failed to create data dir")
        .with_suggestion(|| {
            format!(
                "make sure you have the correct permissions to access the data dir: {home_dirs:?}"
            )
        })?;
    Ok(home_dirs)
}
