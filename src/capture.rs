//! Capture the environment after `VsDevCmd.bat` has mutated it.
//!
//! The pipeline is:
//!
//! 1. [`snapshot_pre_env`] records the parent process's env vars before the
//!    bat is invoked.
//! 2. [`build_capture_command`] constructs the `cmd.exe /U /D /Q /C "..."`
//!    invocation that will run the bat and dump `set` output. `/U` forces the
//!    inner `cmd.exe` to emit its stdout as UTF-16LE so non-ASCII values
//!    (paths under installs, localized strings, etc.) round-trip cleanly.
//! 3. [`fresh_marker`] returns a per-invocation sentinel that separates any
//!    chatter `VsDevCmd.bat` might print on stdout from the post-run `set`
//!    output we actually want to parse.
//! 4. [`capture_with`] consumes the inner `cmd.exe`'s stdout, strips the
//!    optional UTF-16LE BOM, decodes to a `String`, normalizes line endings,
//!    finds the marker line, and parses the lines after it as `KEY=VALUE`
//!    splitting on the first `=` only.
//! 5. [`capture`] wires the pieces together, including propagating
//!    `VsDevCmd.bat`'s exit code as [`Error::VsDevCmdFailed`] when the
//!    `&&`-chained `set` is never reached.
//!
//! ## Return type choice
//!
//! [`capture`] returns `HashMap<OsString, OsString>` directly rather than a
//! `CapturedEnv` newtype. The plan called for a wrapper type but the only
//! consumer in this crate is `diff::diff`, which already takes
//! `&HashMap<OsString, OsString>` for both pre and post snapshots. A wrapper
//! would force callers to either expose the inner map or duplicate the API
//! surface — neither is worth the additional indirection at v1.
//!
//! ## Windows env-var case sensitivity
//!
//! Windows looks up env vars case-insensitively but stores them with
//! whatever casing the writer used. `HashMap<OsString, OsString>` preserves
//! the original casing of every key — case-insensitive *lookup* is the
//! responsibility of consumers (`diff::diff` does it; see Task 6). This module
//! never lower-cases keys.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Path;
use std::process::Command;

use crate::error::{Error, Result};

/// Snapshot the parent process's environment as an `OsString → OsString` map.
///
/// Returns `std::env::vars_os().collect()`. Keys preserve the original casing
/// the parent writer used. Lookup on Windows is conventionally
/// case-insensitive — the diff module (Task 6) handles that comparison;
/// the snapshot itself is just a faithful dump.
pub fn snapshot_pre_env() -> HashMap<OsString, OsString> {
    std::env::vars_os().collect()
}

/// Generate a fresh per-invocation marker like
/// `__PREPARE_DEVENV_ENV_BEGIN_<16 hex>__`.
///
/// The marker only needs to be unguessable enough to avoid colliding with
/// any chatter `VsDevCmd.bat` writes to stdout — this is not a security
/// boundary, just a parser sentinel. We mix `SystemTime` nanos with the
/// process id and a process-local atomic counter through a Knuth
/// multiplicative constant; this gives 16 hex chars that change every call
/// without pulling in `rand` or `getrandom`. The counter guarantees that
/// two calls within the same `SystemTime` tick still produce different
/// output, so the uniqueness test cannot flake on slow runners with
/// coarse-grained clocks.
fn fresh_marker() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = u64::from(std::process::id());
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let raw = nanos
        .wrapping_mul(0x9E37_79B1_7F4A_7C15)
        .wrapping_add(pid)
        .wrapping_add(counter);
    format!("__PREPARE_DEVENV_ENV_BEGIN_{raw:016x}__")
}

