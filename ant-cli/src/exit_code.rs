// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#![allow(dead_code)]

use autonomi::{
    BootstrapError,
    client::{ConnectError, GetError, PutError, analyze::AnalysisError, payment::PayError},
    files::{DownloadError, UploadError},
};
use color_eyre::eyre::Report;

pub(crate) const INVALID_INPUT_EXIT_CODE: i32 = 6;
const SERIALIZATION_ERROR: i32 = 11;
pub const IO_ERROR: i32 = 12;
const NETWORK_ERROR: i32 = 13;
const PROTOCOL_ERROR: i32 = 14;
const SELF_ENCRYPTION_ERROR: i32 = 15;
pub const FEES_ERROR: i32 = 62;

pub type ExitCodeError = (Report, i32);

pub(crate) fn upload_exit_code(err: &UploadError) -> i32 {
    match err {
        UploadError::WalkDir(_) => IO_ERROR,
        UploadError::IoError(_) => IO_ERROR,
        UploadError::PutError(err) => put_error_exit_code(err),
        UploadError::Encryption(_) => SELF_ENCRYPTION_ERROR,
    }
}

pub(crate) fn download_exit_code(err: &DownloadError) -> i32 {
    match err {
        DownloadError::GetError(get_error) => get_error_exit_code(get_error),
        DownloadError::IoError(_) => IO_ERROR,
    }
}

pub(crate) fn pay_error_exit_code(err: &PayError) -> i32 {
    match err {
        PayError::EvmWalletNetworkMismatch => 21,
        PayError::EvmWalletError(_) => 22,
        PayError::SelfEncryption(_) => SELF_ENCRYPTION_ERROR,
        PayError::Cost(_) => 23,
    }
}

pub(crate) fn get_error_exit_code(err: &GetError) -> i32 {
    match err {
        GetError::InvalidDataMap(_) => 31,
        GetError::Decryption(_) => 32,
        GetError::Deserialization(_) => SERIALIZATION_ERROR,
        GetError::Network(_) => NETWORK_ERROR,
        GetError::Protocol(_) => PROTOCOL_ERROR,
        GetError::RecordNotFound => 33,
        GetError::RecordKindMismatch(_) => 34,
        GetError::Configuration(_) => 35,
        GetError::UnrecognizedDataMap(_) => 31,
        GetError::TooLargeForMemory => 31,
    }
}

pub(crate) fn get_download_error_exit_code(err: &autonomi::files::DownloadError) -> i32 {
    match err {
        autonomi::files::DownloadError::GetError(ge) => get_error_exit_code(ge),
        autonomi::files::DownloadError::IoError(_) => IO_ERROR,
    }
}

pub(crate) fn analysis_exit_code(err: &AnalysisError) -> i32 {
    match err {
        AnalysisError::UnrecognizedInput => 36,
        AnalysisError::GetError(get_error) => get_error_exit_code(get_error),
        AnalysisError::FailedGet => 37,
    }
}

pub(crate) fn put_error_exit_code(err: &PutError) -> i32 {
    match err {
        PutError::SelfEncryption(_) => SELF_ENCRYPTION_ERROR,
        PutError::Network { .. } => NETWORK_ERROR,
        PutError::CostError(_) => 41,
        PutError::PayError(pay_error) => pay_error_exit_code(pay_error),
        PutError::Serialization(_) => SERIALIZATION_ERROR,
        PutError::Wallet(_) => 42,
        PutError::Batch(_) => 44,
        PutError::PayeesMissing => 45,
    }
}

pub(crate) fn bootstrap_error_exit_code(err: &BootstrapError) -> i32 {
    match err {
        BootstrapError::NoBootstrapPeersFound => 51,
        BootstrapError::FailedToParseCacheData => 52,
        BootstrapError::CouldNotObtainDataDir => 53,
        BootstrapError::InvalidBootstrapCacheDir => 53,
        BootstrapError::FailedToObtainAddrsFromUrl(_, _) => 54,
        BootstrapError::FailedToParseUrl => 55,
        BootstrapError::Io(_) => IO_ERROR,
        BootstrapError::Json(_) => 56,
        BootstrapError::Http(_) => 57,
        BootstrapError::LockError => 58,
    }
}

pub(crate) fn connect_error_exit_code(err: &ConnectError) -> i32 {
    match err {
        ConnectError::EvmNetworkError(_) => 61,
        ConnectError::Bootstrap(error) => bootstrap_error_exit_code(error),
        ConnectError::TimedOut => 59,
        ConnectError::TimedOutWithIncompatibleProtocol(_, _) => 60,
        ConnectError::NoKnownPeers(_) => 51, // todo: uses duplicate exit code from `BootstrapError::NoBootstrapPeersFound`
    }
}

pub(crate) fn evm_util_error_exit_code(err: &autonomi::EvmUtilError) -> i32 {
    match err {
        autonomi::EvmUtilError::FailedToGetEvmNetwork(_) => 61,
    }
}
