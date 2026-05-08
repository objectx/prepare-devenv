//! End-to-end tests for `--emit` mode.
//!
//! Each test runs the binary against the fake `VsDevCmd.bat` under
//! `tests/fixtures/devcmd-script/fake.bat` (which sets three known env
//! vars: `FAKE_INCLUDE`, `FAKE_LIB`, `FAKE_VCINSTALLDIR`) and compares
//! the stdout against the per-shell fixture under `tests/fixtures/emit/`.
//!
//! Requires the `test_hooks` Cargo feature so the hidden
//! `--devcmd-script` flag is available.

#![cfg(feature = "test_hooks")]

use std::path::Path;

use assert_cmd::Command;

fn fake_bat() -> &'static Path {
    Path::new("tests/fixtures/devcmd-script/fake.bat")
}

fn emit_for_shell(kind: &str) -> Vec<u8> {
    let output = Command::cargo_bin("prepare-devenv")
        .unwrap()
        .args(["--emit", "--shell", kind, "--devcmd-script"])
        .arg(fake_bat())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "binary exited with {:?}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn assert_emit_matches(kind: &str, fixture: &str) {
    let actual = emit_for_shell(kind);
    let expected =
        std::fs::read(fixture).unwrap_or_else(|e| panic!("failed to read {fixture}: {e}"));
    assert_eq!(
        String::from_utf8_lossy(&actual),
        String::from_utf8_lossy(&expected),
        "{kind} emit output diverged from fixture {fixture}"
    );
}

#[test]
fn emit_cmd_matches_fixture() {
    assert_emit_matches("cmd", "tests/fixtures/emit/cmd.txt");
}

#[test]
fn emit_pwsh_matches_fixture() {
    assert_emit_matches("pwsh", "tests/fixtures/emit/pwsh.txt");
}

#[test]
fn emit_bash_matches_fixture() {
    assert_emit_matches("bash", "tests/fixtures/emit/bash.txt");
}

#[test]
fn emit_fish_matches_fixture() {
    assert_emit_matches("fish", "tests/fixtures/emit/fish.txt");
}

#[test]
fn emit_nu_matches_fixture() {
    assert_emit_matches("nu", "tests/fixtures/emit/nu.txt");
}
