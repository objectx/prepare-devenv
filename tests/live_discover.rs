//! Live integration test: real `vswhere.exe` + real `VsDevCmd.bat`.
//!
//! Gated behind the `live` Cargo feature (default off) because it requires
//! a working Visual Studio install on the host machine. CI workflows that
//! lack VS skip this file entirely; only the manually-triggered live job
//! on a self-hosted Windows-with-VS runner exercises it.
//!
//! Integration tests for a binary crate cannot import the binary's
//! private modules directly (they're separate compilation units), so we
//! exercise discovery + capture by running the real binary via
//! `assert_cmd` and asserting against its CLI surface.

#![cfg(feature = "live")]

use assert_cmd::Command;

/// With no `--id` / `--path`, the discovery selector defaults to `Latest`,
/// which probes `vswhere.exe`, parses its JSON output, and locates the
/// associated `VsDevCmd.bat`. If any of that fails the binary exits with
/// code 3 (discovery error). A successful `--emit` run therefore proves
/// the full discovery -> capture -> diff pipeline reached stdout.
#[test]
fn resolve_latest_install_runs_and_emit_succeeds() {
    let output = Command::cargo_bin("prepare-devenv")
        .unwrap()
        .args(["--emit", "--shell", "cmd"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "expected a real VS install to be discoverable on this machine; \
         got exit {:?}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    // The diff is guaranteed to add or modify VS env vars, so cmd-shell
    // emit output must contain at least one `set "..."` line.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("set \""),
        "expected emit output containing `set \"...\"` lines, got:\n{stdout}"
    );
}
