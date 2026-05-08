//! Shell-adapter trait and parent-shell auto-detection.
//!
//! The crate exposes a single [`Shell`] trait whose contract is small enough to
//! keep every implementation a few lines: a [`kind`](Shell::kind) discriminant,
//! the [`executable`](Shell::executable) name to spawn, a
//! [`format_set`](Shell::format_set) producing one eval-able assignment line in
//! the shell's native syntax, and an optional [`translate_path`](Shell::translate_path)
//! hook (defaulting to identity) for adapters that need to rewrite values
//! before emission — currently only `bash`, which converts Windows-style
//! `PATH` segments into POSIX form.
//!
//! Concrete impls live in sibling modules ([`cmd`], [`pwsh`], [`bash`], [`fish`],
//! [`nu`]) and are surfaced through the [`for_kind`] factory. Auto-detection
//! ([`detect`]) walks the parent-process chain via the Windows ToolHelp APIs;
//! the unit-testable variant [`detect_with`] takes the parent names and env
//! hints as parameters so the selection logic can be exercised without
//! actually spawning child processes.
//!
//! Detection is intentionally infallible: if no ancestor matches and no env
//! hint applies, we fall back to `cmd` and emit a `tracing::warn!` event so
//! a `-v` run still surfaces what happened.

use std::ffi::{OsStr, OsString};

pub use crate::cli::ShellKind;
use crate::error::Result;

mod bash;
mod cmd;
mod fish;
mod nu;
mod pwsh;

/// One shell-syntax adapter. Each supported shell implements this trait.
///
/// The trait is deliberately small: callers only need to know how to spawn
/// the shell ([`executable`](Self::executable)), how to render one variable
/// assignment in the shell's syntax ([`format_set`](Self::format_set)), and —
/// for shells whose value semantics differ from Windows (`bash` PATH only
/// today) — how to rewrite a value before formatting ([`translate_path`](Self::translate_path)).
///
/// `Shell` is **not** required to be `Send + Sync`: the binary is single-
/// threaded and we hand the trait object straight to the runner.
pub trait Shell {
    /// The [`ShellKind`] discriminant this adapter implements.
    fn kind(&self) -> ShellKind;

    /// Image name of the binary to spawn for spawn-shell mode.
    ///
    /// E.g. `cmd.exe`, `pwsh.exe`, `bash.exe`. The runner is responsible for
    /// any fallback search (`pwsh.exe` → `powershell.exe`, etc.).
    fn executable(&self) -> &OsStr;

    /// Render `NAME=VALUE` as one eval-able line in this shell's syntax.
    ///
    /// Returns `Err(Error::EnvParse(_))` only when the value cannot be
    /// represented at all in the shell's quoting rules — currently this is
    /// just `cmd.exe`'s inability to embed a literal `"`.
    fn format_set(&self, name: &OsStr, value: &OsStr) -> Result<String>;

    /// Optional value rewrite, keyed on the variable name.
    ///
    /// Default returns the value unchanged. The `bash` adapter overrides this
    /// to translate `PATH` from Windows-style (`C:\foo;D:\bar`) into POSIX
    /// (`/c/foo:/d/bar`).
    fn translate_path(&self, _name: &OsStr, value: &OsStr) -> OsString {
        value.to_owned()
    }
}

/// Factory: build a boxed [`Shell`] adapter for the given [`ShellKind`].
#[must_use]
pub fn for_kind(kind: ShellKind) -> Box<dyn Shell> {
    match kind {
        ShellKind::Cmd => Box::new(cmd::Cmd),
        ShellKind::Pwsh => Box::new(pwsh::Pwsh),
        ShellKind::Bash => Box::new(bash::Bash),
        ShellKind::Fish => Box::new(fish::Fish),
        ShellKind::Nu => Box::new(nu::Nu),
    }
}

/// Env-hint inputs to [`detect_with`].
///
/// Captured separately from the live OS lookup so unit tests can drive the
/// fallback chain deterministically. See [`detect`] for the production
/// fillers.
#[derive(Default, Debug)]
pub(crate) struct EnvHints {
    /// `MSYSTEM` value if set (Git-Bash / MSYS2 sets this to e.g. `MINGW64`).
    pub msystem: Option<String>,
    /// `COMSPEC` value if set (typically `C:\Windows\System32\cmd.exe`).
    pub comspec: Option<String>,
    /// `SHELL` value if set (typically `/usr/bin/bash` under MSYS).
    pub shell: Option<String>,
}

