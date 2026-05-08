//! Command-line argument parsing for `prepare-devenv`.
//!
//! All CLI surface lives in this module: the [`Cli`] struct (parsed by
//! `clap-derive`), the [`ShellKind`] enum exposed via `--shell`, and the
//! [`Mode`] enum returned by [`Cli::mode`] which the orchestration layer
//! (Task 9) dispatches on.
//!
//! The parser intentionally rejects mutually-exclusive combinations
//! (`--id` with `--path`, `--emit` with a trailing `-- COMMAND`) at parse
//! time via clap's `conflicts_with`, so the dispatcher only has to
//! distinguish the three legitimate operating modes.

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Parser, ValueEnum};

/// Shells supported by the `--shell` flag and the shell-adapter suite.
///
/// The variant names match clap's default kebab-case rendering, so
/// `--shell cmd|pwsh|bash|fish|nu` parses without further annotations.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum ShellKind {
    /// Windows `cmd.exe`.
    Cmd,
    /// PowerShell 7+ (`pwsh.exe`); spawn-shell mode falls back to `powershell.exe`.
    Pwsh,
    /// Bash (typically Git-Bash or WSL `bash.exe`).
    Bash,
    /// Fish shell.
    Fish,
    /// Nushell.
    Nu,
}

// Parsed command-line invocation.
//
// Fields map 1:1 to documented flags in `specs/cli-surface-and-diagnostics`.
// Mutually-exclusive flag pairs are enforced via `conflicts_with`; the trailing
// `-- COMMAND` argv is collected into `command` via `last = true`.
//
// Note: this is a non-doc comment deliberately. clap-derive uses the rustdoc
// of `Cli` as `--help`'s long-about, and we don't want this implementation
// note showing up to end-users. The user-visible description lives in the
// `#[command(about = ...)]` attribute below.
#[derive(Parser, Debug)]
#[command(
    name = "prepare-devenv",
    about = "Bridge a Visual Studio developer environment into another shell or process"
)]
pub struct Cli {
    /// Visual Studio instance id (prefix match) selecting which install to use.
    ///
    /// Mutually exclusive with `--path`.
    #[arg(long, conflicts_with = "path")]
    pub id: Option<String>,

    /// Visual Studio installation path selecting which install to use.
    ///
    /// Mutually exclusive with `--id`.
    #[arg(long, conflicts_with = "id")]
    pub path: Option<PathBuf>,

    /// Single string of arguments forwarded verbatim to `VsDevCmd.bat`.
    #[arg(long)]
    pub devcmd_args: Option<String>,

    /// Print the captured env delta as shell-specific `set` lines on stdout
    /// instead of spawning a shell or running a command.
    ///
    /// Mutually exclusive with the trailing `-- COMMAND` argv.
    #[arg(long, conflicts_with = "command")]
    pub emit: bool,

    /// Override the auto-detected target shell for `--emit` and spawn-shell modes.
    ///
    /// Silently ignored in spawn-user-command mode.
    #[arg(long, value_enum)]
    pub shell: Option<ShellKind>,

    /// Increase log verbosity (`-v` for info, `-vv` for debug). Repeatable.
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Trailing command argv collected after `--`.
    ///
    /// When non-empty the tool runs in spawn-user-command mode.
    #[arg(last = true, value_parser = clap::value_parser!(OsString))]
    pub command: Vec<OsString>,

    /// Test-only override pointing at a fake `VsDevCmd.bat` script.
    ///
    /// Hidden from `--help`; only present when the crate is built with the
    /// `test_hooks` Cargo feature so end-to-end tests can drive the
    /// pipeline without an installed Visual Studio.
    #[cfg(feature = "test_hooks")]
    #[arg(long, hide = true)]
    pub devcmd_script: Option<PathBuf>,
}

/// Operating mode dispatched on by the orchestration layer.
///
/// The mapping from a parsed [`Cli`] is computed by [`Cli::mode`] and is
/// the single source of truth for "what does this invocation do".
#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    /// No `--emit` and no trailing argv: spawn the user's shell with the
    /// merged environment so subsequent commands inherit the VS dev vars.
    SpawnShell,
    /// `--emit`: print shell-specific `set` lines to stdout.
    Emit,
    /// `-- COMMAND [ARGS...]`: spawn the given command with the merged
    /// environment and proxy its exit code.
    RunCommand(Vec<OsString>),
}

impl Cli {
    /// Resolve the parsed flags into the operating [`Mode`].
    ///
    /// Trailing argv wins over `--emit` (clap's `conflicts_with` on
    /// `emit` already prevents both being set together; the ordering
    /// here is defensive).
    #[must_use]
    pub fn mode(&self) -> Mode {
        if !self.command.is_empty() {
            Mode::RunCommand(self.command.clone())
        } else if self.emit {
            Mode::Emit
        } else {
            Mode::SpawnShell
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    #[test]
    fn no_args_is_spawn_shell() {
        let cli = Cli::try_parse_from(["prepare-devenv"]).expect("no args parses");
        assert_eq!(cli.mode(), Mode::SpawnShell);
    }

    #[test]
    fn emit_flag_is_emit_mode() {
        let cli = Cli::try_parse_from(["prepare-devenv", "--emit"]).expect("--emit parses");
        assert_eq!(cli.mode(), Mode::Emit);
    }

    #[test]
    fn trailing_argv_is_run_command() {
        let cli = Cli::try_parse_from(["prepare-devenv", "--", "bash"]).expect("-- bash parses");
        match cli.mode() {
            Mode::RunCommand(argv) => {
                assert_eq!(argv, vec![OsString::from("bash")]);
            }
            other => panic!("expected Mode::RunCommand, got {other:?}"),
        }
    }

    #[test]
    fn trailing_argv_with_flags_is_preserved() {
        let cli = Cli::try_parse_from(["prepare-devenv", "--", "bash", "-c", "cl /?"])
            .expect("trailing argv with flags parses");
        match cli.mode() {
            Mode::RunCommand(argv) => {
                assert_eq!(
                    argv,
                    vec![
                        OsString::from("bash"),
                        OsString::from("-c"),
                        OsString::from("cl /?"),
                    ]
                );
            }
            other => panic!("expected Mode::RunCommand, got {other:?}"),
        }
    }

    #[test]
    fn shell_kind_parses() {
        let cli = Cli::try_parse_from(["prepare-devenv", "--shell", "bash"])
            .expect("--shell bash parses");
        assert_eq!(cli.shell, Some(ShellKind::Bash));
    }

    #[test]
    fn verbose_counts() {
        let cli = Cli::try_parse_from(["prepare-devenv", "-vv"]).expect("-vv parses");
        assert_eq!(cli.verbose, 2);
    }

    #[test]
    fn id_and_path_are_mutually_exclusive() {
        let result = Cli::try_parse_from(["prepare-devenv", "--id", "X", "--path", "Y"]);
        let err = result.expect_err("expected clap to reject --id with --path");
        assert_eq!(err.kind(), ErrorKind::ArgumentConflict);
    }

    #[test]
    fn emit_and_trailing_command_are_mutually_exclusive() {
        let result = Cli::try_parse_from(["prepare-devenv", "--emit", "--", "foo"]);
        let err = result.expect_err("expected clap to reject --emit with -- COMMAND");
        assert_eq!(err.kind(), ErrorKind::ArgumentConflict);
    }

    #[test]
    fn unknown_shell_value_is_rejected() {
        let result = Cli::try_parse_from(["prepare-devenv", "--shell", "zsh"]);
        let err = result.expect_err("expected clap to reject --shell zsh");
        assert_eq!(err.kind(), ErrorKind::InvalidValue);
    }
}
