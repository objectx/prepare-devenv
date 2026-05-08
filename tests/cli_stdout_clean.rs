//! Stdout-cleanliness assertions for the binary's non-emit modes.
//!
//! Per the spec, `prepare-devenv` only writes to stdout when running
//! in `--emit` mode (one shell-formatted line per env entry). Every
//! other mode — spawn-shell and spawn-command — must leave stdout
//! untouched by the orchestration layer itself. Tracing output goes
//! to stderr, and the `-v`/`-vv` flags must not leak into stdout.
//!
//! These tests run the binary in spawn-command mode against the fake
//! `VsDevCmd.bat`, with a no-op user command (`cmd /c rem`), and
//! verify stdout is byte-empty.

#![cfg(feature = "test_hooks")]

use assert_cmd::Command;

const FAKE_BAT: &str = "tests/fixtures/devcmd-script/fake.bat";

#[test]
fn run_command_mode_produces_clean_stdout_when_command_is_silent() {
    // `cmd /c rem` is a no-op that produces no output; the only
    // possible stdout writer is `prepare-devenv` itself, and per the
    // contract it should write nothing in spawn-command mode.
    let output = Command::cargo_bin("prepare-devenv")
        .unwrap()
        .args(["--devcmd-script", FAKE_BAT, "--", "cmd", "/c", "rem"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "binary exited with {:?}, stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"",
        "stdout should be empty in spawn-command mode, got: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn run_command_mode_with_v_flag_keeps_stdout_clean() {
    // `-v` raises tracing-subscriber to info; the subscriber writes to
    // stderr (configured in `main::init_tracing`), so stdout must
    // still be empty even with the verbose flag set.
    let output = Command::cargo_bin("prepare-devenv")
        .unwrap()
        .args(["-v", "--devcmd-script", FAKE_BAT, "--", "cmd", "/c", "rem"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "binary exited with {:?}, stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"",
        "stdout should be empty even with -v, got: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn emit_with_v_flag_keeps_stdout_to_format_lines_only() {
    let output = assert_cmd::Command::cargo_bin("prepare-devenv")
        .unwrap()
        .args([
            "-v",
            "--emit",
            "--shell",
            "bash",
            "--devcmd-script",
            "tests/fixtures/devcmd-script/fake.bat",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "binary exited with {:?}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    // stdout should ONLY contain emit format lines (the bash fixture).
    // -v adds info lines to stderr, never stdout.
    let expected = std::fs::read("tests/fixtures/emit/bash.txt").unwrap();
    assert_eq!(output.stdout, expected, "stdout contains diagnostics");

    // stderr should NOT be empty under -v (info-level logs go there).
    assert!(
        !output.stderr.is_empty(),
        "expected info-level diagnostics on stderr under -v"
    );
}