/// Auto-detect the parent shell.
///
/// Walks the parent-process chain via `CreateToolhelp32Snapshot` /
/// `Process32FirstW` / `Process32NextW`, then falls back to env-var hints,
/// then to `cmd.exe` (with a warning log). Always returns a value — never
/// fails. See module docs for the lookup ladder.
#[must_use]
pub fn detect() -> ShellKind {
    let parents = walk_parent_chain();
    let parent_refs: Vec<&str> = parents.iter().map(String::as_str).collect();
    let hints = EnvHints {
        msystem: std::env::var("MSYSTEM").ok(),
        comspec: std::env::var("COMSPEC").ok(),
        shell: std::env::var("SHELL").ok(),
    };
    detect_with(&parent_refs, &hints)
}

/// Test-friendly core of [`detect`].
///
/// `parents` is the ancestor chain ordered immediate-parent-first; basenames
/// like `bash.exe` or full paths are both acceptable (we only inspect the
/// final path component lower-cased).
pub(crate) fn detect_with(parents: &[&str], hints: &EnvHints) -> ShellKind {
    for raw in parents {
        if let Some(kind) = match_image_name(raw) {
            return kind;
        }
    }

    if hints
        .msystem
        .as_deref()
        .is_some_and(|v| !v.trim().is_empty())
    {
        return ShellKind::Bash;
    }
    if let Some(comspec) = hints.comspec.as_deref() {
        let lower = basename_lower(comspec);
        if lower == "cmd.exe" {
            return ShellKind::Cmd;
        }
    }
    if let Some(sh) = hints.shell.as_deref() {
        let lower = basename_lower(sh);
        if lower == "bash" || lower == "bash.exe" {
            return ShellKind::Bash;
        }
    }

    tracing::warn!("falling back to cmd; could not detect parent shell");
    ShellKind::Cmd
}

/// Match an image-name string against the supported shells.
///
/// Returns `None` when the basename is something else (e.g. a launcher like
/// `WindowsTerminal.exe`) so the caller can keep walking up the chain.
fn match_image_name(image: &str) -> Option<ShellKind> {
    let lower = basename_lower(image);
    match lower.as_str() {
        "cmd.exe" => Some(ShellKind::Cmd),
        "pwsh.exe" | "powershell.exe" => Some(ShellKind::Pwsh),
        "bash.exe" | "sh.exe" | "zsh.exe" => Some(ShellKind::Bash),
        "fish.exe" => Some(ShellKind::Fish),
        "nu.exe" => Some(ShellKind::Nu),
        _ => None,
    }
}

/// Lower-case the final path component of a string for case-insensitive
/// shell-image comparisons. Splits on both `\` and `/` to be tolerant of
/// either separator (env values can land in either form on Windows).
fn basename_lower(s: &str) -> String {
    let trimmed = s.trim();
    let after_slash = trimmed.rsplit(['\\', '/']).next().unwrap_or(trimmed);
    after_slash.to_ascii_lowercase()
}

/// Walk the parent-process chain and return image names ordered immediate-
/// parent first. Stops after a small bound to keep us robust against any
/// pathological cycle in the system snapshot.
///
/// Returns an empty `Vec` on any toolhelp failure — `detect_with` then falls
/// back to env hints. Walking is best-effort; we never propagate Win32 errors
/// out of detection.
fn walk_parent_chain() -> Vec<String> {
    use windows::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    const MAX_DEPTH: usize = 16;

    let mut chain: Vec<String> = Vec::new();

    // SAFETY: the toolhelp APIs are FFI-only; we group the calls together and
    // close the snapshot via RAII below. The struct is initialised to zeros
    // (per `PROCESSENTRY32W::default`) and dwSize is set before the first call
    // as required by Win32.
    unsafe {
        let snapshot = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(h) if !h.is_invalid() && h != INVALID_HANDLE_VALUE => h,
            _ => return chain,
        };

        let mut entry = PROCESSENTRY32W {
            dwSize: u32::try_from(std::mem::size_of::<PROCESSENTRY32W>()).unwrap_or(0),
            ..Default::default()
        };

        // Build a (pid, parent_pid, exe_name) table for the whole snapshot.
        let mut table: Vec<(u32, u32, String)> = Vec::new();
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                table.push((
                    entry.th32ProcessID,
                    entry.th32ParentProcessID,
                    exe_name_from_buf(&entry.szExeFile),
                ));
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);

        // Walk parent chain starting from our own PID.
        let mut pid: u32 = std::process::id();
        for _ in 0..MAX_DEPTH {
            let Some((_, parent_pid, _)) = table.iter().find(|row| row.0 == pid).cloned() else {
                break;
            };
            // 0 / 4 are System / Idle on Windows; no useful name to report.
            if parent_pid == 0 || parent_pid == pid {
                break;
            }
            let Some((_, _, name)) = table.iter().find(|row| row.0 == parent_pid).cloned() else {
                break;
            };
            chain.push(name);
            pid = parent_pid;
        }
    }

    chain
}