/// Build the `cmd.exe` invocation that runs `VsDevCmd.bat` and dumps env.
///
/// Constructs argv as separate args (NOT a single joined string) so Windows'
/// argv-to-CommandLine round-trip leaves the inner script string intact.
/// Specifically:
///
/// - `cmd.exe /U /D /Q /C "<inner>"`
///   - `/U` — force UTF-16LE on stdout/stderr.
///   - `/D` — skip `AutoRun` registry hooks (`HKCU\Software\Microsoft\Command Processor\AutoRun`)
///     so user customizations don't perturb the captured env.
///   - `/Q` — turn off command echo.
///   - `/C` — run the following string then exit.
/// - inner = `call "<bat>" <devcmd-args> 1>&2 && echo <MARKER> && set`
///   - `1>&2` redirects `VsDevCmd.bat`'s own chatter to the inner cmd's
///     stderr, so the only thing on stdout is the marker line plus `set`
///     output. Verbose-mode (`-v`) callers can still see the original chatter
///     by capturing stderr.
///   - `&&` short-circuits: if the bat fails, `set` is never reached and the
///     inner `cmd.exe` returns the bat's exit code.
///
/// Assumes `bat` does not contain a literal `"` and does not end in `\\`
/// (both are extremely unusual for a `VsDevCmd.bat` path resolved by
/// vswhere). The cmd.exe quoting algorithm treats `\\"` as an escaped
/// quote, so a trailing-backslash path before the closing `"` would break
/// parsing.
fn build_capture_command(bat: &Path, devcmd_args: Option<&str>, marker: &str) -> Command {
    use std::os::windows::process::CommandExt;

    let inner = format!(
        "call \"{}\" {} 1>&2 && echo {} && set",
        bat.display(),
        devcmd_args.unwrap_or(""),
        marker
    );
    let mut cmd = Command::new("cmd.exe");
    // Use raw_arg for the inner script: Rust's default arg-escaping wraps
    // values containing spaces in `"..."` and rewrites embedded `"` as `\"`,
    // but cmd.exe does NOT understand `\"` — it would parse the resulting
    // command line as a broken path. raw_arg passes the string through to
    // CreateProcess verbatim, letting cmd's own `/C "..."` parser see the
    // intended quoting. The `/U /D /Q /C` flags themselves contain neither
    // spaces nor quotes, so we keep `arg()` for them.
    cmd.arg("/U")
        .arg("/D")
        .arg("/Q")
        .arg("/C")
        .raw_arg(format!("\"{inner}\""));
    cmd
}

/// Decode UTF-16LE bytes (with optional leading BOM) into a `String` and
/// normalize CRLF line endings to LF.
///
/// `cmd /U` emits UTF-16LE bytes with `\r\n` line endings. We strip the
/// BOM if present (Windows is inconsistent about whether `cmd /U` writes one),
/// then chunk the remaining bytes into `u16` units in little-endian order
/// and run them through `String::from_utf16_lossy` so any malformed surrogate
/// pair is replaced rather than failing the parse. Finally we replace
/// `\r\n` with `\n` so downstream `.lines()` iteration is uniform.
///
/// Assumption: callers pass `cmd /U` output, which always begins with the
/// UTF-16LE BOM (`0xFF 0xFE`). We treat a leading `0xFF 0xFE` as a BOM
/// unconditionally; a BOM-less stream whose first code unit *happens* to
/// be U+FEFF would be mis-stripped, but that cannot occur with `cmd /U`
/// output.
fn decode_utf16le(bytes: &[u8]) -> String {
    let body = if bytes.starts_with(&[0xFF, 0xFE]) {
        &bytes[2..]
    } else {
        bytes
    };
    let units: Vec<u16> = body
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let decoded = String::from_utf16_lossy(&units);
    decoded.replace("\r\n", "\n")
}

