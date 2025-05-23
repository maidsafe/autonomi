// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    config::get_node_registry_path, helpers::download_and_extract_release, VerbosityLevel,
};
use ant_bootstrap::ContactsFetcher;
use ant_releases::{AntReleaseRepoActions, ReleaseType};
use ant_service_management::{NatDetectionStatus, NodeRegistry};
use color_eyre::eyre::{bail, OptionExt, Result};
use libp2p::Multiaddr;
use rand::seq::SliceRandom;
use std::{
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Command, Stdio},
};

const NAT_DETECTION_SERVERS_LIST_URL: &str =
    "https://sn-testnet.s3.eu-west-2.amazonaws.com/nat-detection-servers";

pub async fn run_nat_detection(
    servers: Option<Vec<Multiaddr>>,
    force_run: bool,
    path: Option<PathBuf>,
    url: Option<String>,
    version: Option<String>,
    verbosity: VerbosityLevel,
) -> Result<()> {
    let servers = match servers {
        Some(servers) => servers,
        None => {
            let mut contacts_fetcher = ContactsFetcher::new()?;
            contacts_fetcher.insert_endpoint(NAT_DETECTION_SERVERS_LIST_URL.parse()?);

            let servers = contacts_fetcher.fetch_addrs().await?;

            servers
                .choose_multiple(&mut rand::thread_rng(), 10)
                .cloned()
                .collect::<Vec<_>>()
        }
    };
    info!("Running nat detection with servers: {servers:?}");
    let mut node_registry = NodeRegistry::load(&get_node_registry_path()?)?;

    if !force_run {
        if let Some(status) = node_registry.nat_status {
            if verbosity != VerbosityLevel::Minimal {
                println!("NAT status has already been set as: {status:?}");
            }
            debug!("NAT status has already been set as: {status:?}, returning.");
            return Ok(());
        }
    }

    let nat_detection_path = if let Some(path) = path {
        path
    } else {
        let release_repo = <dyn AntReleaseRepoActions>::default_config();

        let (nat_detection_path, _) = download_and_extract_release(
            ReleaseType::NatDetection,
            url,
            version,
            &*release_repo,
            verbosity,
            None,
        )
        .await?;
        nat_detection_path
    };

    if verbosity != VerbosityLevel::Minimal {
        println!("Running NAT detection. This can take a while..");
    }
    debug!("Running NAT detection with path: {nat_detection_path:?}. This can take a while..");

    let mut command = Command::new(nat_detection_path);
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    command.arg(
        servers
            .iter()
            .map(|addr| addr.to_string())
            .collect::<Vec<String>>()
            .join(","),
    );
    if tracing::level_enabled!(tracing::Level::TRACE) {
        command.arg("-vvvv");
    }
    let mut child = command.spawn()?;

    // only execute if log level is set to trace
    if tracing::level_enabled!(tracing::Level::TRACE) {
        // using buf reader to handle both stderr and stout is risky as it might block indefinitely.
        if let Some(ref mut stdout) = child.stdout {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let line = line?;
                // only if log level is trace

                let clean_line = strip_ansi_escapes(&line);
                trace!("{clean_line}");
            }
        }
    }

    let status = child.wait()?;
    let status = match status.code().ok_or_eyre("Failed to get the exit code")? {
        10 => NatDetectionStatus::Public,
        11 => NatDetectionStatus::UPnP,
        12 => NatDetectionStatus::Private,
        code => bail!("Failed to detect NAT status, exit code: {code}"),
    };

    if verbosity != VerbosityLevel::Minimal {
        println!("NAT status has been found to be: {status:?}");
    }

    node_registry.nat_status = Some(status);
    node_registry.save()?;

    Ok(())
}

fn strip_ansi_escapes(input: &str) -> String {
    let mut output = String::new();
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for next_char in chars.by_ref() {
                if next_char.is_ascii_lowercase() || next_char.is_ascii_uppercase() {
                    break;
                }
            }
        } else {
            output.push(c);
        }
    }
    output
}
