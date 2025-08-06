// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// Allow expect usage in this binary
#![allow(clippy::expect_used)]

use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Read};
use tokio::time::{Duration, sleep};
use warp::Filter;

use dirs_next::home_dir;
use std::path::PathBuf;

fn data_file_path() -> PathBuf {
    let mut path = home_dir().expect("Could not get home directory");
    path.push(".autonomi_token_supplies");
    fs::create_dir_all(&path).expect("Failed to create directory");
    path.push("data.json");
    path
}

#[derive(Deserialize, Debug, Clone)]
struct ApiResponse {
    maid_total_circulating_cap: u64,
    omni_burned: u64,
    smart_contract_minted: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct SharedData {
    maid_supply: u64,
    emaid_supply: u64,
}

async fn fetch_api_data() -> Result<ApiResponse, reqwest::Error> {
    reqwest::get("https://emaid.online/api")
        .await?
        .json::<ApiResponse>()
        .await
}

async fn scheduled_fetch() {
    loop {
        let api_result = fetch_api_data().await;
        match api_result {
            Ok(api_data) => {
                let data = SharedData {
                    maid_supply: api_data.maid_total_circulating_cap - api_data.omni_burned,
                    emaid_supply: api_data.smart_contract_minted,
                };
                match write_to_file(&data) {
                    Ok(()) => println!("Data written to file successfully"),
                    Err(e) => eprintln!("Failed to write to file: {e}"),
                }
            }
            Err(e) => eprintln!("Failed to fetch API data: {e}"),
        }

        sleep(Duration::from_secs(43200)).await; // Sleep for 12 hours
    }
}

fn write_to_file(data: &SharedData) -> io::Result<()> {
    let json = serde_json::to_string(data)?;
    fs::write(data_file_path(), json)?;
    Ok(())
}

fn read_from_file() -> io::Result<SharedData> {
    let mut file = File::open(data_file_path())?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let data = serde_json::from_str(&contents)?;
    Ok(data)
}

#[tokio::main]
async fn main() {
    tokio::spawn(async {
        scheduled_fetch().await;
    });

    let maid_supply = warp::path!("maid").map(|| match read_from_file() {
        Ok(data) => format!("{}", data.maid_supply),
        Err(e) => format!("Error reading data: {e}"),
    });

    let emaid_supply = warp::path!("emaid").map(|| match read_from_file() {
        Ok(data) => format!("{}", data.emaid_supply),
        Err(e) => format!("Error reading data: {e}"),
    });

    warp::serve(maid_supply.or(emaid_supply))
        .run(([0, 0, 0, 0], 3030))
        .await;
}
