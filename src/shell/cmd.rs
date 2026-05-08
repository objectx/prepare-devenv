//! `cmd.exe` shell adapter.
//!
//! `cmd.exe` has no escape sequence for a literal `"` inside a `set "X=…"`
//! statement (`^"` only works in unquoted contexts and breaks differently
//! between interactive and non-interactive parsing). Rather than emit syntax
//! that would silently mis-parse, [`Cmd::format_set`] returns
//! [`Error::EnvParse`] when the value contains a `"`. Every other byte —
//! including `;`, `&`, `|`, and the percent character — passes through
//! unchanged inside the surrounding quotes, which is `cmd`'s standard
//! deferred-expansion behaviour for `set "…"`.

use std::ffi::OsStr;

use super::{Shell, ShellKind};
use crate::error::{Error, Result};

/// `cmd.exe` adapter. Zero-sized — the trait holds no state.
pub struct Cmd;

impl Shell for Cmd {
    fn kind(&self) -> ShellKind {
        ShellKind::Cmd
    }

    fn executable(&self) -> &OsStr {
        OsStr::new("cmd.exe")
    }

    fn format_set(&self, name: &OsStr, value: &OsStr) -> Result<String> {
        let v = value.to_string_lossy();
        if v.contains('"') {
            return Err(Error::EnvParse(
                "cmd cannot represent a value containing a literal double quote",
            ));
        }
        let n = name.to_string_lossy();
        Ok(format!("set \"{n}={v}\""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_set_simple() {
        let out = Cmd
            .format_set(OsStr::new("INCLUDE"), OsStr::new(r"C:\foo;C:\bar"))
            .expect("simple value should format");
        assert_eq!(out, r#"set "INCLUDE=C:\foo;C:\bar""#);
    }

    #[test]
    fn format_set_passes_through_special_chars() {
        // `set "X=…"` defers expansion, so `&`, `|`, and `%` are all fine
        // inside the quotes. We don't try to be cleverer than cmd is.
        let out = Cmd
            .format_set(OsStr::new("X"), OsStr::new("a&b|c%d"))
            .expect("special chars should pass through");
        assert_eq!(out, r#"set "X=a&b|c%d""#);
    }

    #[test]
    fn format_set_rejects_double_quote() {
        let err = Cmd
            .format_set(OsStr::new("X"), OsStr::new(r#"a"b"#))
            .expect_err("a literal quote in the value must surface as an error");
        assert!(matches!(err, Error::EnvParse(_)), "got {err:?}");
        let msg = format!("{err}");
        assert!(msg.contains("double quote"), "msg={msg}");
    }

    #[test]
    fn kind_and_executable() {
        assert_eq!(Cmd.kind(), ShellKind::Cmd);
        assert_eq!(Cmd.executable(), OsStr::new("cmd.exe"));
    }

    #[test]
    fn translate_path_default_is_identity() {
        // cmd doesn't override translate_path; the default trait method must
        // return the input unchanged for any var (we sample PATH and INCLUDE).
        let out = Cmd.translate_path(OsStr::new("PATH"), OsStr::new(r"C:\foo;D:\bar"));
        assert_eq!(out, OsStr::new(r"C:\foo;D:\bar"));
        let out = Cmd.translate_path(OsStr::new("INCLUDE"), OsStr::new(r"C:\inc"));
        assert_eq!(out, OsStr::new(r"C:\inc"));
    }
}
