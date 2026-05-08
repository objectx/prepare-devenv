//! Fish (`fish.exe`) shell adapter.
//!
//! Inside fish single-quoted strings only two escapes are recognised: `\\` →
//! `\` and `\'` → `'`. Other backslashes are taken literally — `'C:\foo'`
//! parses to the six-byte string `C:\foo` with no further interpretation.
//!
//! However, a value ending in `\` (such as every VS `*INSTALLDIR` env var,
//! e.g. `C:\Program Files\…\Community\`) would parse as `…\'` — an escaped
//! quote, leaving the string unterminated. To handle this safely we escape
//! backslash and single quote both via prefix-backslash: `\` → `\\` and
//! `'` → `\'`. Backslash MUST be replaced first so that the backslashes we
//! add for `'` are not themselves doubled.
//!
//! See <https://fishshell.com/docs/current/language.html#quotes>.

use std::ffi::OsStr;

use super::{Shell, ShellKind};
use crate::error::Result;

/// Fish adapter. Zero-sized.
pub struct Fish;

impl Shell for Fish {
    fn kind(&self) -> ShellKind {
        ShellKind::Fish
    }

    fn executable(&self) -> &OsStr {
        OsStr::new("fish.exe")
    }

    /// Emit `set -gx NAME 'value'` with backslash and single-quote escaped.
    ///
    /// Escape order is critical: `\` MUST be replaced first (`\` → `\\`) so
    /// that the backslashes added for `'` (`'` → `\'`) are not re-escaped.
    /// This mirrors fish's two recognised in-string escapes and keeps values
    /// with trailing backslashes (VS `*INSTALLDIR`-shape paths) well-formed.
    fn format_set(&self, name: &OsStr, value: &OsStr) -> Result<String> {
        let n = name.to_string_lossy();
        let v = value.to_string_lossy();
        let escaped = v.replace('\\', r"\\").replace('\'', r"\'");
        Ok(format!("set -gx {n} '{escaped}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_set_simple() {
        // Per spec scenario: `INCLUDE` / `C:\foo` — backslash is doubled so
        // the value round-trips through fish's `\\` escape.
        let out = Fish
            .format_set(OsStr::new("INCLUDE"), OsStr::new(r"C:\foo"))
            .expect("simple value should format");
        assert_eq!(out, r"set -gx INCLUDE 'C:\\foo'");
    }

    #[test]
    fn format_set_simple_value() {
        let out = Fish
            .format_set(OsStr::new("X"), OsStr::new("value"))
            .expect("plain value should format");
        assert_eq!(out, "set -gx X 'value'");
    }

    #[test]
    fn format_set_escapes_single_quote() {
        // `a'b` → `a\'b` once the surrounding quotes are added.
        let out = Fish
            .format_set(OsStr::new("X"), OsStr::new("a'b"))
            .expect("single quote should escape, not error");
        assert_eq!(out, r"set -gx X 'a\'b'");
    }

    #[test]
    fn format_set_passes_path_chars_through() {
        // Semicolon and `$` pass through literally inside fish single-quotes;
        // backslashes are doubled so fish's `\\` escape resolves them back.
        let out = Fish
            .format_set(OsStr::new("PATH"), OsStr::new(r"C:\foo;C:\bar"))
            .expect("path chars should pass through");
        assert_eq!(out, r"set -gx PATH 'C:\\foo;C:\\bar'");
    }

    #[test]
    fn format_set_trailing_backslash() {
        // VS `*INSTALLDIR` env vars all end in `\`; without escaping `\` the
        // resulting `'…\'` would be parsed as an unterminated string with an
        // escaped quote. Doubling the backslash keeps the value well-formed.
        let s = Fish
            .format_set(
                OsStr::new("VSINSTALLDIR"),
                OsStr::new(r"C:\Program Files\Microsoft Visual Studio\2022\Community\"),
            )
            .expect("trailing backslash should escape");
        assert_eq!(
            s,
            r"set -gx VSINSTALLDIR 'C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\'"
        );
    }

    #[test]
    fn kind_and_executable() {
        assert_eq!(Fish.kind(), ShellKind::Fish);
        assert_eq!(Fish.executable(), OsStr::new("fish.exe"));
    }

    #[test]
    fn translate_path_default_is_identity() {
        let out = Fish.translate_path(OsStr::new("PATH"), OsStr::new(r"C:\foo;D:\bar"));
        assert_eq!(out, OsStr::new(r"C:\foo;D:\bar"));
    }
}
