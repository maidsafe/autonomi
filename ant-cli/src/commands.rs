// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod analyze;
mod file;
mod pointer;
mod register;
mod scratchpad;
mod vault;
mod wallet;

use crate::actions::NetworkContext;
use crate::args::max_fee_per_gas::MaxFeePerGasParam;
use crate::opt::{NetworkId, Opt};
use autonomi::networking::Quorum;
use clap::{Args, CommandFactory as _, Subcommand, error::ErrorKind};
use color_eyre::Result;
use pointer::TargetDataType;
use pointer::parse_target_data_type;
use std::num::NonZeroUsize;
use std::path::PathBuf;

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

    /// Operations related to scratchpad management.
    Scratchpad {
        #[command(subcommand)]
        command: ScratchpadCmd,
    },

    /// Operations related to pointer management.
    Pointer {
        #[command(subcommand)]
        command: PointerCmd,
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
        /// Skip creating an archive after uploading a directory.
        /// When uploading a directory, files are normally grouped into an archive for easier management.
        /// This flag uploads the files individually without creating the archive metadata.
        /// Note: This option only affects directory uploads - single file uploads never create archives.
        #[arg(long)]
        no_archive: bool,
        /// Retry failed uploads automatically after 1 minute pause.
        /// This will persistently retry any failed chunks until all data is successfully uploaded.
        #[arg(long)]
        #[clap(default_value = "0")]
        retry_failed: u64,
        #[command(flatten)]
        transaction_opt: TransactionOpt,
    },

    /// Download a file from the given address.
    Download {
        /// The address of the file to download.
        addr: String,
        /// The destination file path.
        dest_file: String,
        /// Experimental: Optionally specify the quorum for the download (makes sure that we have n copies for each chunk).
        ///
        /// Possible values are: "one", "majority", "all", n (where n is a number greater than 0)
        #[arg(short, long, value_parser = parse_quorum)]
        quorum: Option<Quorum>,
        /// Experimental: Optionally specify the number of retries for the download.
        #[arg(short, long)]
        retries: Option<usize>,
        /// By default, chunks will be cached to enable resuming downloads.
        /// Set this flag to disable the cache.
        #[arg(long)]
        disable_cache: bool,
        /// Custom cache directory for chunk caching.
        /// If not specified, uses the default Autonomi client data directory.
        /// This option only applies when cache is enabled (default).
        #[arg(long, conflicts_with = "disable_cache")]
        cache_dir: Option<PathBuf>,
    },

    /// List previous uploads
    List {
        /// List files in archives. Requires network connection.
        #[arg(short, long)]
        verbose: bool,
    },
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
pub enum ScratchpadCmd {
    /// Generate a new general scratchpad key from which all your scratchpad keys can be derived (using their names).
    GenerateKey {
        /// Overwrite existing key if it exists
        /// Warning: overwriting the existing key will result in loss of access to any existing scratchpads
        #[arg(short, long)]
        overwrite: bool,
    },

    /// Estimate cost to create a scratchpad.
    Cost {
        /// The name of the scratchpad.
        name: String,
    },

    /// Create a new scratchpad.
    Create {
        /// The name of the scratchpad.
        name: String,
        /// The data to store in the scratchpad (Up to 4MB)
        data: String,
        #[command(flatten)]
        transaction_opt: TransactionOpt,
    },

    /// Share a scratchpad secret key with someone else.
    /// Sharing this key means that the other party will have permanent read and write access to the scratchpad.
    Share {
        /// The name of the scratchpad.
        name: String,
    },

    /// Get the contents of an existing scratchpad from the network.
    Get {
        /// The name of the scratchpad.
        name: String,
        /// Indicate that this is an external scratchpad secret key.
        /// (Use this when interacting with a scratchpad shared with you by someone else)
        #[arg(short, long)]
        secret_key: bool,
        /// Display the data as a hex string instead of raw bytes
        #[arg(long)]
        hex: bool,
    },

    /// Edit the contents of an existing scratchpad.
    Edit {
        /// The name of the scratchpad.
        name: String,
        /// Indicate that this is an external scratchpad secret key.
        /// (Use this when interacting with a scratchpad shared with you by someone else)
        #[arg(short, long)]
        secret_key: bool,
        /// The new data to store in the scratchpad (Up to 4MB)
        data: String,
    },

