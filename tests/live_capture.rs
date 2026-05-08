//! Live integration test: real `VsDevCmd.bat` capture round-trip.
//!
//! Gated behind the `live` Cargo feature (default off). Requires a real
//! Visual Studio install (so `vswhere.exe` resolves to a valid
//! `VsDevCmd.bat`).
//!
//! Asserts that the documented MSVC environment variables survive the
//! discover -> capture -> diff -> emit round-trip. We compare against
//! `--shell cmd` output (the format is `set "NAME=VALUE"`) so a simple
//! substring check on `NAME=` is sufficient.

#![cfg(feature = "live")]

use assert_cmd::Command;

#[test]
fn capture_emit_includes_msvc_vars() {
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

    let stdout = String::from_utf8_lossy(&output.stdout);
    let upper = stdout.to_uppercase();
    // VsDevCmd always sets these — verify they appear in the emitted diff.
    for var in ["INCLUDE", "LIB", "VCINSTALLDIR"] {
        assert!(
            upper.contains(&format!("{var}=")),
            "missing `{var}=` in emit output:\n{stdout}"
        );
    }
}
