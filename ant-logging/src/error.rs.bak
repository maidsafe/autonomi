// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use thiserror::Error;

pub(super) type Result<T, E = Error> = std::result::Result<T, E>;
/// Internal error.
#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    ReloadError(#[from] tracing_subscriber::reload::Error),

    #[cfg(feature = "otlp")]
    #[error("OpenTelemetry Tracing error: {0}")]
    OpenTelemetryTracing(#[from] opentelemetry::trace::TraceError),

    #[error("Could not configure logging: {0}")]
    LoggingConfiguration(String),
}