    /// List owned scratchpads
    List {
        /// Verbose output. Detailed description of the scratchpads.
        #[arg(short, long)]
        verbose: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum PointerCmd {
    /// Generate a new general pointer key from which all your pointer keys can be derived (using their names).
    GenerateKey {
        /// Overwrite existing key if it exists
        /// Warning: overwriting the existing key will result in loss of access to any existing pointers
        #[arg(short, long)]
        overwrite: bool,
    },

    /// Estimate cost to create a pointer.
    Cost {
        /// The name of the pointer.
        name: String,
    },

    /// Create a new pointer.
    Create {
        /// The name of the pointer.
        name: String,
        /// The target address to point to
        target: String,
        /// The data type of the target (valid values: graph, scratchpad, pointer, chunk, auto)
        /// If not specified (or 'auto'), the type will be automatically detected by fetching the data from the network
        #[arg(value_parser = parse_target_data_type, default_value = "auto", long, short)]
        target_data_type: TargetDataType,
        #[command(flatten)]
        transaction_opt: TransactionOpt,
    },

    /// Share a pointer secret key with someone else.
    /// Sharing this key means that the other party will have permanent read and write access to the pointer.
    Share {
        /// The name of the pointer.
        name: String,
    },

    /// Get the target of an existing pointer from the network.
    Get {
        /// The name of the pointer.
        name: String,
        /// Indicate that this is an external pointer secret key.
        /// (Use this when interacting with a pointer shared with you by someone else)
        #[arg(short, long)]
        secret_key: bool,
    },

    /// Edit the target of an existing pointer.
    Edit {
        /// The name of the pointer.
        name: String,
        /// The new target address to point to
        target: String,
        /// The data type of the target (valid values: graph, scratchpad, pointer, chunk, auto)
        /// If not specified (or 'auto'), the type will be automatically detected by fetching the data from the network
        #[arg(value_parser = parse_target_data_type, default_value = "auto", long, short)]
        target_data_type: TargetDataType,
        /// Indicate that this is an external pointer secret key.
        /// (Use this when interacting with a pointer shared with you by someone else)
        #[arg(short, long)]
        secret_key: bool,
    },

    /// List owned pointers
    List {
        /// Verbose output. Detailed description of the pointers.
        #[arg(short, long)]
        verbose: bool,
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
    /// - `auto`: Use the current max gas price bid. WARNING: Can result in high gas fees! (default: when using custom EVM network)
    /// - `limited-auto:<WEI AMOUNT>`: Use the current max gas price bid, with a specified upper limit.
    /// - `unlimited`: Do not use a limit for the gas price bid. WARNING: Can result in high gas fees!
    /// - `<WEI AMOUNT>`: Set a custom max gas price bid.
    #[clap(long, verbatim_doc_comment)]
    pub max_fee_per_gas: Option<MaxFeePerGasParam>,
}

pub async fn handle_subcommand(opt: Opt) -> Result<()> {
    let cmd = opt.command;

    let network_context = if opt.alpha {
        NetworkContext::new(opt.peers, NetworkId::alpha())
    } else {
        NetworkContext::new(opt.peers, opt.network_id)
    };

    match cmd {
        Some(SubCmd::File { command }) => match command {
            FileCmd::Cost { file } => file::cost(&file, network_context).await,
            FileCmd::Upload {
                file,
                public,
                no_archive,
                retry_failed,
                transaction_opt,
            } => {
                if let Err((err, exit_code)) = file::upload(
                    &file,
                    public,
                    no_archive,
                    network_context,
                    transaction_opt.max_fee_per_gas,
                    retry_failed,
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
                retries,
                disable_cache,
                cache_dir,
            } => {
                if let Err((err, exit_code)) = file::download(
                    &addr,
                    &dest_file,
                    network_context,
                    quorum,
                    retries,
                    !disable_cache, // Invert the flag - cache is enabled by default
                    cache_dir.as_ref(),
                )
                .await
                {
                    eprintln!("{err:?}");
                    if !disable_cache {
                        println!("Successfully downloaded chunks were cached.");
                        println!(
                            "Please run the command again to obtain the chunks that were not retrieved and complete the download."
                        );
                    }
                    std::process::exit(exit_code);
                } else {
                    Ok(())
                }
            }
            FileCmd::List { verbose } => {
                if let Err((err, exit_code)) = file::list(network_context, verbose).await {
                    eprintln!("{err:?}");
                    std::process::exit(exit_code);
                } else {
                    Ok(())
                }
            }
        },
        Some(SubCmd::Register { command }) => match command {
            RegisterCmd::GenerateKey { overwrite } => register::generate_key(overwrite),
            RegisterCmd::Cost { name } => register::cost(&name, network_context).await,
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
                    network_context,
                    transaction_opt.max_fee_per_gas,
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
                    network_context,
                    transaction_opt.max_fee_per_gas,
                )
                .await
            }
            RegisterCmd::Get { address, name, hex } => {
                register::get(address, name, hex, network_context).await
            }
            RegisterCmd::History { address, name, hex } => {
                register::history(address, name, hex, network_context).await
            }
            RegisterCmd::List => register::list(),
        },
        Some(SubCmd::Vault { command }) => match command {
            VaultCmd::Cost { expected_max_size } => {
                vault::cost(network_context, expected_max_size).await
            }
            VaultCmd::Create { transaction_opt } => {
                vault::create(network_context, transaction_opt.max_fee_per_gas).await
            }
            VaultCmd::Load => vault::load(network_context).await,
            VaultCmd::Sync { force } => vault::sync(force, network_context).await,
        },
        Some(SubCmd::Scratchpad { command }) => match command {
            ScratchpadCmd::GenerateKey { overwrite } => scratchpad::generate_key(overwrite),
            ScratchpadCmd::Cost { name } => scratchpad::cost(name, network_context).await,
            ScratchpadCmd::Create {
                name,
                data,
                transaction_opt,
            } => {
                scratchpad::create(network_context, name, data, transaction_opt.max_fee_per_gas)
                    .await
            }
            ScratchpadCmd::Share { name } => scratchpad::share(name),
            ScratchpadCmd::Get {
                name,
                secret_key,
                hex,
            } => scratchpad::get(network_context, name, secret_key, hex).await,
            ScratchpadCmd::Edit {
                name,
                secret_key,
                data,
            } => scratchpad::edit(network_context, name, secret_key, data).await,
            ScratchpadCmd::List { verbose } => scratchpad::list(verbose),
        },
        Some(SubCmd::Pointer { command }) => match command {
            PointerCmd::GenerateKey { overwrite } => pointer::generate_key(overwrite),
            PointerCmd::Cost { name } => pointer::cost(name, network_context).await,
            PointerCmd::Create {
                name,
                target,
                target_data_type,
                transaction_opt,
            } => {
                pointer::create(
                    network_context,
                    name,
                    target,
                    target_data_type,
                    transaction_opt.max_fee_per_gas,
                )
                .await
            }
            PointerCmd::Share { name } => pointer::share(name),
            PointerCmd::Get { name, secret_key } => {
                pointer::get(network_context, name, secret_key).await
            }
            PointerCmd::Edit {
                name,
                target,
                target_data_type,
                secret_key,
            } => pointer::edit(network_context, name, secret_key, target, target_data_type).await,
            PointerCmd::List { verbose } => pointer::list(verbose),
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
            WalletCmd::Balance => wallet::balance(network_context).await,
        },
        Some(SubCmd::Analyze { addr, verbose }) => {
            analyze::analyze(&addr, verbose, network_context).await
        }
        None => {
            // If no subcommand is given, default to clap's error behaviour.
            Opt::command()
                .error(ErrorKind::MissingSubcommand, "Please provide a subcommand")
                .exit();
        }
    }
}

fn parse_quorum(str: &str) -> Result<Quorum, String> {
    match str {
        "one" => Ok(Quorum::One),
        "majority" => Ok(Quorum::Majority),
        "all" => Ok(Quorum::All),
        _ => {
            let n: NonZeroUsize = str.parse().map_err(|_| "Invalid quorum value")?;
            Ok(Quorum::N(n))
        }
    }
}
