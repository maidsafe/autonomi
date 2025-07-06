// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use autonomi::client::{Amount, ClientEvent, UploadSummary};

/// Collects upload summary from the event receiver.
/// Send a signal to the returned sender to stop collecting and to return the result via the join handle.
pub fn collect_upload_summary(
    mut event_receiver: tokio::sync::mpsc::Receiver<ClientEvent>,
) -> (
    tokio::task::JoinHandle<UploadSummary>,
    tokio::sync::oneshot::Sender<()>,
) {
    let (upload_completed_tx, mut upload_completed_rx) = tokio::sync::oneshot::channel::<()>();
    let stats_thread = tokio::spawn(async move {
        let mut tokens_spent: Amount = Amount::from(0);
        let mut record_count = 0;
        let mut records_already_paid = 0;

        loop {
            tokio::select! {
                event = event_receiver.recv() => {
                    match event {
                        Some(ClientEvent::UploadComplete(upload_summary)) => {
                            tokens_spent += upload_summary.tokens_spent;
                            record_count += upload_summary.records_paid;
                            records_already_paid += upload_summary.records_already_paid;
                        }
                        None => break,
                    }
                }
                _ = &mut upload_completed_rx => break,
            }
        }

        // try to drain the event receiver in case there are any more events
        while let Ok(event) = event_receiver.try_recv() {
            match event {
                ClientEvent::UploadComplete(upload_summary) => {
                    tokens_spent += upload_summary.tokens_spent;
                    record_count += upload_summary.records_paid;
                    records_already_paid += upload_summary.records_already_paid;
                }
            }
        }

        UploadSummary {
            tokens_spent,
            records_paid: record_count,
            records_already_paid,
        }
    });

    (stats_thread, upload_completed_tx)
}
