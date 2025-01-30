// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use autonomi::{client::Amount, ClientEvent};
use serde::Serialize;
use serde::Serializer;
use std::time::SystemTime;

#[derive(Debug, Clone, Serialize, Default)]
pub(crate) struct TaskSummary {
    pub(crate) records_paid: usize,
    pub(crate) records_already_paid: usize,
    #[serde(serialize_with = "serialize_amount_as_string")]
    pub(crate) tokens_spent: Amount,
    pub(crate) records_uploaded: usize,
    pub(crate) records_upload_failed: usize,
    pub(crate) records_downloaded: usize,
    pub(crate) records_download_failed: usize,
}

// Custom serializer function
fn serialize_amount_as_string<S>(amount: &Amount, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&amount.to_string())
}

/// Collects task summary from the event receiver.
/// Send a signal to the returned sender to stop collecting and to return the result via the join handle.
pub(crate) fn collect_task_summary(
    mut event_receiver: tokio::sync::mpsc::Receiver<ClientEvent>,
) -> (
    tokio::task::JoinHandle<TaskSummary>,
    tokio::sync::oneshot::Sender<()>,
) {
    let (upload_completed_tx, mut upload_completed_rx) = tokio::sync::oneshot::channel::<()>();
    let stats_thread = tokio::spawn(async move {
        let mut task_summary = TaskSummary::default();

        loop {
            tokio::select! {
                event = event_receiver.recv() => {
                    match event {
                        Some(client_event) => {
                            handle_client_events(&mut task_summary, client_event);
                        }
                        None => break,
                    }
                }
                _ = &mut upload_completed_rx => break,
            }
        }

        // try to drain the event receiver in case there are any more events
        while let Ok(event) = event_receiver.try_recv() {
            handle_client_events(&mut task_summary, event);
        }

        task_summary
    });

    (stats_thread, upload_completed_tx)
}

fn handle_client_events(task_summary: &mut TaskSummary, event: ClientEvent) {
    match event {
        ClientEvent::PaymentSucceeded(payment_summary) => {
            task_summary.tokens_spent += payment_summary.tokens_spent;
            task_summary.records_paid += payment_summary.records_paid;
            task_summary.records_already_paid += payment_summary.records_already_paid;
        }
        ClientEvent::UploadSucceeded(_) => {
            task_summary.records_uploaded += 1;
        }
        ClientEvent::DataAlreadyPresent(_) => {
            task_summary.records_already_paid += 1;
        }
        ClientEvent::UploadFailed(_) => {
            task_summary.records_upload_failed += 1;
        }
        ClientEvent::DownloadSucceeded(_) => {
            task_summary.records_downloaded += 1;
        }
        ClientEvent::DownloadFailed(_) => {
            task_summary.records_download_failed += 1;
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FileUploadOutput {
    pub(crate) exit_code: i32,
    pub(crate) file: String,
    pub(crate) task_summary: TaskSummary,
    pub(crate) start_time: SystemTime,
    pub(crate) end_time: SystemTime,
    pub(crate) uploaded_address: Option<String>,
    pub(crate) public: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FileDownloadOutput {
    pub(crate) exit_code: i32,
    pub(crate) addr: String,
    pub(crate) public: bool,
    pub(crate) task_summary: TaskSummary,
    pub(crate) start_time: SystemTime,
    pub(crate) end_time: SystemTime,
    pub(crate) failed_files: Vec<(String, String)>,
}

impl FileUploadOutput {
    pub(crate) fn print(&self) {
        info!("Upload completed: {self:?}");

        if self.exit_code != 0 {
            println!("Failed to upload file.");
            println!(
                "Number of chunks paid for: {}",
                self.task_summary.records_paid
            );
            println!("Cost: {} AttoTokens", self.task_summary.tokens_spent);
        } else if self.task_summary.records_paid == 0 {
            println!("All chunks already exist on the network.");
        } else {
            println!("Successfully uploaded: {}", self.file);
            if let Some(local_addr) = &self.uploaded_address {
                {
                    println!("At address: {local_addr}");
                }
                println!(
                    "Number of chunks uploaded: {}",
                    self.task_summary.records_paid
                );
                println!(
                    "Number of chunks already paid/uploaded: {}",
                    self.task_summary.records_already_paid
                );
                println!("Total cost: {} AttoTokens", self.task_summary.tokens_spent);
            }
        }
    }

    pub(crate) fn print_json(&self) {
        let json =
            serde_json::to_string_pretty(&self).expect("Failed to serialize UploadOutput to JSON");
        eprintln!("{json}");
        info!("JSON output: {json}");
    }
}

impl FileDownloadOutput {
    pub(crate) fn print(&self) {
        info!("Download completed: {self:?}");

        if self.exit_code != 0 {
            let err_no = self.failed_files.len();
            if self.public {
                println!("{err_no} errors while downloading data at: {}", self.addr);
                println!("{:#?}", self.failed_files);
                error!(
                    "Errors while downloading data at {}: {:#?}",
                    self.addr, self.failed_files
                );
            } else if err_no == 0 {
                println!(
                    "Failed to download private data with local address: {}",
                    self.addr
                );
            } else {
                println!(
                    "{err_no} errors while downloading private data with local address: {}",
                    self.addr
                );
                println!("{:#?}", self.failed_files);
            }
        } else if self.public {
            println!("Successfully downloaded data at: {}", self.addr);
        } else {
            println!(
                "Successfully downloaded private data with local address: {}",
                self.addr
            );
        }
    }

    pub(crate) fn print_json(&self) {
        let json = serde_json::to_string_pretty(&self)
            .expect("Failed to serialize DownloadOutput to JSON");
        eprintln!("{json}");
        info!("JSON output: {json}");
    }
}
