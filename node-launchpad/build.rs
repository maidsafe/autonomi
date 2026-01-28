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

    // Embed Windows icon resource when building ON Windows FOR Windows.
    // This uses compile-time cfg because winresource is only available as a
    // build dependency when the host is Windows.
    #[cfg(windows)]
    {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
            .expect("CARGO_MANIFEST_DIR should be set by cargo");
        let icon_path = std::path::Path::new(&manifest_dir)
            .join("../resources/icons/windows/node-launchpad/node-launchpad.ico");

        if !icon_path.exists() {
            panic!(
                "Icon file not found at: {}. Current dir: {:?}",
                icon_path.display(),
                std::env::current_dir()
            );
        }

        println!("cargo:rerun-if-changed={}", icon_path.display());

        let mut res = winresource::WindowsResource::new();
        res.set_icon(icon_path.to_str().expect("Icon path should be valid UTF-8"));
        res.set("ProductName", "Node Launchpad");
        res.set(
            "FileDescription",
            "TUI for running nodes on the Autonomi network",
        );
        res.compile()?;
    }

    Ok(())
}
