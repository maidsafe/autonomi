// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use clap::{command, Arg, ArgAction};
use color_eyre::{eyre::eyre, Result};
use regex::Regex;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::Command,
};
use walkdir::WalkDir;

const LOG_FILENAME_PREFIX: &str = "antnode.log";
type NodeId = String;

#[derive(serde::Serialize)]
struct PrometheusConfig {
    global: Global,
    scrape_configs: Vec<ScrapeConfigs>,
}

#[derive(serde::Serialize)]
struct Global {
    scrape_interval: String,
    evaluation_interval: String,
}

#[derive(serde::Serialize)]
struct ScrapeConfigs {
    job_name: String,
    // Override the global default
    scrape_interval: String,
    static_configs: Vec<StaticConfig>,
}

#[derive(serde::Serialize)]
struct StaticConfig {
    targets: Vec<String>,
    labels: Labels,
}

#[derive(serde::Serialize)]
struct Labels {
    node_id: NodeId,
}

fn main() -> Result<()> {
    let default_log_dir = dirs_next::data_dir()
        .ok_or_else(|| eyre!("could not obtain data directory path".to_string()))?
        .join("autonomi")
        .join("node");

    let matches = command!()
        .arg(
            Arg::new("run")
                .short('r')
                .long("run")
                .help("Runs the docker containers for you")
                .action(ArgAction::SetTrue)
            )
        .arg(
            Arg::new("log_dirs")
                .help("Provide one or more log directories to get the metrics server from.\nAll the files inside a provided dir are scanned.")
                .action(ArgAction::Append)
                .value_parser(clap::value_parser!(PathBuf))
                .default_value(default_log_dir.into_os_string())
        )
        .get_matches();

    let should_run_containers = matches.get_flag("run");
    let mut metrics_server_list = BTreeMap::<NodeId, url::Url>::new();
    for log_dir in matches.get_many::<PathBuf>("log_dirs").unwrap() {
        metrics_server_list.extend(get_metric_servers(log_dir)?);
    }

    if metrics_server_list.is_empty() {
        return Err(eyre!("Could not find any metrics server. Aborting!"));
    }

    println!(
        "Collecting metrics from {} nodes",
        metrics_server_list.len()
    );
    let prometheus_config = build_prometheus_config(metrics_server_list);
    let prometheus_config = serde_yaml::to_string(&prometheus_config)?;

    let working_dir = get_working_dir()?;
    // write prometheus config
    let prometheus_dir = working_dir.join("prometheus");
    fs::create_dir_all(&prometheus_dir)?;
    fs::write(prometheus_dir.join("prometheus.yml"), prometheus_config)?;

    if should_run_containers {
        // stop the containers if running already
        let docker_output = Command::new("docker-compose")
            .arg("down")
            .arg("--volumes")
            .current_dir(&working_dir)
            .output()?;
        if !docker_output.status.success() {
            return Err(eyre!(
                "'docker-compose down' failed with {:?}",
                String::from_utf8(docker_output.stderr)?
            ));
        }

        // start the containers
        let docker_output = Command::new("docker-compose")
            .arg("up")
            .arg("-d")
            .current_dir(&working_dir)
            .output()?;
        if !docker_output.status.success() {
            return Err(eyre!(
                "'docker-compose up' failed with {:?}",
                String::from_utf8(docker_output.stderr)?
            ));
        }

        println!("Grafana dashboard is running at http://localhost:3001/d/node_metrics/node-metrics?orgId=1&refresh=5s");
        println!("Connect with the following credentials\nusername:admin\npassword:pwd");
    } else {
        println!("The Prometheus config file has been updated with the metrics server URLs. The containers are not yet started\nRead the docs to start/stop the containers.");
    }

    Ok(())
}

// Parse node logs files and extract the metrics server url for each node
fn get_metric_servers(path: &Path) -> Result<BTreeMap<NodeId, url::Url>> {
    let mut urls = BTreeMap::<NodeId, url::Url>::new();
    let re_node_id = Regex::new(r"Node \(PID: (\d+)\) with PeerId: (.*)")?;
    let re_metrics_server = Regex::new(r"Metrics server on (.*)")?;

    let log_files = WalkDir::new(path).into_iter().filter_map(|entry| {
        entry.ok().and_then(|f| {
            if f.file_type().is_file() {
                Some(f.into_path())
            } else {
                None
            }
        })
    });

    for file_path in log_files {
        let file_name = if let Some(name) = file_path.file_name().and_then(|s| s.to_str()) {
            name
        } else {
            println!("Failed to obtain filename from {}", file_path.display());
            continue;
        };

        if file_name.starts_with(LOG_FILENAME_PREFIX) {
            let file = File::open(&file_path)?;
            let lines = BufReader::new(file).lines().map_while(|item| item.ok());

            let mut peer_id: Option<NodeId> = None;
            let mut metrics_server_url: Option<url::Url> = None;
            for line in lines {
                if peer_id.is_some() && metrics_server_url.is_some() {
                    break;
                }

                if let Some(cap) = re_node_id.captures_iter(&line).next() {
                    peer_id = Some(
                        cap[2]
                            .parse()
                            .expect("Failed to parse NodeId from node log"),
                    );
                }

                if let Some(cap) = re_metrics_server.captures_iter(&line).next() {
                    let url = url::Url::parse(&cap[1])
                        .expect("Failed to parse metrics server URL from node log");
                    metrics_server_url = Some(url);
                }
            }

            if let (Some(node), Some(url)) = (peer_id, metrics_server_url) {
                urls.insert(node, url);
            }
        }
    }

    Ok(urls)
}

// build the prometheus config given the NodeId and the metrics server url
fn build_prometheus_config(metrics_server_list: BTreeMap<NodeId, url::Url>) -> PrometheusConfig {
    let static_configs = metrics_server_list
        .into_iter()
        .map(|(node_id, url)| StaticConfig {
            targets: vec![format!(
                "host.docker.internal:{}",
                url.port()
                    .expect("Port should be present for the metrics server")
            )],
            labels: Labels {
                node_id: last_n_chars(&node_id, 4),
            },
        })
        .collect();
    PrometheusConfig {
        global: Global {
            scrape_interval: "15s".to_string(),
            evaluation_interval: "15s".to_string(),
        },
        scrape_configs: vec![ScrapeConfigs {
            job_name: "safe_network_testnet".to_string(),
            scrape_interval: "5s".to_string(),
            static_configs,
        }],
    }
}

// Returns the `./metrics` dir
fn get_working_dir() -> Result<PathBuf> {
    let working_dir: PathBuf = std::env::current_dir()?;
    if working_dir.ends_with("metrics") {
        Ok(working_dir)
    } else {
        Ok(working_dir.join("metrics"))
    }
}

fn last_n_chars(s: &str, n: usize) -> String {
    s.chars()
        .rev()
        .take(n)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}
