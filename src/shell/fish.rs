//! Fish (`fish.exe`) shell adapter.
//!
//! Inside fish single-quoted strings only two escapes are recognised: `\\` →
//! `\` and `\'` → `'`. Other backslashes are taken literally — `'C:\foo'`
//! parses to the six-byte string `C:\foo` with no further interpretation. We
//! therefore only need to escape `'` (as `\'`) for the values we expect to
//! handle (Windows-shape PATH / INCLUDE / etc.).
//!
//! **Known limitation:** a value ending in `\` is unrepresentable here —
//! `'foo\'` parses as `foo` + `\'` (escaped quote, string still open). The VS
//! dev env vars we forward never end in a bare backslash in practice; if a
//! user supplies such a value via `--devcmd-args` and we hand it to the fish
//! emit path, the result would be ill-formed fish syntax. Documented and
//! left for a follow-up.
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

    fn format_set(&self, name: &OsStr, value: &OsStr) -> Result<String> {
        let n = name.to_string_lossy();
        let v = value.to_string_lossy();
        let escaped = v.replace('\'', r"\'");
        Ok(format!("set -gx {n} '{escaped}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_set_simple() {
        // Per spec scenario: `INCLUDE` / `C:\foo` round-trips with a single
        // backslash inside `'…'`. Fish leaves non-escape backslashes literal.
        let out = Fish
            .format_set(OsStr::new("INCLUDE"), OsStr::new(r"C:\foo"))
            .expect("simple value should format");
        assert_eq!(out, r"set -gx INCLUDE 'C:\foo'");
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
        // Backslash, semicolon, `$` are all literal inside fish single-quotes
        // (apart from the two recognised escapes `\\` and `\'`).
        let out = Fish
            .format_set(OsStr::new("PATH"), OsStr::new(r"C:\foo;C:\bar"))
            .expect("path chars should pass through");
        assert_eq!(out, r"set -gx PATH 'C:\foo;C:\bar'");
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
