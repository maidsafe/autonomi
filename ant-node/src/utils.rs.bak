// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_protocol::node::get_antnode_root_dir;
use eyre::eyre;
use libp2p::identity::Keypair;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The keypair is located inside the root directory. At the same time, when no dir is specified,
/// the dir name is derived from the keypair used in the application: the peer ID is used as the directory name.
pub fn get_root_dir_and_keypair(root_dir: &Option<PathBuf>) -> eyre::Result<(PathBuf, Keypair)> {
    match root_dir {
        Some(dir) => {
            std::fs::create_dir_all(dir)?;

            let secret_key_path = dir.join("secret-key");
            Ok((dir.clone(), keypair_from_path(secret_key_path)?))
        }
        None => {
            let secret_key = libp2p::identity::ed25519::SecretKey::generate();
            let keypair: Keypair =
                libp2p::identity::ed25519::Keypair::from(secret_key.clone()).into();
            let peer_id = keypair.public().to_peer_id();

            let dir = get_antnode_root_dir(peer_id)?;
            std::fs::create_dir_all(&dir)?;

            let secret_key_path = dir.join("secret-key");

            let mut file = create_secret_key_file(secret_key_path)
                .map_err(|err| eyre!("could not create secret key file: {err}"))?;
            file.write_all(secret_key.as_ref())?;

            Ok((dir, keypair))
        }
    }
}

fn keypair_from_path(path: impl AsRef<Path>) -> eyre::Result<Keypair> {
    let keypair = match std::fs::read(&path) {
        // If the file is opened successfully, read the key from it
        Ok(key) => {
            let keypair = Keypair::ed25519_from_bytes(key)
                .map_err(|err| eyre!("could not read ed25519 key from file: {err}"))?;

            info!("loaded secret key from file: {:?}", path.as_ref());

            keypair
        }
        // In case the file is not found, generate a new keypair and write it to the file
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let secret_key = libp2p::identity::ed25519::SecretKey::generate();
            let mut file = create_secret_key_file(&path)
                .map_err(|err| eyre!("could not create secret key file: {err}"))?;
            file.write_all(secret_key.as_ref())?;

            info!("generated new key and stored to file: {:?}", path.as_ref());

            libp2p::identity::ed25519::Keypair::from(secret_key).into()
        }
        // Else the file can't be opened, for whatever reason (e.g. permissions).
        Err(err) => {
            return Err(eyre!("failed to read secret key file: {err}"));
        }
    };

    Ok(keypair)
}

fn create_secret_key_file(path: impl AsRef<Path>) -> eyre::Result<std::fs::File, std::io::Error> {
    let mut opt = std::fs::OpenOptions::new();
    let _ = opt.write(true).create_new(true);

    // On Unix systems, make sure only the current user can read/write.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let _ = opt.mode(0o600);
    }

    opt.open(path)
}
