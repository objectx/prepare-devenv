//! Bash (`bash.exe`) shell adapter.
//!
//! Two non-trivial bits live here:
//!
//! 1. **Single-quote escaping in [`format_set`](Bash::format_set).** Bash's
//!    single-quoted string has no in-string escape, so to embed a literal `'`
//!    we close-quote, emit `\'`, then re-open. The canonical form is `'\''`.
//! 2. **`PATH` translation in [`translate_path`](Bash::translate_path).** A
//!    Windows-shaped `PATH` (`C:\foo;D:\bar baz`) is unusable to bash — the
//!    `;` is a statement separator and the drive letters are nonsensical to
//!    POSIX tooling. We split on `;`, rewrite each `[A-Za-z]:[\\/]…` segment
//!    to `/<lower-drive>/<rest>` (with `\` flipped to `/`), preserve UNC
//!    paths and non-drive segments verbatim, and join with `:`.

use std::ffi::{OsStr, OsString};

use super::{Shell, ShellKind};
use crate::error::Result;

/// Bash adapter. Zero-sized.
pub struct Bash;

impl Shell for Bash {
    fn kind(&self) -> ShellKind {
        ShellKind::Bash
    }

    fn executable(&self) -> &OsStr {
        OsStr::new("bash.exe")
    }

    fn format_set(&self, name: &OsStr, value: &OsStr) -> Result<String> {
        let n = name.to_string_lossy();
        let v = value.to_string_lossy();
        // Canonical bash single-quote escape: close, escape, reopen.
        let escaped = v.replace('\'', r"'\''");
        Ok(format!("export {n}='{escaped}'"))
    }

    fn translate_path(&self, name: &OsStr, value: &OsStr) -> OsString {
        // OsStr has no eq_ignore_ascii_case, so route via the lossy String.
        if name.to_string_lossy().eq_ignore_ascii_case("PATH") {
            translate_path_value(value)
        } else {
            value.to_owned()
        }
    }
}

/// Convert a Windows-shaped `PATH` value to POSIX form.
///
/// Splits on `;`, runs each segment through [`translate_segment`], then joins
/// with `:`. An empty input yields an empty output (single empty segment, no
/// separators).
fn translate_path_value(value: &OsStr) -> OsString {
    let s = value.to_string_lossy();
    let segments: Vec<String> = s.split(';').map(translate_segment).collect();
    OsString::from(segments.join(":"))
}

/// Rewrite one `PATH` segment from Windows to POSIX form.
///
/// Drive-letter rules:
/// - `<L>:<sep>...`  (sep is `\` or `/`) → `/<lower-l>/<rest with \\ → />`
/// - bare `<L>:`                          → `/<lower-l>/`
/// - everything else                      → returned verbatim (UNC paths,
///   relative paths, and whatever the upstream env block contained)
fn translate_segment(seg: &str) -> String {
    let bytes = seg.as_bytes();

    // <L>:<\\|/>...
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        let drive = (bytes[0] as char).to_ascii_lowercase();
        let rest: String = seg[3..]
            .chars()
            .map(|c| if c == '\\' { '/' } else { c })
            .collect();
        return format!("/{drive}/{rest}");
    }

    // Bare `<L>:` — treat as `<L>:\` → `/<l>/`.
    if bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let drive = (bytes[0] as char).to_ascii_lowercase();
        return format!("/{drive}/");
    }

    seg.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_set_simple() {
        let out = Bash
            .format_set(OsStr::new("X"), OsStr::new("value"))
            .expect("simple value should format");
        assert_eq!(out, "export X='value'");
    }

    #[test]
    fn format_set_escapes_single_quote() {
        let out = Bash
            .format_set(OsStr::new("X"), OsStr::new("a'b"))
            .expect("single quote should escape, not error");
        assert_eq!(out, r"export X='a'\''b'");
    }

    #[test]
    fn format_set_passes_path_chars_through() {
        // Backslash, semicolon, `$` are literal inside bash single-quotes.
        let out = Bash
            .format_set(OsStr::new("INCLUDE"), OsStr::new(r"C:\foo;C:\bar"))
            .expect("path chars should pass through");
        assert_eq!(out, r"export INCLUDE='C:\foo;C:\bar'");
    }

    #[test]
    fn kind_and_executable() {
        assert_eq!(Bash.kind(), ShellKind::Bash);
        assert_eq!(Bash.executable(), OsStr::new("bash.exe"));
    }

    #[test]
    fn translate_path_mixed_slashes() {
        let out = Bash.translate_path(OsStr::new("PATH"), OsStr::new(r"C:\foo;D:/bar baz;E:\"));
        assert_eq!(out, OsStr::new("/c/foo:/d/bar baz:/e/"));
    }

    #[test]
    fn translate_path_drive_letter_casing_normalized() {
        // `c:\X` and `C:\X` must both yield `/c/X`.
        let lower = Bash.translate_path(OsStr::new("PATH"), OsStr::new(r"c:\X"));
        let upper = Bash.translate_path(OsStr::new("PATH"), OsStr::new(r"C:\X"));
        assert_eq!(lower, OsStr::new("/c/X"));
        assert_eq!(upper, OsStr::new("/c/X"));
    }

    #[test]
    fn translate_path_empty() {
        let out = Bash.translate_path(OsStr::new("PATH"), OsStr::new(""));
        assert_eq!(out, OsStr::new(""));
    }

    #[test]
    fn translate_path_non_path_var_passthrough() {
        let out = Bash.translate_path(OsStr::new("INCLUDE"), OsStr::new(r"C:\foo;C:\bar"));
        assert_eq!(out, OsStr::new(r"C:\foo;C:\bar"));
    }

    #[test]
    fn translate_path_unc_passes_through() {
        // UNC `\\server\share\bin` survives verbatim, joined with `:`.
        let out = Bash.translate_path(
            OsStr::new("PATH"),
            OsStr::new(r"\\server\share\bin;C:\tools"),
        );
        assert_eq!(out, OsStr::new(r"\\server\share\bin:/c/tools"));
    }

    #[test]
    fn translate_path_case_insensitive_var_name() {
        // The spec says PATH match is case-insensitive — `Path`, `path`, and
        // `PATH` must all trigger the rewrite.
        for name in ["PATH", "path", "Path", "pAtH"] {
            let out = Bash.translate_path(OsStr::new(name), OsStr::new(r"C:\foo"));
            assert_eq!(out, OsStr::new("/c/foo"), "var name {name}");
        }
    }

    #[test]
    fn translate_path_relative_segment_passes_through() {
        let out = Bash.translate_path(OsStr::new("PATH"), OsStr::new(r"foo\bar;C:\baz"));
        assert_eq!(out, OsStr::new(r"foo\bar:/c/baz"));
    }

    #[test]
    fn translate_path_empty_segment_passes_through() {
        // Trailing `;` produces an empty segment; that becomes an empty piece
        // joined with `:`. Verifies we don't smear segments together.
        let out = Bash.translate_path(OsStr::new("PATH"), OsStr::new(r"C:\foo;;C:\bar"));
        assert_eq!(out, OsStr::new("/c/foo::/c/bar"));
    }
}
