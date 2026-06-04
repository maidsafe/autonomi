//! Conditional logging macros that compile to nothing in release builds.
//!
//! These macros wrap `tracing` and are gated behind `cfg(debug_assertions)`,
//! ensuring that no tracing callsite metadata (module paths, file paths,
//! field names) is present in release binaries.

/// Emit a TRACE-level event (debug builds only).
#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {{
        #[cfg(debug_assertions)]
        { tracing::trace!($($arg)*) }
    }};
}

/// Emit a DEBUG-level event (debug builds only).
#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {{
        #[cfg(debug_assertions)]
        { tracing::debug!($($arg)*) }
    }};
}

/// Emit an INFO-level event (debug builds only).
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {{
        #[cfg(debug_assertions)]
        { tracing::info!($($arg)*) }
    }};
}

/// Emit a WARN-level event (debug builds only).
#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {{
        #[cfg(debug_assertions)]
        { tracing::warn!($($arg)*) }
    }};
}

/// Emit an ERROR-level event (debug builds only).
#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {{
        #[cfg(debug_assertions)]
        { tracing::error!($($arg)*) }
    }};
}
