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
use ant_service_management::{NatDetectionStatus, NodeRegistryManager};
use color_eyre::eyre::{bail, Context, Result};
use libp2p::Multiaddr;
use rand::seq::SliceRandom;
use std::{
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};
use tokio::{task, time::timeout};
pub const NAT_DETECTION_TIMEOUT_SECS: u64 = 180;

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
            contacts_fetcher.ignore_peer_id(true);
            contacts_fetcher.insert_endpoint(NAT_DETECTION_SERVERS_LIST_URL.parse()?);

            let fetched = contacts_fetcher.fetch_addrs().await?;
            fetched
                .choose_multiple(&mut rand::thread_rng(), 10)
                .cloned()
                .collect::<Vec<_>>()
        }
    };
    info!("Running nat detection with servers: {servers:?}");

    let node_registry = NodeRegistryManager::load(&get_node_registry_path()?).await?;

    if !force_run {
        if let Some(status) = node_registry.nat_status.read().await.as_ref() {
            if verbosity != VerbosityLevel::Minimal {
                println!("NAT status has already been set as: {status:?}");
            }
            debug!("NAT status already set as: {status:?}, skipping.");
            return Ok(());
        }
    }

    let nat_detection_path = if let Some(path) = path {
        path
    } else {
        let release_repo = <dyn AntReleaseRepoActions>::default_config();
        let (path, _) = download_and_extract_release(
            ReleaseType::NatDetection,
            url,
            version,
            &*release_repo,
            verbosity,
            None,
        )
        .await?;
        path
    };

    if verbosity != VerbosityLevel::Minimal {
        println!("Running NAT detection. This can take a while..");
    }
    debug!(
        "Running NAT detection with binary path: {:?}",
        nat_detection_path
    );

    let servers_arg = servers
        .iter()
        .map(|a| a.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let trace_enabled = tracing::level_enabled!(tracing::Level::TRACE);
    let timeout_duration = Duration::from_secs(NAT_DETECTION_TIMEOUT_SECS);
    debug!(
        "NAT detection timeout set to {} seconds",
        NAT_DETECTION_TIMEOUT_SECS
    );

    let output = timeout(
        timeout_duration,
        task::spawn_blocking(move || -> Result<i32> {
            let mut command = Command::new(&nat_detection_path);
            command.stdout(Stdio::piped()).stderr(Stdio::null());
            command.arg(servers_arg);
            if trace_enabled {
                command.arg("-vvvv");
            }

            let mut child = command
                .spawn()
                .wrap_err("Failed to spawn NAT detection process")?;

            if trace_enabled {
                if let Some(ref mut stdout) = child.stdout {
                    let reader = BufReader::new(stdout);
                    for line in reader.lines() {
                        let line = line?;
                        let clean_line = strip_ansi_escapes(&line);
                        trace!("{clean_line}");
                    }
                }
            }

            let status = child
                .wait()
                .wrap_err("Failed to wait on NAT detection process")?;
            Ok(status.code().unwrap_or(-1))
        }),
    )
    .await;

    let exit_code = match output {
        Ok(Ok(code)) => code,
        Ok(Err(e)) => bail!("Failed to detect NAT status, exit code: {:?}", e),
        Err(_) => {
            debug!(
                "NAT detection timed out after {} seconds",
                NAT_DETECTION_TIMEOUT_SECS
            );
            bail!(
                "NAT detection timed out after {} seconds",
                NAT_DETECTION_TIMEOUT_SECS
            );
        }
    };

    let status = match exit_code {
        Ok(10) => NatDetectionStatus::Public,
        Ok(11) => NatDetectionStatus::UPnP,
        Ok(12) => NatDetectionStatus::Private,
        code => bail!("Failed to detect NAT status, exit code: {:?}", code),
    };

    if verbosity != VerbosityLevel::Minimal {
        println!("NAT status has been found to be: {status:?}");
    }

    *node_registry.nat_status.write().await = Some(status);
    node_registry.save().await?;

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
