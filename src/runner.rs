//! Output-mode dispatch: the three execution paths that consume an
//! [`EnvDiff`] and a base env to either print eval-able lines, spawn the
//! detected/overridden shell, or spawn the user's command.
//!
//! All three functions are pure with respect to the process's current env
//! variables — they take the pre-snapshot as `base` and overlay the diff —
//! so that orchestration in `main.rs` (Task 9) can drive them without any
//! ambient state.
//!
//! ## Why `env_clear()` on every spawn
//!
//! Both spawn paths build the merged env via [`EnvDiff::merged_env`] (with
//! per-shell `translate_path` applied for `spawn_shell`). That merged map
//! ALREADY contains everything from the pre-snapshot plus the diff overlay.
//! Without `env_clear()`, `Command` defaults to inheriting the parent's
//! env on top of `.envs(&merged)` — which would silently re-introduce
//! the pre-snapshot's casing of any case-variant vars (e.g. parent's
//! `Path` shadowing the merged map's `PATH`), since Windows folds names
//! case-insensitively at process-creation time. Calling `env_clear()`
//! first guarantees the spawned process sees only what's in `merged`.
//!
//! ## `pwsh.exe` → `powershell.exe` fallback
//!
//! [`spawn_shell`] tries the adapter's `executable()` first; if that fails
//! with `ErrorKind::NotFound` AND the adapter is `Pwsh`, it retries with
//! `powershell.exe`. The fallback is logged at `info!` level so a `-v`
//! run surfaces what happened. We deliberately don't fold this into the
//! `Pwsh::executable()` accessor itself: the adapter is a pure shell-syntax
//! adapter and is shared with `--emit` mode, where the fallback is
//! irrelevant.

#![allow(dead_code)] // wired into main.rs by Task 9

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{self, ErrorKind, Write};
use std::process::Command;

use crate::diff::EnvDiff;
use crate::error::{Error, Result};
use crate::shell::{Shell, ShellKind};

/// Write the diff as one shell-formatted line per entry to `out`.
///
/// Iterates the diff (added entries first, sorted by key, then modified
/// entries, sorted by key — see [`EnvDiff::iter`]), runs each value
/// through `shell.translate_path` to handle bash's `PATH` POSIX-conversion
/// (every other adapter's `translate_path` is identity), formats the
/// (name, value) pair via `shell.format_set`, and writes the resulting
/// line followed by `\n` to `out`.
///
/// Any IO error from the writer surfaces as `Error::Io`; any
/// adapter-side rejection (currently only cmd's literal-`"` rejection)
/// surfaces as `Error::EnvParse`.
pub fn emit(diff: &EnvDiff, shell: &dyn Shell, out: &mut dyn Write) -> Result<()> {
    tracing::info!(
        added = diff.added.len(),
        modified = diff.modified.len(),
        "emitting env entries"
    );
    for (name, value) in diff.iter() {
        let translated = shell.translate_path(name, value);
        let line = shell.format_set(name, &translated)?;
        writeln!(out, "{line}")?;
    }
    Ok(())
}

/// Build the merged env for the spawn-shell mode, applying
/// `shell.translate_path` per (name, value) so the spawned shell sees
/// values in its own native form (e.g. POSIX `PATH` for bash).
fn build_shell_env(
    diff: &EnvDiff,
    base: &HashMap<OsString, OsString>,
    shell: &dyn Shell,
) -> HashMap<OsString, OsString> {
    // Start from the case-folded merge so any case-variant base entry
    // (e.g. `Path` from cmd, `PATH` from the diff) is collapsed to the
    // diff's casing. Then overwrite each diff entry with the
    // `translate_path`-rewritten value so bash's `PATH` lands in POSIX
    // form. The first pass already inserted the raw value; the second
    // pass replaces it with the translated one.
    let mut merged = diff.merged_env(base);
    for (name, value) in diff.iter() {
        let translated = shell.translate_path(name, value);
        merged.insert(name.clone(), translated);
    }
    merged
}

