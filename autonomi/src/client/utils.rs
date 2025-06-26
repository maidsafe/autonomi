// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::client::PutError;
use futures::stream::{FuturesUnordered, StreamExt};
use libp2p::PeerId;
use std::future::Future;

pub(crate) async fn process_tasks_with_max_concurrency<I, R>(tasks: I, batch_size: usize) -> Vec<R>
where
    I: IntoIterator,
    I::Item: Future<Output = R> + Send,
    R: Send,
{
    let mut futures = FuturesUnordered::new();
    let mut results = Vec::new();

    for task in tasks.into_iter() {
        futures.push(task);

        if futures.len() >= batch_size {
            if let Some(result) = futures.next().await {
                results.push(result);
            }
        }
    }

    // Process remaining tasks
    while let Some(result) = futures.next().await {
        results.push(result);
    }

    results
}

pub(crate) async fn process_request_tasks_expect_majority_succeeds<I>(
    total_tasks: usize,
    tasks: I,
) -> Result<(), PutError>
where
    I: IntoIterator,
    I::Item: Future<Output = Result<Option<PeerId>, PutError>> + Send,
{
    let mut tasks_results = process_tasks_with_max_concurrency(tasks, total_tasks).await;

    // return error only when not having enough OK responses.
    tasks_results.retain(|res| !res.is_ok());
    if tasks_results.len() > total_tasks / 2 {
        // Just return the first error
        #[allow(clippy::question_mark)]
        if let Err(err) = tasks_results.remove(0) {
            return Err(err);
        }
    }
    Ok(())
}
