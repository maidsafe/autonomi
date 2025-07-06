// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.
use vergen::EmitBuilder;

mod release_info {
    include!("src/release_info.rs");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    EmitBuilder::builder()
        .build_date()
        .git_sha(true)
        .git_branch()
        .git_describe(true, false, None)
        .emit()?;

    println!(
        "cargo:rustc-env=RELEASE_YEAR={}",
        release_info::RELEASE_YEAR
    );
    println!(
        "cargo:rustc-env=RELEASE_MONTH={}",
        release_info::RELEASE_MONTH
    );
    println!(
        "cargo:rustc-env=RELEASE_CYCLE={}",
        release_info::RELEASE_CYCLE
    );
    println!(
        "cargo:rustc-env=RELEASE_CYCLE_COUNTER={}",
        release_info::RELEASE_CYCLE_COUNTER
    );

    Ok(())
}
