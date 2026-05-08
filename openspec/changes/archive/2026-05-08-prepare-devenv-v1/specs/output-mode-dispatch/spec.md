## ADDED Requirements

### Requirement: Three mutually-exclusive output modes

The tool SHALL support exactly three output modes determined at CLI-parse time and MUST reject any combination of them: spawn-detected-shell (default — selected when no `-- COMMAND` is given and `--emit` is absent), emit-eval-able-commands (selected by `--emit`), and spawn-user-command (selected by the presence of any positional arg after `--`). The `--shell` flag MUST be honored by the spawn-shell and emit modes; it MUST be silently ignored by the spawn-user-command mode.

#### Scenario: no flags, no -- COMMAND

- **WHEN** the user runs `prepare-devenv` with no `--emit` and no `-- COMMAND`
- **THEN** the dispatcher selects spawn-detected-shell mode

#### Scenario: --emit alone

- **WHEN** the user runs `prepare-devenv --emit`
- **THEN** the dispatcher selects emit mode

#### Scenario: -- COMMAND given

- **WHEN** the user runs `prepare-devenv -- bash -c "cl /?"`
- **THEN** the dispatcher selects spawn-user-command mode with argv `["bash", "-c", "cl /?"]`

#### Scenario: --emit and -- COMMAND together

- **WHEN** the user runs `prepare-devenv --emit -- bash`
- **THEN** clap rejects the combination and the tool exits with code `2`

---

### Requirement: Spawn detected/overridden shell with merged env

In spawn-detected-shell mode the tool SHALL build a merged env block by overlaying the env diff (added + modified) onto the pre-snapshot, MUST run the chosen shell adapter's `translate_path` over each (name, value) before placing it in the merged env (so `bash` gets POSIX-style `PATH`), MUST spawn the shell adapter's `executable()` interactively with stdio inherited from the parent, and MUST exit with the shell child's exit code on completion.

#### Scenario: parent is pwsh, no --shell

- **WHEN** the user runs `prepare-devenv` from `pwsh.exe`
- **THEN** the tool spawns `pwsh.exe` with the merged env, the user's stdio attached, and exits with `pwsh`'s eventual exit code

#### Scenario: --shell bash from cmd parent applies POSIX PATH

- **WHEN** the user runs `prepare-devenv --shell bash` from `cmd.exe`
- **THEN** the spawned `bash.exe` sees `PATH` in `/c/...:` POSIX form while `INCLUDE`/`LIB` keep their Windows form

#### Scenario: shell child exits with code 130

- **WHEN** the user `Ctrl-C`s out of the spawned shell, which exits with code 130
- **THEN** the tool exits with code 130 (pass-through)

---

### Requirement: Emit eval-able commands to stdout

In emit mode the tool SHALL write to stdout one `format_set`-emitted line per (name, value) in `added ∪ modified`, MUST apply `translate_path` before formatting, MUST emit `added` entries in sorted-key order before `modified` entries (also sorted), MUST end the output with a trailing newline, and MUST keep stdout otherwise pristine (no banners, no diagnostics — those go to stderr).

#### Scenario: emit for bash with PATH translation

- **WHEN** the user runs `prepare-devenv --emit --shell bash`
- **THEN** stdout contains `export` lines for every added/modified var, with `PATH` translated to POSIX form, in sorted-by-name order with added vars before modified vars

#### Scenario: emit for cmd

- **WHEN** the user runs `prepare-devenv --emit --shell cmd`
- **THEN** stdout contains `set "NAME=VALUE"` lines per the format rules, terminated by a trailing newline

#### Scenario: emit produces no diagnostics on stdout

- **WHEN** the user runs `prepare-devenv --emit -v`
- **THEN** all `info!`-level log lines appear on stderr and stdout contains only the `format_set` lines

---

### Requirement: Spawn user command with merged env

In spawn-user-command mode the tool SHALL build the same merged env as spawn-detected-shell mode but MUST NOT apply per-shell `translate_path` (the user's command receives the env in Windows-native form), MUST spawn the user's argv directly with stdio inherited, and MUST exit with the spawned command's exit code (passing through values up to and including 255).

#### Scenario: -- where cl.exe

- **WHEN** the user runs `prepare-devenv -- where cl.exe` and the resolved VS install has MSVC's `cl.exe` on its PATH
- **THEN** the spawned `where` prints the path to MSVC's `cl.exe` to stdout and exits 0; the tool exits 0

#### Scenario: spawned command not found

- **WHEN** the user runs `prepare-devenv -- nonexistent.exe`
- **THEN** the tool exits with code `1` (spawn failure) with an error indicating the command could not be started

#### Scenario: spawned command exits non-zero

- **WHEN** the user runs `prepare-devenv -- cmd /c exit 42`
- **THEN** the tool exits with code `42`
