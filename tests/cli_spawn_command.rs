//! End-to-end test for the spawn-command (`-- COMMAND`) mode.
//!
//! Runs the binary with `--devcmd-script <fake.bat> -- cmd /c set` and
//! checks that the diffed env vars produced by the fake bat are visible
//! in the spawned `cmd /c set` output. This proves the orchestration
//! layer correctly threads the merged env into the user-supplied
//! command.

#![cfg(feature = "test_hooks")]

use assert_cmd::Command;

#[test]
fn spawn_command_inherits_diffed_env() {
    let output = Command::cargo_bin("prepare-devenv")
        .unwrap()
        .args([
            "--devcmd-script",
            "tests/fixtures/devcmd-script/fake.bat",
            "--",
            "cmd",
            "/c",
            "set",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "binary exited with {:?}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("FAKE_INCLUDE=C:\\fake\\include"),
        "stdout missing FAKE_INCLUDE: {stdout}"
    );
    assert!(
        stdout.contains("FAKE_LIB=C:\\fake\\lib"),
        "stdout missing FAKE_LIB: {stdout}"
    );
    assert!(
        stdout.contains("FAKE_VCINSTALLDIR=C:\\fake\\vc"),
        "stdout missing FAKE_VCINSTALLDIR: {stdout}"
    );
}
