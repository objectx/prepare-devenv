# shell-adapter-suite Specification

## Purpose
TBD - created by archiving change prepare-devenv-v1. Update Purpose after archive.
## Requirements
### Requirement: Define a Shell trait covering format / executable / path-translation

The crate SHALL expose an internal `trait Shell` whose contract MUST include at minimum: a `kind()` returning a `ShellKind` discriminant, an `executable()` returning the binary name to spawn (e.g. `cmd.exe`, `pwsh.exe`, `bash.exe`, `fish.exe`, `nu.exe`), a `format_set(name, value)` returning a single line of shell syntax that sets one env var, and a `translate_path(name, value)` defaulting to identity but overridable per shell.

#### Scenario: every supported shell implements Shell

- **WHEN** the crate is compiled
- **THEN** concrete `Shell` impls exist for `cmd`, `pwsh`, `bash`, `fish`, and `nu`, and each is reachable via `shell::for_kind(ShellKind::*)`

---

### Requirement: Per-shell format_set syntax and quoting

Each `Shell` impl SHALL emit a single eval-able line in its native syntax: `cmd` MUST emit `set "NAME=VALUE"` with a literal `"` in `VALUE` causing a clear error rather than malformed output; `pwsh` MUST emit `$env:NAME = 'VALUE'` with embedded `'` doubled to `''`; `bash` MUST emit `export NAME='VALUE'` with embedded `'` escaped as `'\''`; `fish` MUST emit `set -gx NAME 'VALUE'` with embedded `'` escaped as `\'`; `nu` MUST emit `$env.NAME = 'VALUE'` with embedded `'` escaped as `\''`.

#### Scenario: cmd format with simple value

- **WHEN** `Shell::Cmd::format_set("INCLUDE", "C:\\foo;C:\\bar")` is invoked
- **THEN** the result is exactly `set "INCLUDE=C:\foo;C:\bar"`

#### Scenario: cmd format with embedded double quote rejected

- **WHEN** `Shell::Cmd::format_set("X", "a\"b")` is invoked
- **THEN** the call MUST surface an error rather than emit broken `set "X=a"b"` syntax

#### Scenario: pwsh format with embedded single quote

- **WHEN** `Shell::Pwsh::format_set("X", "a'b")` is invoked
- **THEN** the result is `$env:X = 'a''b'`

#### Scenario: bash format with embedded single quote

- **WHEN** `Shell::Bash::format_set("X", "a'b")` is invoked
- **THEN** the result is `export X='a'\''b'`

#### Scenario: fish format with simple value

- **WHEN** `Shell::Fish::format_set("INCLUDE", "C:\\foo")` is invoked
- **THEN** the result is `set -gx INCLUDE 'C:\foo'`

#### Scenario: nu format with simple value

- **WHEN** `Shell::Nu::format_set("INCLUDE", "C:\\foo")` is invoked
- **THEN** the result is `$env.INCLUDE = 'C:\foo'`

---

### Requirement: PATH-only POSIX translation for bash adapter

The `bash` adapter's `translate_path` SHALL return its input unchanged for any var name other than `PATH` (case-insensitive), and for `PATH` MUST split the value on `;`, MUST convert each `[A-Za-z]:[\\/]…` segment to `/<lower-drive-letter>/…` with `\` replaced by `/`, MUST preserve UNC paths and non-drive-letter segments verbatim, and MUST join the result with `:`.

#### Scenario: PATH segments with mixed slashes

- **WHEN** `Shell::Bash::translate_path("PATH", "C:\\foo;D:/bar baz;E:\\")` is invoked
- **THEN** the result is `/c/foo:/d/bar baz:/e/`

#### Scenario: drive-letter casing normalized

- **WHEN** the input contains `c:\X` and `C:\X`
- **THEN** both become `/c/X`

#### Scenario: empty PATH

- **WHEN** `Shell::Bash::translate_path("PATH", "")` is invoked
- **THEN** the result is `""`

#### Scenario: non-PATH var passes through

- **WHEN** `Shell::Bash::translate_path("INCLUDE", "C:\\foo;C:\\bar")` is invoked
- **THEN** the result equals the input verbatim

#### Scenario: UNC paths preserved

- **WHEN** the input PATH contains a `\\server\share\bin` segment
- **THEN** that segment passes through verbatim (joined with `:` separators)

---

### Requirement: Auto-detect parent shell

The crate SHALL provide a `shell::detect()` function that walks the parent-process chain via Windows process APIs (e.g., `CreateToolhelp32Snapshot`) to find the first ancestor whose image name matches a supported shell, falling back to `MSYSTEM`/`COMSPEC`/`$env:SHELL` env hints, with a final fallback to `cmd`. Detection MUST be infallible and MUST emit a `tracing::warn!` event when falling back to the default.

#### Scenario: parent process is bash.exe under MSYS

- **WHEN** the immediate parent is `bash.exe` and `MSYSTEM` is set
- **THEN** `detect()` returns `ShellKind::Bash`

#### Scenario: parent process is pwsh.exe

- **WHEN** the immediate parent is `pwsh.exe`
- **THEN** `detect()` returns `ShellKind::Pwsh`

#### Scenario: launcher obscures parent and ancestor is cmd.exe

- **WHEN** the immediate parent is a launcher (e.g., Windows Terminal) but a further ancestor is `cmd.exe`
- **THEN** `detect()` returns `ShellKind::Cmd`

#### Scenario: no recognizable shell in ancestor chain or env

- **WHEN** neither the ancestor walk nor env hints identify a known shell
- **THEN** `detect()` returns `ShellKind::Cmd` and emits a `tracing::warn!` log

---

### Requirement: --shell flag overrides detection

When the user supplies `--shell <kind>`, the tool SHALL bypass auto-detection entirely and use the requested adapter; an unknown value MUST be rejected by clap with exit code 2.

#### Scenario: --shell bash overrides cmd parent

- **WHEN** the parent process is `cmd.exe` but the user passes `--shell bash`
- **THEN** the tool uses the `bash` adapter for both spawn and emit

#### Scenario: --shell with unknown value

- **WHEN** the user passes `--shell zsh-but-not-supported`
- **THEN** clap rejects the input and the tool exits with code `2`