/// Spawn the chosen shell with the merged env, inheriting stdio so the
/// user gets an interactive session.
///
/// Returns the shell's exit code on clean termination (or `1` if the
/// platform reports termination by signal with no exit code), or
/// [`Error::Spawn`] when the shell binary cannot be started. For the
/// `Pwsh` adapter, a `NotFound` failure transparently falls back to
/// `powershell.exe`.
pub fn spawn_shell(
    diff: &EnvDiff,
    base: &HashMap<OsString, OsString>,
    shell: &dyn Shell,
) -> Result<i32> {
    let merged = build_shell_env(diff, base, shell);
    let exe = shell.executable();
    tracing::info!(executable = ?exe, "spawning shell");
    tracing::debug!(env_count = merged.len(), "merged env size");

    // env_clear() ensures Command does not stack the parent process's env
    // on top of `merged`. The merged map already contains the pre-snapshot
    // overlaid with the diff, so the spawned shell should see only that.
    match Command::new(exe).env_clear().envs(&merged).status() {
        Ok(status) => Ok(status.code().unwrap_or(1)),
        Err(e) if shell.kind() == ShellKind::Pwsh && e.kind() == ErrorKind::NotFound => {
            tracing::info!("pwsh.exe not found; falling back to powershell.exe");
            let status = Command::new("powershell.exe")
                .env_clear()
                .envs(&merged)
                .status()
                .map_err(|e| Error::Spawn {
                    cmd: "powershell.exe".into(),
                    source: e,
                })?;
            Ok(status.code().unwrap_or(1))
        }
        Err(e) => Err(Error::Spawn {
            cmd: exe.to_string_lossy().into_owned(),
            source: e,
        }),
    }
}

