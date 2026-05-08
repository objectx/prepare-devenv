//! PowerShell (`pwsh.exe`) shell adapter.
//!
//! PowerShell's single-quoted string is the verbatim form: no escape
//! sequences other than the doubled `''` for an embedded `'`. We always wrap
//! the value in `'…'` and replace each `'` with `''`. The `pwsh.exe` →
//! `powershell.exe` fallback for older boxes is the runner's job (Task 8);
//! this adapter just names the preferred binary.

use std::ffi::OsStr;

use super::{Shell, ShellKind};
use crate::error::Result;

/// PowerShell adapter. Zero-sized.
pub struct Pwsh;

impl Shell for Pwsh {
    fn kind(&self) -> ShellKind {
        ShellKind::Pwsh
    }

    fn executable(&self) -> &OsStr {
        OsStr::new("pwsh.exe")
    }

    fn format_set(&self, name: &OsStr, value: &OsStr) -> Result<String> {
        let n = name.to_string_lossy();
        let v = value.to_string_lossy();
        let escaped = v.replace('\'', "''");
        Ok(format!("$env:{n} = '{escaped}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_set_simple() {
        let out = Pwsh
            .format_set(OsStr::new("X"), OsStr::new("value"))
            .expect("simple value should format");
        assert_eq!(out, "$env:X = 'value'");
    }

    #[test]
    fn format_set_doubles_single_quote() {
        let out = Pwsh
            .format_set(OsStr::new("X"), OsStr::new("a'b"))
            .expect("single quote should double, not error");
        assert_eq!(out, "$env:X = 'a''b'");
    }

    #[test]
    fn format_set_handles_path_chars() {
        // Backslash, semicolon, and `$` are all literal inside PowerShell's
        // single-quoted strings — no escaping needed.
        let out = Pwsh
            .format_set(OsStr::new("PATH"), OsStr::new(r"C:\foo;C:\bar;$env:USER"))
            .expect("path chars should pass through");
        assert_eq!(out, r"$env:PATH = 'C:\foo;C:\bar;$env:USER'");
    }

    #[test]
    fn kind_and_executable() {
        assert_eq!(Pwsh.kind(), ShellKind::Pwsh);
        assert_eq!(Pwsh.executable(), OsStr::new("pwsh.exe"));
    }

    #[test]
    fn translate_path_default_is_identity() {
        let out = Pwsh.translate_path(OsStr::new("PATH"), OsStr::new(r"C:\foo;D:\bar"));
        assert_eq!(out, OsStr::new(r"C:\foo;D:\bar"));
    }
}
