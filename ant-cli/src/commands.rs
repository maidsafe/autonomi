// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod file;
mod vault;
mod wallet;

use crate::opt::Opt;
use clap::{error::ErrorKind, CommandFactory as _, Subcommand};
use color_eyre::Result;

#[derive(Subcommand, Debug)]
pub enum SubCmd {
    /// Operations related to file handling.
    File {
        #[command(subcommand)]
        command: FileCmd,
    },

    /// Operations related to vault management.
    Vault {
        #[command(subcommand)]
        command: VaultCmd,
    },

    /// Operations related to wallet management.
    Wallet {
        #[command(subcommand)]
        command: WalletCmd,
    },
}

#[derive(Subcommand, Debug)]
pub enum FileCmd {
    /// Estimate cost to upload a file.
    Cost {
        /// The file to estimate cost for.
        file: String,
    },

    /// Upload a file and pay for it. Data on the Network is private by default.
    Upload {
        /// The file to upload.
        file: String,
        /// Upload the file as public. Everyone can see public data on the Network.
        #[arg(short, long)]
        public: bool,
    },

    /// Download a file from the given address.
    Download {
        /// The address of the file to download.
        addr: String,
        /// The destination file path.
        dest_file: String,
    },

    /// List previous uploads
    List,
}

#[derive(Subcommand, Debug)]
pub enum VaultCmd {
    /// Estimate cost to create a vault.
    Cost {
        /// Expected max_size of a vault, only for cost estimation.
        #[clap(default_value = "3145728")]
        expected_max_size: u64,
    },

    /// Create a vault at a deterministic address based on your `SECRET_KEY`.
    /// Pushing an encrypted backup of your local user data to the network
    Create,

    /// Load an existing vault from the network.
    /// Use this when loading your user data to a new device.
    /// You need to have your original `SECRET_KEY` to load the vault.
    Load,

    /// Sync vault with the network, safeguarding local user data.
    /// Loads existing user data from the network and merges it with your local user data.
    /// Pushes your local user data to the network.
    Sync {
        /// Force push your local user data to the network.
        /// This will overwrite any existing data in your vault.
        #[arg(short, long)]
        force: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum WalletCmd {
    /// Create a wallet.
    Create {
        /// Optional flag to not add a password.
        #[clap(long, action)]
        no_password: bool,
        /// Optional password to encrypt the wallet with.
        #[clap(long, short)]
        password: Option<String>,
    },

    /// Import an existing wallet.
    Import {
        /// Hex-encoded private key.
        private_key: String,
        /// Optional flag to not add a password.
        #[clap(long, action)]
        no_password: bool,
        /// Optional password to encrypt the wallet with.
        #[clap(long, short)]
        password: Option<String>,
    },

    /// Print the private key of a wallet.
    Export,

    /// Check the balance of the wallet.
    Balance,
}

pub async fn handle_subcommand(opt: Opt) -> Result<()> {
    let peers = crate::access::network::get_peers(opt.peers);
    let cmd = opt.command;

    match cmd {
        Some(SubCmd::File { command }) => match command {
            FileCmd::Cost { file } => file::cost(&file, peers.await?).await,
            FileCmd::Upload { file, public } => file::upload(&file, public, peers.await?).await,
            FileCmd::Download { addr, dest_file } => {
                file::download(&addr, &dest_file, peers.await?).await
            }
            FileCmd::List => file::list(),
        },
        Some(SubCmd::Vault { command }) => match command {
            VaultCmd::Cost { expected_max_size } => {
                vault::cost(peers.await?, expected_max_size).await
            }
            VaultCmd::Create => vault::create(peers.await?).await,
            VaultCmd::Load => vault::load(peers.await?).await,
            VaultCmd::Sync { force } => vault::sync(force, peers.await?).await,
        },
        Some(SubCmd::Wallet { command }) => match command {
            WalletCmd::Create {
                no_password,
                password,
            } => wallet::create(no_password, password),
            WalletCmd::Import {
                private_key,
                no_password,
                password,
            } => wallet::import(private_key, no_password, password),
            WalletCmd::Export => wallet::export(),
            WalletCmd::Balance => wallet::balance(peers.await?.is_local()).await,
        },
        None => {
            // If no subcommand is given, default to clap's error behaviour.
            Opt::command()
                .error(ErrorKind::MissingSubcommand, "Please provide a subcommand")
                .exit();
        }
    }
}