/// Spawn the user-supplied argv with the merged env, inheriting stdio.
///
/// Per the spec, the user's command receives env values in Windows-native
/// form (no per-shell `translate_path`); the merged env is built directly
/// via [`EnvDiff::merged_env`].
///
/// Empty `argv` is rejected with [`Error::Spawn`] wrapping an
/// `InvalidInput` IO error — the caller's mode dispatcher should never
/// route here without at least the command name, but defending against
/// it keeps the failure mode crisp.
pub fn spawn_command(
    diff: &EnvDiff,
    base: &HashMap<OsString, OsString>,
    argv: &[OsString],
) -> Result<i32> {
    if argv.is_empty() {
        return Err(Error::Spawn {
            cmd: "<empty>".into(),
            source: io::Error::new(io::ErrorKind::InvalidInput, "empty argv"),
        });
    }
    tracing::info!(argv = ?argv, "spawning command");

    let merged = diff.merged_env(base);
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]).env_clear().envs(&merged);

    let status = cmd.status().map_err(|e| Error::Spawn {
        cmd: argv[0].to_string_lossy().into_owned(),
        source: e,
    })?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell;
    use std::collections::BTreeMap;

    /// Build an `EnvDiff` with one added entry. Convenience for emit tests.
    fn diff_one_added(name: &str, value: &str) -> EnvDiff {
        let mut d = EnvDiff::default();
        d.added.insert(OsString::from(name), OsString::from(value));
        d
    }

    /// Build an `EnvDiff` with one modified entry. Convenience for emit tests.
    fn diff_one_modified(name: &str, value: &str) -> EnvDiff {
        let mut d = EnvDiff::default();
        d.modified
            .insert(OsString::from(name), OsString::from(value));
        d
    }

    #[test]
    fn emit_for_bash_writes_export_line() {
        let diff = diff_one_added("FOO", "bar");
        let shell = shell::for_kind(ShellKind::Bash);
        let mut buf: Vec<u8> = Vec::new();
        emit(&diff, shell.as_ref(), &mut buf).expect("bash emit should succeed");
        assert_eq!(String::from_utf8(buf).unwrap(), "export FOO='bar'\n");
    }

    #[test]
    fn emit_for_cmd_writes_set_line() {
        let diff = diff_one_added("INCLUDE", r"C:\foo");
        let shell = shell::for_kind(ShellKind::Cmd);
        let mut buf: Vec<u8> = Vec::new();
        emit(&diff, shell.as_ref(), &mut buf).expect("cmd emit should succeed");
        assert_eq!(String::from_utf8(buf).unwrap(), "set \"INCLUDE=C:\\foo\"\n");
    }

    #[test]
    fn emit_for_pwsh_writes_env_assignment() {
        let diff = diff_one_added("FOO", "bar");
        let shell = shell::for_kind(ShellKind::Pwsh);
        let mut buf: Vec<u8> = Vec::new();
        emit(&diff, shell.as_ref(), &mut buf).expect("pwsh emit should succeed");
        assert_eq!(String::from_utf8(buf).unwrap(), "$env:FOO = 'bar'\n");
    }

    #[test]
    fn emit_for_fish_writes_set_gx_line() {
        let diff = diff_one_added("INCLUDE", r"C:\foo");
        let shell = shell::for_kind(ShellKind::Fish);
        let mut buf: Vec<u8> = Vec::new();
        emit(&diff, shell.as_ref(), &mut buf).expect("fish emit should succeed");
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "set -gx INCLUDE 'C:\\\\foo'\n"
        );
    }

    #[test]
    fn emit_for_nu_writes_env_dot_assignment() {
        let diff = diff_one_added("INCLUDE", r"C:\foo");
        let shell = shell::for_kind(ShellKind::Nu);
        let mut buf: Vec<u8> = Vec::new();
        emit(&diff, shell.as_ref(), &mut buf).expect("nu emit should succeed");
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "$env.INCLUDE = 'C:\\foo'\n"
        );
    }

    #[test]
    fn emit_for_bash_translates_path() {
        // The `Bash` adapter overrides `translate_path` to convert
        // Windows-style PATH to POSIX. Verifies that `emit` actually
        // routes values through `translate_path` before formatting.
        let diff = diff_one_modified("PATH", r"C:\foo;D:\bar");
        let bash = shell::for_kind(ShellKind::Bash);
        let mut buf: Vec<u8> = Vec::new();
        emit(&diff, bash.as_ref(), &mut buf).expect("bash emit should succeed");
        let s = String::from_utf8(buf).unwrap();
        assert!(
            s.contains("/c/foo:/d/bar"),
            "expected POSIX-form path in output, got: {s}"
        );
        assert_eq!(s, "export PATH='/c/foo:/d/bar'\n");
    }

    #[test]
    fn emit_writes_added_before_modified_each_sorted() {
        // Spec: emit MUST produce added entries (sorted) before modified
        // entries (sorted). Use a diff with both buckets and verify line
        // order in the rendered output.
        let diff = EnvDiff {
            added: BTreeMap::from([
                (OsString::from("ZULU"), OsString::from("za")),
                (OsString::from("ALPHA"), OsString::from("al")),
            ]),
            modified: BTreeMap::from([
                (OsString::from("YANKEE"), OsString::from("ya")),
                (OsString::from("BRAVO"), OsString::from("br")),
            ]),
        };
        let shell = shell::for_kind(ShellKind::Bash);
        let mut buf: Vec<u8> = Vec::new();
        emit(&diff, shell.as_ref(), &mut buf).expect("emit should succeed");
        let out = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(
            lines,
            vec![
                "export ALPHA='al'",
                "export ZULU='za'",
                "export BRAVO='br'",
                "export YANKEE='ya'",
            ],
            "expected added (ALPHA, ZULU) before modified (BRAVO, YANKEE)"
        );
    }

    #[test]
    fn emit_propagates_format_set_error() {
        // cmd's `format_set` rejects values containing a literal `"`.
        // `emit` must surface that as `Error::EnvParse`, not eat it.
        let diff = diff_one_added("X", r#"a"b"#);
        let shell = shell::for_kind(ShellKind::Cmd);
        let mut buf: Vec<u8> = Vec::new();
        let err = emit(&diff, shell.as_ref(), &mut buf).expect_err("cmd should reject `\"`");
        assert!(matches!(err, Error::EnvParse(_)), "got {err:?}");
    }

    #[test]
    fn emit_empty_diff_writes_nothing() {
        // Sanity: an empty diff produces an empty stream — no banners,
        // no leading newline, just zero bytes.
        let diff = EnvDiff::default();
        let shell = shell::for_kind(ShellKind::Bash);
        let mut buf: Vec<u8> = Vec::new();
        emit(&diff, shell.as_ref(), &mut buf).expect("empty emit should succeed");
        assert!(buf.is_empty(), "expected empty output, got {buf:?}");
    }

    #[test]
    fn build_shell_env_overlays_diff_on_base() {
        // The merge logic that backs `spawn_shell`. With a non-bash
        // adapter (translate_path is identity) the result must equal
        // `merged_env`'s overlay.
        let mut base: HashMap<OsString, OsString> = HashMap::new();
        base.insert(OsString::from("USER"), OsString::from("alice"));
        base.insert(OsString::from("PATH"), OsString::from("A;B"));

        let mut diff = EnvDiff::default();
        diff.modified
            .insert(OsString::from("PATH"), OsString::from("X;A;B"));
        diff.added
            .insert(OsString::from("INCLUDE"), OsString::from("/foo"));

        let shell = shell::for_kind(ShellKind::Cmd);
        let merged = build_shell_env(&diff, &base, shell.as_ref());

        assert_eq!(
            merged.get(&OsString::from("USER")),
            Some(&OsString::from("alice")),
            "unchanged base entry should survive"
        );
        assert_eq!(
            merged.get(&OsString::from("PATH")),
            Some(&OsString::from("X;A;B")),
            "modified entry should overwrite base"
        );
        assert_eq!(
            merged.get(&OsString::from("INCLUDE")),
            Some(&OsString::from("/foo")),
            "added entry should be present"
        );
    }

    #[test]
    fn build_shell_env_applies_bash_translate_path() {
        // For the bash adapter, `build_shell_env` must rewrite the PATH
        // value in the merged map to POSIX form. Other vars (INCLUDE,
        // base USER) must keep their Windows-native form.
        let mut base: HashMap<OsString, OsString> = HashMap::new();
        base.insert(OsString::from("USER"), OsString::from("alice"));

        let mut diff = EnvDiff::default();
        diff.modified
            .insert(OsString::from("PATH"), OsString::from(r"C:\foo;D:\bar"));
        diff.added
            .insert(OsString::from("INCLUDE"), OsString::from(r"C:\inc"));

        let shell = shell::for_kind(ShellKind::Bash);
        let merged = build_shell_env(&diff, &base, shell.as_ref());

        assert_eq!(
            merged.get(&OsString::from("PATH")),
            Some(&OsString::from("/c/foo:/d/bar")),
            "PATH should be POSIX-form for bash"
        );
        assert_eq!(
            merged.get(&OsString::from("INCLUDE")),
            Some(&OsString::from(r"C:\inc")),
            "INCLUDE should keep Windows form (translate_path is a no-op)"
        );
        assert_eq!(
            merged.get(&OsString::from("USER")),
            Some(&OsString::from("alice")),
            "base USER should survive untouched"
        );
    }

    // `spawn_shell` is intentionally not unit-tested here. Spawning
    // `cmd.exe` (or any other supported shell binary) without arguments
    // would block waiting for input via inherited stdio, and CI runners
    // have no interactive console. End-to-end coverage lives in Task 10
    // under `tests/cli_*.rs` using `assert_cmd` and the `test_hooks`
    // feature, which can spin the spawned shell up against a recorded
    // fake bat. The merge logic — which is the load-bearing part — is
    // exercised above via `build_shell_env_*` tests.

    #[test]
    fn spawn_command_runs_argv_and_returns_exit_code() {
        // `cmd /c exit 17` is the canonical Windows way to produce a
        // specific exit code without touching stdin/stdout.
        let argv = vec![
            OsString::from("cmd.exe"),
            OsString::from("/c"),
            OsString::from("exit 17"),
        ];
        let diff = EnvDiff::default();
        let base: HashMap<OsString, OsString> = HashMap::new();
        let code = spawn_command(&diff, &base, &argv).expect("cmd /c exit 17 should run");
        assert_eq!(code, 17, "expected proxied exit code 17, got {code}");
    }

    #[test]
    fn spawn_command_rejects_empty_argv() {
        let diff = EnvDiff::default();
        let base: HashMap<OsString, OsString> = HashMap::new();
        let err = spawn_command(&diff, &base, &[]).expect_err("empty argv should be rejected");
        match err {
            Error::Spawn { cmd, source } => {
                assert_eq!(cmd, "<empty>");
                assert_eq!(source.kind(), io::ErrorKind::InvalidInput);
            }
            other => panic!("expected Error::Spawn, got {other:?}"),
        }
    }

    #[test]
    fn spawn_command_surfaces_spawn_failure_as_error() {
        // A nonexistent command must surface as `Error::Spawn` carrying
        // the command name and an `io::Error::NotFound` source.
        let argv = vec![OsString::from(
            "definitely-not-a-real-command-xyz-1234567890.exe",
        )];
        let diff = EnvDiff::default();
        let base: HashMap<OsString, OsString> = HashMap::new();
        let err = spawn_command(&diff, &base, &argv)
            .expect_err("spawning a nonexistent binary should fail");
        match err {
            Error::Spawn { cmd, source } => {
                assert!(
                    cmd.contains("definitely-not-a-real-command"),
                    "unexpected cmd in error: {cmd}"
                );
                assert_eq!(source.kind(), io::ErrorKind::NotFound);
            }
            other => panic!("expected Error::Spawn, got {other:?}"),
        }
    }
}
