// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use autonomi::client::{ChunkBatchUploadState, payment::Receipt};
use color_eyre::eyre::{Context, Result};
use std::collections::HashMap;
use std::fs::{DirEntry, File};
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// Cleanup old cached payments after 30 days
const PAYMENT_EXPIRATION_SECS: u64 = 3600 * 24 * 30;

pub fn get_payments_dir() -> Result<PathBuf> {
    let dir = super::data_dir::get_client_data_dir_path()?;
    let payments_dir = dir.join("payments");
    std::fs::create_dir_all(&payments_dir)
        .wrap_err("Could not create cached payments directory")?;
    Ok(payments_dir)
}

/// Save the payment for the given file name to be reused later.
pub fn save_payment(file: &str, upload_state: &ChunkBatchUploadState) -> Result<()> {
    let dir = get_payments_dir()?;
    let timestamp =
        get_timestamp_from_receipt(upload_state.payment.as_ref().unwrap_or(&HashMap::new()));
    let file_hash = filename_short(file);
    let file_path = dir.join(format!("{timestamp}_{file_hash}"));

    let file = File::create(&file_path)?;
    let writer = BufWriter::new(&file);
    serde_json::to_writer(writer, &upload_state)?;

    println!("Cached payment for {file:?} to {}", file_path.display());
    Ok(())
}

/// Load the payment for the given file name.
/// Returns None if no payment is found.
pub fn load_payment_for_file(file_name: &str) -> Result<Option<Receipt>> {
    cleanup_outdated_payments()?;

    let dir = get_payments_dir()?;
    let file_hash = filename_short(file_name);

    let files = std::fs::read_dir(dir)?;
    for file in files {
        if let Some(path) = matches_filename(file.ok(), &file_hash) {
            let file = File::open(path)?;
            let reader = BufReader::new(file);
            let receipt: Receipt = serde_json::from_reader(reader)?;
            println!("Found cached payment for {file_name}");
            return Ok(Some(receipt));
        }
    }

    Ok(None)
}

/// Cleanup outdated cached payments.
fn cleanup_outdated_payments() -> Result<()> {
    let dir = get_payments_dir()?;
    let files = std::fs::read_dir(dir)?;
    let expired_files = files.into_iter().filter_map(|file| {
        let path = file.ok()?.path();
        if is_expired_file(path.to_str()?) {
            Some(path)
        } else {
            None
        }
    });
    for file in expired_files {
        println!("Removing expired cached payment file: {}", file.display());
        std::fs::remove_file(file)?;
    }
    Ok(())
}

fn matches_filename(file: Option<DirEntry>, file_hash: &str) -> Option<PathBuf> {
    let path = file?.path();
    if !path.is_file() {
        return None;
    }
    let file_name = path.file_name()?;
    let file_name = file_name.to_str()?;
    if file_name.contains(file_hash) {
        Some(path)
    } else {
        None
    }
}

/// if filename is longer than 32 characters or is a path, use the hash instead
fn filename_short(filename: &str) -> String {
    if filename.len() > 32 || filename.contains("/") || filename.contains("\\") {
        sha256::digest(filename)
    } else {
        filename.to_string()
    }
}

fn is_expired_file(filename: &str) -> bool {
    let exp = PAYMENT_EXPIRATION_SECS;
    let expired_if_before = SystemTime::now() - Duration::from_secs(exp);

    let timestr = filename.split('_').next().unwrap_or_default();
    let sec = timestr.parse::<u64>().unwrap_or_default();
    let timestamp = SystemTime::UNIX_EPOCH + Duration::from_secs(sec);
    timestamp < expired_if_before
}

fn now() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    timestamp.to_string()
}

fn get_timestamp_from_receipt(receipt: &Receipt) -> String {
    if let Some((proof, _)) = receipt.values().next() {
        if let Some(timestamp) = proof
            .peer_quotes
            .first()
            .map(|(_, _, quote)| quote.timestamp)
        {
            return timestamp
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .to_string();
        }
    }

    now()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_expired_filename() {
        let just_expired = (SystemTime::now() - Duration::from_secs(PAYMENT_EXPIRATION_SECS))
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string();
        let just_expired_1 = (SystemTime::now() - Duration::from_secs(PAYMENT_EXPIRATION_SECS + 1))
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string();
        let not_expired = now();
        let not_expired_1 = (SystemTime::now() + Duration::from_secs(PAYMENT_EXPIRATION_SECS - 1))
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string();

        let file_hash = filename_short("test");
        assert!(is_expired_file(&format!("{just_expired}_{file_hash}")));
        assert!(is_expired_file(&format!("{just_expired_1}_{file_hash}")));
        assert!(!is_expired_file(&format!("{not_expired}_{file_hash}")));
        assert!(!is_expired_file(&format!("{not_expired_1}_{file_hash}")));
    }
}