/// Parse a UTF-16LE byte stream produced by the inner `cmd.exe` into an env map.
///
/// This is the test seam: production code calls [`capture`] which spawns
/// `cmd.exe`, but tests can drive this function directly with committed
/// fixture bytes (no live VS install needed).
///
/// The reader is read to end (we expect a few kilobytes at most), decoded
/// via [`decode_utf16le`], then split by line. We scan for the marker as an
/// exact match (NOT `contains`) so a value that happens to embed the marker
/// substring can't fool the parser. After the marker, blank lines are
/// skipped, lines without `=` are skipped silently (`set` shouldn't emit any
/// but we stay lenient), and `KEY=VALUE` lines are split on the first `=`
/// only — values may legitimately contain further `=` characters
/// (e.g. encoded query strings cached in some tools' env vars).
pub fn capture_with(mut reader: impl Read, marker: &str) -> Result<HashMap<OsString, OsString>> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    let text = decode_utf16le(&bytes);

    let mut lines = text.lines();
    let mut found = false;
    for line in lines.by_ref() {
        // cmd.exe's `echo X && set` echoes `X ` with a trailing space (the
        // space between `echo X` and `&&` is part of echo's argument), so
        // the marker line we wrote with `echo {marker} && set` arrives as
        // `{marker} `. Trim trailing whitespace before the equality check
        // so that quirk doesn't make us miss our own sentinel.
        if line.trim_end() == marker {
            found = true;
            break;
        }
    }
    if !found {
        return Err(Error::EnvParse(
            "marker not found; re-run with -v to see cmd/VsDevCmd.bat output",
        ));
    }

    let mut map = HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        map.insert(OsString::from(key), OsString::from(value));
    }
    Ok(map)
}

