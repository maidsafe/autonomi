// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use file_rotate::{
    compression::Compression,
    suffix::{AppendTimestamp, FileLimit},
    ContentLimit, FileRotate,
};
use std::{
    env,
    ffi::OsStr,
    fmt::Debug,
    io,
    io::Write,
    path::{Path, PathBuf},
};
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};

/// max_bytes:
/// - the maximum size a log can grow to until it is rotated.
///
/// uncompressed_files:
/// - number of files to keep uncompressed.
/// - should be lesser than `max_files` to enable compression of excess files.
///
/// max_files:
/// - maximum number of files to keep.
/// - older files are deleted.
pub(super) fn file_rotater(
    dir: &PathBuf,
    max_bytes: usize,
    uncompressed_files: usize,
    max_files: usize,
) -> (NonBlocking, WorkerGuard) {
    let binary_name = env::current_exe()
        .map(|path| {
            path.file_stem()
                .unwrap_or(OsStr::new("autonomi"))
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_else(|_| "autonomi".to_string());

    let file_appender = FileRotateAppender::make_rotate_appender(
        dir,
        format!("{binary_name}.log"),
        AppendTimestamp::default(FileLimit::MaxFiles(max_files)),
        ContentLimit::BytesSurpassed(max_bytes),
        Compression::OnRotate(uncompressed_files),
    );

    // configure how tracing non-blocking works: https://tracing.rs/tracing_appender/non_blocking/struct.nonblockingbuilder#method.default
    let non_blocking_builder = tracing_appender::non_blocking::NonBlockingBuilder::default();

    non_blocking_builder
        // lose lines and keep perf, or exert backpressure?
        .lossy(false)
        // optionally change buffered lines limit
        // .buffered_lines_limit(buffered_lines_limit)
        .finish(file_appender)
}

/// `FileRotateAppender` is a `tracing_appender` with extra logrotate features:
///  - most recent logfile name re-used to support following (e.g. 'tail -f=logfile')
///  - numbered rotation (logfile.1, logfile.2 etc)
///  - limit logfile by size, lines or time
///  - limit maximum number of logfiles
///  - optional compression of rotated logfiles
//
// The above functionality is provided using crate file_rotation
pub(super) struct FileRotateAppender {
    writer: FileRotate<AppendTimestamp>,
}

impl FileRotateAppender {
    /// Create `FileRotateAppender` using parameters
    pub(super) fn make_rotate_appender(
        directory: impl AsRef<Path>,
        file_name_prefix: impl AsRef<Path>,
        file_limit: AppendTimestamp,
        max_log_size: ContentLimit,
        compression: Compression,
    ) -> Self {
        let log_directory = directory.as_ref();
        let log_filename_prefix = file_name_prefix.as_ref();
        let path = Path::new(&log_directory).join(log_filename_prefix);
        let writer = FileRotate::new(
            Path::new(&path),
            file_limit,
            max_log_size,
            compression,
            #[cfg(unix)]
            None,
        );

        Self { writer }
    }
}

impl Write for FileRotateAppender {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

use std::fmt;

impl Debug for FileRotateAppender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileRotateAppender").finish()
    }
}
