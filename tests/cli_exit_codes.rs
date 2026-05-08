//! Exit-code matrix for the binary, exercising the documented mapping
//! from operational outcomes to process exit status.
//!
//! Mapping (per `specs/cli-surface-and-diagnostics`):
//!
//! - `0` — clean success (emit / spawn-shell / spawn-command)
//! - `1` — generic failure (IO, EnvParse, Spawn — not exercised here
//!   because triggering them deterministically requires fixtures we
//!   don't yet keep; covered by unit tests in `runner` and `capture`)
//! - `2` — clap / CLI parse error (mutually-exclusive flags, unknown
//!   shell, etc.)
//! - `3` — discovery failure (`VsWhereMissing`, `NoMatch`, `AmbiguousId`,
//!   `VsDevCmdMissing`)
//! - `4` — `VsDevCmd.bat` ran but returned non-zero
//! - `N` — pass-through exit code from the user-supplied command in
//!   `-- COMMAND` mode
//!
//! The exit-3 path via `--id <bogus>` is intentionally NOT exercised
//! here: the `test_hooks` branch in `main::resolve_install` bypasses
//! discovery entirely once `--devcmd-script` is set, so `--id` is
//! ignored. Driving the real discovery path would require a live
//! `vswhere.exe` on PATH, making the test environment-dependent. That
//! coverage will land alongside live tests under the `live` Cargo
//! feature.

#![cfg(feature = "test_hooks")]

use assert_cmd::Command;

/// Path to the fake `VsDevCmd.bat` shared with the other end-to-end tests.
const FAKE_BAT: &str = "tests/fixtures/devcmd-script/fake.bat";

#[test]
fn happy_path_emit_exits_0() {
    let output = Command::cargo_bin("prepare-devenv")
        .unwrap()
        .args(["--emit", "--devcmd-script", FAKE_BAT])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn id_and_path_conflict_yields_exit_2() {
    // clap's `conflicts_with` rejects `--id` together with `--path`
    // before any of our code runs; clap exits 2 by convention.
    let output = Command::cargo_bin("prepare-devenv")
        .unwrap()
        .args(["--id", "X", "--path", "Y"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn unknown_shell_yields_exit_2() {
    // Unknown `--shell` value is rejected by clap's `ValueEnum`.
    let output = Command::cargo_bin("prepare-devenv")
        .unwrap()
        .args(["--emit", "--shell", "zsh", "--devcmd-script", FAKE_BAT])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn vsdevcmd_failing_with_exit_7_yields_exit_4() {
    // The fake bat's `--exit-code N` short-circuit returns N. Our cmd
    // wrapper's `&&` chain propagates that to the inner cmd's exit code,
    // which `capture::capture` surfaces as `Error::VsDevCmdFailed(7)`.
    // `main::error_to_code` maps that variant to exit 4.
    let output = Command::cargo_bin("prepare-devenv")
        .unwrap()
        .args([
            "--emit",
            "--devcmd-args=--exit-code 7",
            "--devcmd-script",
            FAKE_BAT,
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(4),
        "expected exit 4, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn nonexistent_devcmd_script_yields_exit_4() {
    // The `test_hooks` branch in `main::resolve_install` does NOT verify
    // that the supplied path exists: it just hands the path to
    // `capture::capture`. The inner `cmd /c "call <bad-path> ..."` then
    // fails — cmd reports exit 1 — and that surfaces as
    // `Error::VsDevCmdFailed(1)`, which maps to exit 4.
    //
    // This is a slightly indirect way to test "missing bat" behaviour,
    // but it's deterministic and doesn't require a real VS install to
    // exercise the failure path.
    let output = Command::cargo_bin("prepare-devenv")
        .unwrap()
        .args([
            "--emit",
            "--devcmd-script",
            "C:/definitely/not/here/fake.bat",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(4),
        "expected exit 4, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn pass_through_user_command_exit_code() {
    // `cmd /c exit 17` exits with 17; spawn_command must proxy that
    // exact code to the binary's parent.
    let output = Command::cargo_bin("prepare-devenv")
        .unwrap()
        .args(["--devcmd-script", FAKE_BAT, "--", "cmd", "/c", "exit 17"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(17),
        "expected proxied exit 17, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
