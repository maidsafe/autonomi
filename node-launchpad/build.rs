// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Emit build information
    vergen::EmitBuilder::builder()
        .all_build()
        .all_git()
        .emit()?;

    // Embed Windows icon resource when building for Windows
    // Note: We check CARGO_CFG_TARGET_OS because build.rs runs on the host,
    // so #[cfg(target_os = "windows")] would check the host OS, not the target.
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../resources/icons/windows/node-launchpad/node-launchpad.ico");
        res.set("ProductName", "Node Launchpad");
        res.set(
            "FileDescription",
            "TUI for running nodes on the Autonomi network",
        );
        res.compile()?;
    }

    Ok(())
}
