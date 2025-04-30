// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod analyze;
mod file;
mod register;
mod vault;
mod wallet;

use crate::args::max_fee_per_gas::MaxFeePerGasParam;
use crate::opt::Opt;
use autonomi::ResponseQuorum;
use clap::{error::ErrorKind, Args, CommandFactory as _, Subcommand};
use color_eyre::Result;

#[derive(Subcommand, Debug)]
pub enum SubCmd {
    /// Operations related to file handling.
    File {
        #[command(subcommand)]
        command: FileCmd,
    },

    /// Operations related to register management.
    Register {
        #[command(subcommand)]
        command: RegisterCmd,
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

    /// Operations related to data analysis.
    Analyze {
        /// The address of the data to analyse.
        addr: String,
        /// Verbose output. Detailed description of the analysis.
        #[arg(short, long)]
        verbose: bool,
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
        /// Experimental: Optionally specify the quorum for the verification of the upload.
        ///
        /// Possible values are: "one", "majority", "all", n (where n is a number greater than 0)
        #[arg(short, long)]
        quorum: Option<ResponseQuorum>,
        #[command(flatten)]
        transaction_opt: TransactionOpt,
    },

    /// Download a file from the given address.
    Download {
        /// The address of the file to download.
        addr: String,
        /// The destination file path.
        dest_file: String,
        /// Experimental: Optionally specify the quorum for the download (makes sure that we have n copies for each chunks).
        ///
        /// Possible values are: "one", "majority", "all", n (where n is a number greater than 0)
        #[arg(short, long)]
        quorum: Option<ResponseQuorum>,
    },

    /// List previous uploads
    List,
}

#[derive(Subcommand, Debug)]
pub enum RegisterCmd {
    /// Generate a new register key.
    GenerateKey {
        /// Overwrite existing key if it exists
        /// Warning: overwriting the existing key will result in loss of access to any existing registers created using that key
        #[arg(short, long)]
        overwrite: bool,
    },

    /// Estimate cost to register a name.
    Cost {
        /// The name to register.
        name: String,
    },

    /// Create a new register with the given name and value.
    /// Note that anyone with the register address can read its value.
    Create {
        /// The name of the register.
        name: String,
        /// The value to store in the register.
        value: String,
        /// Treat the value as a hex string and convert it to binary before storing
        #[arg(long)]
        hex: bool,
        #[command(flatten)]
        transaction_opt: TransactionOpt,
    },

    /// Edit an existing register.
    /// Note that anyone with the register address can read its value.
    Edit {
        /// Use the name of the register instead of the address
        /// Note that only the owner of the register can use this shorthand as the address can be generated from the name and register key.
        #[arg(short, long)]
        name: bool,
        /// The address of the register
        /// With the name option on the address will be used as a name
        address: String,
        /// The new value to store in the register.
        value: String,
        /// Treat the value as a hex string and convert it to binary before storing
        #[arg(long)]
        hex: bool,
        #[command(flatten)]
        transaction_opt: TransactionOpt,
    },

    /// Get the value of a register.
    Get {
        /// Use the name of the register instead of the address
        /// Note that only the owner of the register can use this shorthand as the address can be generated from the name and register key.
        #[arg(short, long)]
        name: bool,
        /// The address of the register
        /// With the name option on the address will be used as a name
        address: String,
        /// Display the value as a hex string instead of raw bytes
        #[arg(long)]
        hex: bool,
    },

    /// Show the history of values for a register.
    History {
        /// Use the name of the register instead of the address
        /// Note that only the owner of the register can use this shorthand as the address can be generated from the name and register key.
        #[arg(short, long)]
        name: bool,
        /// The address of the register
        /// With the name option on the address will be used as a name
        address: String,
        /// Display the values as hex strings instead of raw bytes
        #[arg(long)]
        hex: bool,
    },

