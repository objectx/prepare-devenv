//! `prepare-devenv` — bridges Visual Studio's developer environment
//! into other shells and child processes.
//!
//! This binary is the orchestration layer: it parses the CLI, locates a
//! Visual Studio install, runs `VsDevCmd.bat` to capture the post-init
//! environment, computes the delta against the parent process's env, and
//! dispatches one of three output modes (emit / spawn-shell / run-command)
//! depending on the user's invocation.

#[cfg(not(windows))]
compile_error!("prepare-devenv targets Windows only");

mod capture;
mod cli;
mod diff;
mod discovery;
mod error;
mod runner;
mod shell;

use clap::Parser;

use crate::cli::Mode;
use crate::error::{Error, Result};

fn main() {
    let exit_code = match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            let mut source: Option<&dyn std::error::Error> = std::error::Error::source(&e);
            while let Some(err) = source {
                eprintln!("  caused by: {err}");
                source = err.source();
            }
            error_to_code(&e)
        }
    };
    std::process::exit(exit_code);
}

fn run() -> Result<i32> {
    let cli = cli::Cli::parse();
    init_tracing(cli.verbose);

    let install = resolve_install(&cli)?;
    tracing::info!(
        instance_id = %install.instance_id,
        version = %install.version,
        path = ?install.install_path,
        "resolved Visual Studio install"
    );

    let pre = capture::snapshot_pre_env();
    // Per spec `cli-surface-and-diagnostics / Surface VsDevCmd.bat's stdout
    // only at -v or higher`: forward the captured stderr (where VsDevCmd.bat's
    // own chatter lives via the inner `1>&2`) to the user's stderr only when
    // -v or higher. On failure, capture::capture forwards unconditionally.
    let post = capture::capture(
        &install.vsdevcmd_path,
        cli.devcmd_args.as_deref(),
        cli.verbose >= 1,
    )?;
    let env_diff = diff::diff(&pre, &post);
    tracing::info!(
        added = env_diff.added.len(),
        modified = env_diff.modified.len(),
        "computed env diff"
    );

    let shell_kind = cli.shell.unwrap_or_else(shell::detect);
    let shell_adapter = shell::for_kind(shell_kind);

    match cli.mode() {
        Mode::Emit => {
            tracing::info!(?shell_kind, "emit mode");
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            runner::emit(&env_diff, shell_adapter.as_ref(), &mut handle)?;
            Ok(0)
        }
        Mode::SpawnShell => {
            tracing::info!(?shell_kind, "spawn-shell mode");
            runner::spawn_shell(&env_diff, &pre, shell_adapter.as_ref())
        }
        Mode::RunCommand(argv) => {
            tracing::info!(?argv, "run-command mode");
            runner::spawn_command(&env_diff, &pre, &argv)
        }
    }
}

fn init_tracing(verbose: u8) {
    use tracing_subscriber::{EnvFilter, fmt};
    let level = match verbose {
        0 => "error",
        1 => "info",
        _ => "debug",
    };
    let filter = EnvFilter::new(level);
    fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .compact()
        .init();
}

fn error_to_code(e: &Error) -> i32 {
    match e {
        Error::VsWhereMissing
        | Error::VsWhereFailed { .. }
        | Error::NoInstalls
        | Error::NoMatch { .. }
        | Error::AmbiguousId { .. }
        | Error::VsDevCmdMissing(_) => 3,
        Error::VsDevCmdFailed(_) => 4,
        _ => 1,
    }
}

fn resolve_install(cli: &cli::Cli) -> Result<discovery::ResolvedInstall> {
    #[cfg(feature = "test_hooks")]
    if let Some(script) = &cli.devcmd_script {
        tracing::warn!(
            ?script,
            "test_hooks: bypassing discovery, using --devcmd-script"
        );
        return Ok(discovery::ResolvedInstall {
            install_path: script
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .to_path_buf(),
            instance_id: "test-hook".into(),
            display_name: "TEST HOOK".into(),
            version: "0.0.0".into(),
            vsdevcmd_path: script.clone(),
        });
    }

    let selector = if let Some(id) = &cli.id {
        discovery::Selector::ById(id)
    } else if let Some(path) = &cli.path {
        discovery::Selector::ByPath(path)
    } else {
        discovery::Selector::Latest
    };
    discovery::resolve(selector)
}