/// Decode `PROCESSENTRY32W::szExeFile` (a null-terminated `[u16; 260]`) into a
/// String. Stops at the first NUL; if there's no NUL we take the whole buffer.
fn exe_name_from_buf(buf: &[u16; 260]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_hints() -> EnvHints {
        EnvHints::default()
    }

    #[test]
    fn detect_with_bash_msys() {
        let kind = detect_with(
            &["bash.exe"],
            &EnvHints {
                msystem: Some("MINGW64".into()),
                ..Default::default()
            },
        );
        assert_eq!(kind, ShellKind::Bash);
    }

    #[test]
    fn detect_with_pwsh() {
        let kind = detect_with(&["pwsh.exe"], &no_hints());
        assert_eq!(kind, ShellKind::Pwsh);
    }

    #[test]
    fn detect_with_powershell_alias() {
        let kind = detect_with(&["powershell.exe"], &no_hints());
        assert_eq!(kind, ShellKind::Pwsh);
    }

    #[test]
    fn detect_with_launcher_then_cmd() {
        // Immediate parent is a launcher; we must skip past it and pick the
        // cmd ancestor further up the chain.
        let kind = detect_with(&["WindowsTerminal.exe", "cmd.exe"], &no_hints());
        assert_eq!(kind, ShellKind::Cmd);
    }

    #[test]
    fn detect_with_full_path_basename() {
        // Image names can arrive as full paths from some snapshot rows; the
        // detector lower-cases just the basename.
        let kind = detect_with(&[r"C:\Program Files\Git\bin\bash.exe"], &no_hints());
        assert_eq!(kind, ShellKind::Bash);
    }

    #[test]
    fn detect_with_no_recognizable_shell_falls_back_to_cmd() {
        let kind = detect_with(&["something.exe"], &no_hints());
        assert_eq!(kind, ShellKind::Cmd);
    }

    #[test]
    fn detect_with_msystem_hint_falls_back_to_bash() {
        let kind = detect_with(
            &[],
            &EnvHints {
                msystem: Some("MINGW64".into()),
                ..Default::default()
            },
        );
        assert_eq!(kind, ShellKind::Bash);
    }

    #[test]
    fn detect_with_comspec_hint_falls_back_to_cmd() {
        let kind = detect_with(
            &[],
            &EnvHints {
                comspec: Some(r"C:\Windows\System32\cmd.exe".into()),
                ..Default::default()
            },
        );
        assert_eq!(kind, ShellKind::Cmd);
    }

    #[test]
    fn detect_with_shell_hint_falls_back_to_bash() {
        let kind = detect_with(
            &[],
            &EnvHints {
                shell: Some("/usr/bin/bash".into()),
                ..Default::default()
            },
        );
        assert_eq!(kind, ShellKind::Bash);
    }

    #[test]
    fn detect_with_empty_msystem_does_not_promote() {
        // An MSYSTEM that is set but empty should not be treated as a hit;
        // fall through to the next rung.
        let kind = detect_with(
            &[],
            &EnvHints {
                msystem: Some("   ".into()),
                ..Default::default()
            },
        );
        assert_eq!(kind, ShellKind::Cmd);
    }

    #[test]
    fn for_kind_returns_correct_adapter_for_each_variant() {
        for k in [
            ShellKind::Cmd,
            ShellKind::Pwsh,
            ShellKind::Bash,
            ShellKind::Fish,
            ShellKind::Nu,
        ] {
            let s = for_kind(k);
            assert_eq!(s.kind(), k, "for_kind({k:?}).kind() should round-trip");
        }
    }

    #[test]
    fn basename_lower_handles_separators() {
        assert_eq!(basename_lower(r"C:\Windows\System32\cmd.exe"), "cmd.exe");
        assert_eq!(basename_lower("/usr/bin/bash"), "bash");
        assert_eq!(basename_lower("PWSH.EXE"), "pwsh.exe");
    }
}