/// Run `VsDevCmd.bat` under a captured `cmd.exe /U` and return the post-run env.
///
/// Generates a fresh marker, builds the command via [`build_capture_command`],
/// spawns it with `.output()`, and propagates the inner cmd's exit code: a
/// non-zero status means `VsDevCmd.bat` failed (the `&&` short-circuited and
/// `cmd.exe` exited with the bat's code), surfaced as
/// [`Error::VsDevCmdFailed`]. On a zero exit, the captured stdout is fed to
/// [`capture_with`] for parsing.
///
/// Returns `HashMap<OsString, OsString>` directly — see the module-level note
/// about why we don't wrap this in a `CapturedEnv` newtype.
///
/// # `forward_stderr_on_success`
///
/// Per spec `cli-surface-and-diagnostics / Surface VsDevCmd.bat's stdout only
/// at -v or higher` and `devenv-environment-capture / VsDevCmd.bat exits
/// non-zero`, the inner cmd's stderr (which is where the `1>&2` redirect
/// dumps `VsDevCmd.bat`'s own chatter) must be surfaced to the user's stderr
/// at `-v` or higher and unconditionally on failure (it is the only diagnostic
/// the user has when the bat refuses to run).
///
/// - On **failure** (non-zero exit), the captured `output.stderr` is ALWAYS
///   written to `std::io::stderr()` before returning [`Error::VsDevCmdFailed`].
/// - On **success**, the captured stderr is written to `std::io::stderr()`
///   only when `forward_stderr_on_success == true` (i.e., when the caller was
///   invoked with `-v` or higher).
///
/// Side-effect note: this function writes directly to `std::io::stderr()` so
/// no writer plumbing has to thread through the call site. The stream is
/// always shared with `tracing-subscriber`, which is correct: `-v` users
/// asked for noise.
pub fn capture(
    bat: &Path,
    devcmd_args: Option<&str>,
    forward_stderr_on_success: bool,
) -> Result<HashMap<OsString, OsString>> {
    let marker = fresh_marker();
    let mut cmd = build_capture_command(bat, devcmd_args, &marker);
    // Trace the constructed cmd /U argv and marker token under `-v`
    // (info!) per spec `cli-surface-and-diagnostics / -v should include
    // the constructed cmd /U line, the marker token`.
    let argv: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    tracing::info!(?argv, marker = %marker, "running cmd /U capture");
    let output = cmd.output().map_err(|source| Error::Spawn {
        cmd: "cmd.exe".to_string(),
        source,
    })?;
    // output.stderr holds VsDevCmd.bat's own chatter (via the inner `1>&2`).
    if !output.status.success() {
        // Failure: always forward — it's the only diagnostic the user has.
        let _ = std::io::stderr().write_all(&output.stderr);
        return Err(Error::VsDevCmdFailed(output.status.code().unwrap_or(-1)));
    }
    if forward_stderr_on_success {
        let _ = std::io::stderr().write_all(&output.stderr);
    }
    capture_with(output.stdout.as_slice(), &marker)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_MARKER: &str = "__PREPARE_DEVENV_ENV_BEGIN_TEST__";

    // Fixture bytes are generated by `examples/gen_capture_fixtures.rs` and
    // committed under `tests/fixtures/capture/`. We `include_bytes!` them so
    // the tests don't need to spawn the example or touch the filesystem.
    const MINIMAL_WITH_BOM: &[u8] =
        include_bytes!("../tests/fixtures/capture/minimal_with_bom.bin");
    const MINIMAL_NO_BOM: &[u8] = include_bytes!("../tests/fixtures/capture/minimal_no_bom.bin");
    const VALUE_WITH_EQUALS: &[u8] =
        include_bytes!("../tests/fixtures/capture/value_with_equals.bin");
    const MULTIBYTE_NON_ASCII: &[u8] =
        include_bytes!("../tests/fixtures/capture/multibyte_non_ascii.bin");
    const MISSING_MARKER: &[u8] = include_bytes!("../tests/fixtures/capture/missing_marker.bin");

    #[test]
    fn well_formed_capture_output() {
        let env = capture_with(MINIMAL_WITH_BOM, TEST_MARKER).expect("parse OK");
        assert_eq!(
            env.get(&OsString::from("FOO")),
            Some(&OsString::from("bar"))
        );
        assert_eq!(
            env.get(&OsString::from("BAZ")),
            Some(&OsString::from("qux"))
        );
    }

    #[test]
    fn value_contains_equals() {
        // "MYVAR=a=b=c" must parse as MYVAR -> "a=b=c", not MYVAR -> "a"
        // (split_once on first `=`). This is the spec scenario "value contains
        // '=' characters".
        let env = capture_with(VALUE_WITH_EQUALS, TEST_MARKER).expect("parse OK");
        assert_eq!(
            env.get(&OsString::from("MYVAR")),
            Some(&OsString::from("a=b=c"))
        );
    }

    #[test]
    fn marker_not_found() {
        let err = capture_with(MISSING_MARKER, TEST_MARKER).expect_err("should fail");
        match err {
            Error::EnvParse(msg) => {
                assert!(
                    msg.contains("marker not found"),
                    "unexpected EnvParse message: {msg}"
                );
            }
            other => panic!("expected EnvParse, got {other:?}"),
        }
    }

    #[test]
    fn bom_and_crlf_handling() {
        // minimal_with_bom: BOM + CRLF — must be parsed cleanly.
        let env = capture_with(MINIMAL_WITH_BOM, TEST_MARKER).expect("BOM+CRLF parse OK");
        assert_eq!(
            env.get(&OsString::from("FOO")),
            Some(&OsString::from("bar"))
        );
        assert_eq!(
            env.get(&OsString::from("BAZ")),
            Some(&OsString::from("qux"))
        );

        // minimal_no_bom: no BOM, LF-only — must also parse cleanly.
        let env = capture_with(MINIMAL_NO_BOM, TEST_MARKER).expect("no-BOM/LF parse OK");
        assert_eq!(
            env.get(&OsString::from("FOO")),
            Some(&OsString::from("bar"))
        );
    }

    #[test]
    fn multibyte_non_ascii() {
        let env = capture_with(MULTIBYTE_NON_ASCII, TEST_MARKER).expect("parse OK");
        assert_eq!(
            env.get(&OsString::from("WELCOME")),
            Some(&OsString::from("こんにちは"))
        );
        // PATH value with backslashes survives unchanged (the fixture writes
        // a single backslash; the Rust string literal escapes are doubled).
        assert_eq!(
            env.get(&OsString::from("PATH")),
            Some(&OsString::from(r"C:\foo"))
        );
    }

    #[test]
    fn build_capture_command_argv_shape() {
        let bat = Path::new(r"C:\bat.bat");
        let cmd = build_capture_command(bat, None, "MARK");
        let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();
        assert_eq!(args.len(), 5, "expected exactly 5 args, got {args:?}");
        assert_eq!(args[0], std::ffi::OsStr::new("/U"));
        assert_eq!(args[1], std::ffi::OsStr::new("/D"));
        assert_eq!(args[2], std::ffi::OsStr::new("/Q"));
        assert_eq!(args[3], std::ffi::OsStr::new("/C"));
        // The 5th arg is the inner script wrapped in literal quotes — we
        // use `raw_arg` so cmd.exe's `/C "..."` parser sees the embedded
        // quotes around the bat path verbatim. `1>&2` keeps VsDevCmd.bat's
        // own chatter off our stdout, the marker is emitted on its own line,
        // and `set` follows. devcmd_args=None expands to an empty string.
        let expected_script = r#""call "C:\bat.bat"  1>&2 && echo MARK && set""#;
        assert_eq!(args[4], std::ffi::OsStr::new(expected_script));
    }

    #[test]
    fn build_capture_command_with_devcmd_args() {
        // Spec scenario: --devcmd-args "-arch=x64" should land verbatim
        // between the bat path and the `1>&2` redirect.
        let bat = Path::new(r"C:\bat.bat");
        let cmd = build_capture_command(bat, Some("-arch=x64"), "MARK");
        let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();
        let expected_script = r#""call "C:\bat.bat" -arch=x64 1>&2 && echo MARK && set""#;
        assert_eq!(args[4], std::ffi::OsStr::new(expected_script));
    }

    #[test]
    fn fresh_marker_is_unique() {
        // SystemTime::now()-based; consecutive calls land in different nanos
        // in practice. If this becomes flaky on a clock with poor resolution
        // we can sleep 1ns between calls — for now keep it simple.
        let a = fresh_marker();
        let b = fresh_marker();
        assert_ne!(a, b, "consecutive markers collided: {a} vs {b}");
        assert!(a.starts_with("__PREPARE_DEVENV_ENV_BEGIN_"));
        assert!(a.ends_with("__"));
    }

    #[test]
    fn capture_with_handles_marker_followed_by_trailing_whitespace() {
        use std::io::Cursor;

        // Build UTF-16LE bytes for: BOM + "MARKER \r\nFOO=bar\r\n"
        // (note the trailing space after MARKER, simulating cmd's `echo X && set` output).
        let marker = "__TEST_MARKER__";
        let mut s = String::new();
        s.push_str(marker);
        s.push(' '); // trailing space — cmd's echo behavior
        s.push_str("\r\n");
        s.push_str("FOO=bar\r\n");

        let mut bytes = vec![0xFF, 0xFE]; // UTF-16LE BOM
        for unit in s.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }

        let env = capture_with(Cursor::new(&bytes), marker).unwrap();
        assert_eq!(
            env.get(std::ffi::OsStr::new("FOO")),
            Some(&std::ffi::OsString::from("bar"))
        );
    }

    #[test]
    fn snapshot_pre_env_includes_path() {
        // Every Windows process has a PATH variable. Lookup is
        // case-insensitive on Windows but our snapshot preserves the writer's
        // casing, so we iterate and match case-insensitively.
        let env = snapshot_pre_env();
        let has_path = env
            .keys()
            .any(|k| k.to_string_lossy().eq_ignore_ascii_case("PATH"));
        assert!(has_path, "snapshot missing PATH key");
    }
}
