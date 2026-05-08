//! Nushell (`nu.exe`) shell adapter.
//!
//! Nushell's quoting model has two relevant string forms:
//!
//! - **Single-quoted (`'…'`)**: completely literal. No escape sequences at
//!   all — `\`, `$`, and even `"` are passed through verbatim. The only
//!   character you cannot include is `'` itself.
//! - **Double-quoted (`"…"`)**: supports C-style backslash escapes for
//!   `\\`, `\"`, `\n`, etc. `$` is also special inside double-quoted
//!   strings (string interpolation), so we escape it.
//!
//! Strategy: the common case (no embedded `'`) takes the single-quote path,
//! which needs zero escaping. When the value DOES contain `'` we fall back
//! to the double-quoted form and escape `\`, `"`, and `$` so the value
//! round-trips. This is more correct than the `\''` rule listed in some
//! older docs (which was never actually valid nushell) — see the upstream
//! string-literal docs at <https://www.nushell.sh/book/working_with_strings.html>.

use std::ffi::OsStr;

use super::{Shell, ShellKind};
use crate::error::Result;

/// Nushell adapter. Zero-sized.
pub struct Nu;

impl Shell for Nu {
    fn kind(&self) -> ShellKind {
        ShellKind::Nu
    }

    fn executable(&self) -> &OsStr {
        OsStr::new("nu.exe")
    }

    fn format_set(&self, name: &OsStr, value: &OsStr) -> Result<String> {
        let n = name.to_string_lossy();
        let v = value.to_string_lossy();
        if v.contains('\'') {
            // Switch to double-quoted form and escape the three characters
            // nushell treats as special inside `"…"`: `\`, `"`, `$`.
            //
            // Escape order is critical: backslash MUST be replaced first to
            // avoid re-escaping the `\\` we add for `"` and `$`. The current
            // order is `\` → `\\`, then `"` → `\"`, then `$` → `\$`.
            let escaped = v
                .replace('\\', r"\\")
                .replace('"', r#"\""#)
                .replace('$', r"\$");
            Ok(format!("$env.{n} = \"{escaped}\""))
        } else {
            Ok(format!("$env.{n} = '{v}'"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_set_simple() {
        // Spec scenario: backslash and `;` pass through literally inside
        // single quotes — nushell does no escape interpretation here.
        let out = Nu
            .format_set(OsStr::new("INCLUDE"), OsStr::new(r"C:\foo"))
            .expect("simple value should format");
        assert_eq!(out, r"$env.INCLUDE = 'C:\foo'");
    }

    #[test]
    fn format_set_simple_value() {
        let out = Nu
            .format_set(OsStr::new("X"), OsStr::new("value"))
            .expect("plain value should format");
        assert_eq!(out, "$env.X = 'value'");
    }

    #[test]
    fn format_set_with_single_quote() {
        // A literal `'` in the value can't live inside a single-quoted
        // nushell string (no escape exists), so we switch to double quotes
        // and escape backslash / quote / `$`.
        let out = Nu
            .format_set(OsStr::new("X"), OsStr::new("a'b"))
            .expect("single quote should switch to double-quoted form");
        assert_eq!(out, r#"$env.X = "a'b""#);
    }

    #[test]
    fn format_set_with_quote_and_backslash() {
        // A path-shaped value with a literal `'` exercises the escape order:
        // backslash must be doubled before we add escapes for `"` or `$`.
        let out = Nu
            .format_set(OsStr::new("X"), OsStr::new(r"C:\a'b"))
            .expect("backslash + quote should both escape");
        assert_eq!(out, r#"$env.X = "C:\\a'b""#);
    }

    #[test]
    fn format_set_with_quote_and_dollar() {
        let out = Nu
            .format_set(OsStr::new("X"), OsStr::new(r"$VAR'val"))
            .expect("dollar + quote should escape");
        assert_eq!(out, r#"$env.X = "\$VAR'val""#);
    }

    #[test]
    fn format_set_passes_dollar_through_in_single_quote_form() {
        // No `'` → stays in single-quote form, where `$` is not special.
        let out = Nu
            .format_set(OsStr::new("X"), OsStr::new(r"$VAR"))
            .expect("dollar without quote should stay literal");
        assert_eq!(out, r"$env.X = '$VAR'");
    }

    #[test]
    fn kind_and_executable() {
        assert_eq!(Nu.kind(), ShellKind::Nu);
        assert_eq!(Nu.executable(), OsStr::new("nu.exe"));
    }

    #[test]
    fn translate_path_default_is_identity() {
        let out = Nu.translate_path(OsStr::new("PATH"), OsStr::new(r"C:\foo;D:\bar"));
        assert_eq!(out, OsStr::new(r"C:\foo;D:\bar"));
    }
}
