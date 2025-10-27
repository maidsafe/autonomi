// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// Allow expect_used in binary - to be refactored
#![allow(clippy::expect_used)]

mod terminal;

#[macro_use]
extern crate tracing;

use ant_bootstrap::InitialPeersConfig;
use ant_logging::LogBuilder;
#[cfg(target_os = "windows")]
use ant_node_manager::config::is_running_as_root;
use clap::Parser;
use color_eyre::eyre::Result;
use node_launchpad::{
    app::App,
    config::{configure_winsw, get_launchpad_data_dir_path},
};
use std::{path::PathBuf, time::Duration};
use tracing::{Level, error};

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

fn main() -> Result<()> {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    let _log_handle = get_log_builder()?.initialize()?;
    let result: Result<()> = rt.block_on(async {
        ensure_admin_privileges()?;
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
            debug!("Running inside a terminal!");
        }

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
        info!("App finished running");
        Ok(())
    });
    result?;

    info!("Shutting down runtime");
    rt.shutdown_timeout(Duration::from_millis(100));

    Ok(())
}

pub fn get_log_builder() -> Result<LogBuilder> {
    let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let log_path = get_launchpad_data_dir_path()?
        .join("logs")
        .join(format!("launchpad_{timestamp}.log"));

    let logging_targets = vec![
        ("ant_bootstrap".to_string(), Level::DEBUG),
        ("evmlib".to_string(), Level::DEBUG),
        ("ant_node_manager".to_string(), Level::DEBUG),
        ("ant_service_management".to_string(), Level::DEBUG),
        ("service-manager".to_string(), Level::DEBUG),
        ("node_launchpad".to_string(), Level::DEBUG),
    ];
    let mut log_builder = LogBuilder::new(logging_targets);
    log_builder.output_dest(ant_logging::LogOutputDest::Path(log_path));
    log_builder.print_updates_to_stdout(false);
    Ok(log_builder)
}

#[cfg(target_os = "windows")]
fn ensure_admin_privileges() -> Result<()> {
    use std::io::{self, Write};

    if is_running_as_root() {
        return Ok(());
    }

    println!("Administrator privileges are required to manage Autonomi node services on Windows.");
    println!(
        "Requesting elevation. Please approve the Windows User Account Control prompt to continue..."
    );
    io::stdout().flush().ok();

    let exe = std::env::current_exe()?;
    let args: Vec<_> = std::env::args_os().skip(1).collect();

    // Get exe name before moving exe
    let exe_name = exe
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("node-launchpad")
        .to_string();

    let mut cmd = runas::Command::new(exe);
    for arg in args {
        cmd.arg(arg);
    }

    // Launch elevation in background
    std::thread::spawn(move || {
        let _ = cmd.status();
    });

    println!("Waiting for UAC approval and elevated process to start...");

    let start_time = std::time::Instant::now();
    let max_wait = std::time::Duration::from_secs(300);

    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Check if an elevated version of our process is running
        if is_elevated_process_running(&exe_name) {
            println!("Elevated process is now running with administrator privileges.");
            println!("You can close this window.");
            break;
        }

        if start_time.elapsed() > max_wait {
            color_eyre::eyre::bail!(
                "Timeout waiting for elevated process. Please try running from an elevated terminal."
            );
        }
    }

    std::process::exit(0)
}

#[cfg(target_os = "windows")]
fn is_elevated_process_running(exe_name: &str) -> bool {
    use sysinfo::System;

    let system = System::new_all();

    // Count processes with our executable name
    let count = system.processes_by_name(exe_name).count();

    // If more than 1 instance, elevated version likely started
    count > 1
}

#[cfg(not(target_os = "windows"))]
fn ensure_admin_privileges() -> Result<()> {
    Ok(())
}