    /// List previous registers
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
    Create {
        #[command(flatten)]
        transaction_opt: TransactionOpt,
    },

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

#[derive(Args, Debug)]
pub(crate) struct TransactionOpt {
    /// Max fee per gas / gas price bid.
    /// Options:
    /// - `low`: Use the average max gas price bid.
    /// - `market`: Use the current max gas price bid, with a max of 4 * the average gas price bid. (default)
    /// - `auto`: Use the current max gas price bid. WARNING: Can result in high gas fees!
    /// - `limited-auto:<WEI AMOUNT>`: Use the current max gas price bid, with a specified upper limit.
    /// - `unlimited`: Do not use a limit for the gas price bid. WARNING: Can result in high gas fees!
    /// - `<WEI AMOUNT>`: Set a custom max gas price bid.
    #[clap(long, verbatim_doc_comment, default_value = "market")]
    pub max_fee_per_gas: MaxFeePerGasParam,
}

pub async fn handle_subcommand(opt: Opt) -> Result<()> {
    let cmd = opt.command;

    match cmd {
        Some(SubCmd::File { command }) => match command {
            FileCmd::Cost { file } => file::cost(&file, opt.peers, opt.network_id).await,
            FileCmd::Upload {
                file,
                public,
                quorum,
                transaction_opt,
            } => {
                if let Err((err, exit_code)) = file::upload(
                    &file,
                    public,
                    opt.peers,
                    quorum,
                    transaction_opt.max_fee_per_gas,
                    opt.network_id,
                )
                .await
                {
                    eprintln!("{err:?}");
                    std::process::exit(exit_code);
                } else {
                    Ok(())
                }
            }
            FileCmd::Download {
                addr,
                dest_file,
                quorum,
            } => {
                if let Err((err, exit_code)) =
                    file::download(&addr, &dest_file, opt.peers, quorum, opt.network_id).await
                {
                    eprintln!("{err:?}");
                    std::process::exit(exit_code);
                } else {
                    Ok(())
                }
            }
            FileCmd::List => file::list(),
        },
        Some(SubCmd::Register { command }) => match command {
            RegisterCmd::GenerateKey { overwrite } => register::generate_key(overwrite),
            RegisterCmd::Cost { name } => register::cost(&name, opt.peers, opt.network_id).await,
            RegisterCmd::Create {
                name,
                value,
                hex,
                transaction_opt,
            } => {
                register::create(
                    &name,
                    &value,
                    hex,
                    opt.peers,
                    transaction_opt.max_fee_per_gas,
                    opt.network_id,
                )
                .await
            }
            RegisterCmd::Edit {
                address,
                name,
                value,
                hex,
                transaction_opt,
            } => {
                register::edit(
                    address,
                    name,
                    &value,
                    hex,
                    opt.peers,
                    transaction_opt.max_fee_per_gas,
                    opt.network_id,
                )
                .await
            }
            RegisterCmd::Get { address, name, hex } => {
                register::get(address, name, hex, opt.peers, opt.network_id).await
            }
            RegisterCmd::History { address, name, hex } => {
                register::history(address, name, hex, opt.peers, opt.network_id).await
            }
            RegisterCmd::List => register::list(),
        },
        Some(SubCmd::Vault { command }) => match command {
            VaultCmd::Cost { expected_max_size } => {
                vault::cost(opt.peers, expected_max_size, opt.network_id).await
            }
            VaultCmd::Create { transaction_opt } => {
                vault::create(opt.peers, transaction_opt.max_fee_per_gas, opt.network_id).await
            }
            VaultCmd::Load => vault::load(opt.peers, opt.network_id).await,
            VaultCmd::Sync { force } => vault::sync(force, opt.peers, opt.network_id).await,
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
            WalletCmd::Balance => wallet::balance(opt.peers.local).await,
        },
        Some(SubCmd::Analyze { addr, verbose }) => {
            analyze::analyze(&addr, verbose, opt.peers, opt.network_id).await
        }
        None => {
            // If no subcommand is given, default to clap's error behaviour.
            Opt::command()
                .error(ErrorKind::MissingSubcommand, "Please provide a subcommand")
                .exit();
        }
    }
}
