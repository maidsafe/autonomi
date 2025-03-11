// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod terminal;

#[macro_use]
extern crate tracing;

use ant_bootstrap::InitialPeersConfig;
#[cfg(target_os = "windows")]
use ant_node_manager::config::is_running_as_root;
use clap::Parser;
use color_eyre::eyre::Result;
use node_launchpad::{
    app::App,
    config::configure_winsw,
    utils::{initialize_logging, initialize_panic_handler},
};
use std::{env, path::PathBuf};

#[derive(Parser, Debug)]
#[command(disable_version_flag = true)]
pub struct Cli {
    /// Provide a path for the antnode binary to be used by the service.
    ///
    /// Useful for creating the service using a custom built binary.
    #[clap(long)]
    antnode_path: Option<PathBuf>,

    /// Print the crate version.
    #[clap(long)]
    crate_version: bool,

    /// Specify the network ID to use. This will allow you to run the node on a different network.
    ///
    /// By default, the network ID is set to 1, which represents the mainnet.
    #[clap(long, verbatim_doc_comment)]
    network_id: Option<u8>,

    /// Frame rate, i.e. number of frames per second
    #[arg(short, long, value_name = "FLOAT", default_value_t = 60.0)]
    frame_rate: f64,

    /// Provide a path for the antnode binary to be used by the service.
    ///
    /// Useful for creating the service using a custom built binary.
    #[clap(long)]
    path: Option<PathBuf>,

    #[command(flatten)]
    peers: InitialPeersConfig,

    /// Print the package version.
    #[clap(long)]
    #[cfg(not(feature = "nightly"))]
    package_version: bool,

    /// Tick rate, i.e. number of ticks per second
    #[arg(short, long, value_name = "FLOAT", default_value_t = 1.0)]
    tick_rate: f64,

    /// Print the version.
    #[clap(long)]
    version: bool,
}

fn is_running_in_terminal() -> bool {
    atty::is(atty::Stream::Stdout)
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    initialize_logging()?;
    configure_winsw().await?;

    if !is_running_in_terminal() {
        info!("Running in non-terminal mode. Launching terminal.");
        // If we weren't already running in a terminal, this process returns early, having spawned
        // a new process that launches a terminal.
        let terminal_type = terminal::detect_and_setup_terminal()?;
        terminal::launch_terminal(&terminal_type)
            .inspect_err(|err| error!("Error while launching terminal: {err:?}"))?;
        return Ok(());
    } else {
        // Windows spawns the terminal directly, so the check for root has to happen here as well.
        debug!("Running inside a terminal!");
        #[cfg(target_os = "windows")]
        if !is_running_as_root() {
            {
                // TODO: There is no terminal to show this error message when double clicking on the exe.
                error!("Admin privileges required to run on Windows. Exiting.");
                color_eyre::eyre::bail!("Admin privileges required to run on Windows. Exiting.");
            }
        }
    }

    initialize_panic_handler()?;
    let args = Cli::parse();

    if args.version {
        println!(
            "{}",
            ant_build_info::version_string(
                "Autonomi Node Launchpad",
                env!("CARGO_PKG_VERSION"),
                None
            )
        );
        return Ok(());
    }

    if args.crate_version {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    #[cfg(not(feature = "nightly"))]
    if args.package_version {
        println!("{}", ant_build_info::package_version());
        return Ok(());
    }

    info!("Starting app with args: {args:?}");
    let mut app = App::new(
        args.tick_rate,
        args.frame_rate,
        args.peers,
        args.antnode_path,
        args.path,
        args.network_id,
    )
    .await?;
    app.run().await?;

    Ok(())
}
