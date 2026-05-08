//! Error type and `Result` alias for `prepare-devenv`.
//!
//! All fallible operations in this crate return [`Result<T>`] which aliases
//! `std::result::Result<T, Error>`. The variants here cover the failure modes
//! enumerated in the design plan (discovery, capture, shell selection,
//! spawning, and the IO/JSON funnel variants used with `?`).

use std::io;
use std::path::PathBuf;

/// All errors produced by `prepare-devenv`.
///
/// Display strings are user-facing and intended to be printed straight to
/// stderr (typically via `anyhow`'s formatted chain). The wording is chosen to
/// be concise and actionable; later tasks may refine the exact phrasing
/// against the specs, but the shape and `#[from]` / `#[source]` plumbing are
/// fixed by Task 2.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// `vswhere.exe` could not be located in the standard installer path or on `%PATH%`.
    #[error(
        "vswhere.exe not found at %ProgramFiles(x86)%\\Microsoft Visual Studio\\Installer\\ or on %PATH%; install Visual Studio (2017+) or set --path explicitly"
    )]
    VsWhereMissing,

    /// `vswhere.exe` ran but returned a non-zero exit status.
    #[error("vswhere.exe failed with exit code {code}: {stderr}")]
    VsWhereFailed { code: i32, stderr: String },

    /// `vswhere.exe` ran successfully but reported zero installations.
    #[error("no Visual Studio installations found")]
    NoInstalls,

    /// The selector (e.g. `--id`, `--path`) did not match any installation.
    #[error("no Visual Studio installation matched {kind}={value}")]
    NoMatch { kind: &'static str, value: String },

    /// An `--id` prefix matched more than one installation.
    #[error("instance id prefix '{0}' is ambiguous; supply more characters to disambiguate")]
    AmbiguousId(String),

    /// `VsDevCmd.bat` is missing under the resolved installation path.
    #[error("VsDevCmd.bat not found at {0}")]
    VsDevCmdMissing(PathBuf),

    /// `VsDevCmd.bat` ran but returned a non-zero exit status.
    #[error("VsDevCmd.bat failed with exit code {0}")]
    VsDevCmdFailed(i32),

    /// The captured environment block could not be parsed (missing marker, malformed line, etc.).
    #[error("failed to parse captured environment: {0}")]
    EnvParse(&'static str),

    /// `--shell` was given a value not recognised by `prepare-devenv`.
    #[error("unknown shell '{0}'")]
    UnknownShell(String),

    /// Failed to spawn a child process. The `source` field carries the underlying `io::Error`.
    #[error("failed to spawn '{cmd}'")]
    Spawn {
        cmd: String,
        #[source]
        source: io::Error,
    },

    /// IO error funnel for `?` propagation.
    #[error(transparent)]
    Io(#[from] io::Error),

    /// JSON deserialisation error funnel for `?` propagation.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Crate-wide `Result` alias bound to [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vs_where_missing_message_format() {
        let msg = format!("{}", Error::VsWhereMissing);
        assert_eq!(
            msg,
            "vswhere.exe not found at %ProgramFiles(x86)%\\Microsoft Visual Studio\\Installer\\ or on %PATH%; install Visual Studio (2017+) or set --path explicitly"
        );
    }

    #[test]
    fn vs_where_failed_message_handles_empty_stderr() {
        // `run_vswhere` substitutes "(no diagnostic output)" when vswhere
        // exits non-zero with empty stderr, so the rendered error never has a
        // trailing colon-space-nothing. We exercise both the substitution
        // behaviour (the empty-stderr placeholder) and confirm the message
        // doesn't end with `: ` regardless of which side is empty.
        let err = Error::VsWhereFailed {
            code: 1,
            stderr: "(no diagnostic output)".to_string(),
        };
        let msg = format!("{err}");
        assert!(
            !msg.ends_with(": "),
            "rendered message should not end with `: `, got {msg:?}"
        );
        assert!(
            msg.contains("(no diagnostic output)"),
            "message should surface the placeholder, got {msg:?}"
        );
        assert!(msg.contains("exit code 1"), "got {msg:?}");
    }

    #[test]
    fn spawn_error_chain_walks_into_io_source() {
        let err = Error::Spawn {
            cmd: "x".into(),
            source: io::Error::new(io::ErrorKind::NotFound, "no"),
        };
        let any: anyhow::Error = err.into();
        let chain: Vec<&(dyn std::error::Error + 'static)> = any.chain().collect();
        assert!(
            chain.len() >= 2,
            "expected anyhow chain to include the inner io::Error source, got {} entries",
            chain.len()
        );
        // Sanity-check: the leaf carries the io::Error message.
        let leaf = chain.last().expect("chain non-empty");
        assert!(
            leaf.to_string().contains("no"),
            "leaf error should be the io::Error, got: {leaf}"
        );
    }
}
