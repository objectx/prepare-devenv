# cli-surface-and-diagnostics Specification

## Purpose
TBD - created by archiving change prepare-devenv-v1. Update Purpose after archive.
## Requirements
### Requirement: Documented CLI flag set parsed by clap-derive

The CLI SHALL be defined via clap (derive macros) and MUST accept exactly these arguments: `--id <id>` (mutually exclusive with `--path`), `--path <path>` (mutually exclusive with `--id`), `--devcmd-args <s>` (a single forwarded string), `--shell <cmd|pwsh|bash|fish|nu>`, `--emit` (mutually exclusive with positional args after `--`), `-v`/`-vv` repeatable verbosity flag, `--help`, and a trailing argv collected with `last = true` after `--`. Unknown or malformed combinations MUST be rejected by clap with exit code 2.

#### Scenario: --id and --path mutually exclusive

- **WHEN** the user passes `--id X --path Y`
- **THEN** clap rejects the input and the tool exits with code `2`

#### Scenario: --emit and -- COMMAND mutually exclusive

- **WHEN** the user passes `--emit -- foo`
- **THEN** clap rejects the input and the tool exits with code `2`

#### Scenario: -- separates options from user argv

- **WHEN** the user passes `-- bash -c "cl /?"`
- **THEN** clap collects `["bash", "-c", "cl /?"]` as the trailing command argv and does not parse them as flags

#### Scenario: --help short-circuits

- **WHEN** the user passes `--help`
- **THEN** clap prints help to stdout and exits with code `0`

---

### Requirement: Documented exit-code map

The tool SHALL map error categories to a documented exit-code set: `0` for success or pass-through 0 from a spawned command; `1` for generic failure (IO, internal env-parse failure, spawn-failed); `2` for clap-emitted bad-CLI errors; `3` for discovery failures (`vswhere` missing, no installs, no/ambiguous match, `VsDevCmd.bat` missing); `4` for `VsDevCmd.bat` itself returning non-zero; and the spawned command's own exit code passed through unchanged in spawn-user-command mode.

#### Scenario: discovery failure surfaces as 3

- **WHEN** `vswhere` is not present at the standard path or on `%PATH%`
- **THEN** the tool exits with code `3`

#### Scenario: VsDevCmd.bat failure surfaces as 4

- **WHEN** `VsDevCmd.bat` returns a non-zero exit code (e.g., bad `--devcmd-args`)
- **THEN** the tool exits with code `4`

#### Scenario: spawned command exit code passes through

- **WHEN** the user runs `prepare-devenv -- cmd /c exit 17`
- **THEN** the tool exits with code `17`

---

### Requirement: tracing-based stderr diagnostics with -v / -vv

The tool SHALL initialize `tracing-subscriber` against stderr at a level controlled by `--verbose`: `0` enables only `error!`, `1` (`-v`) additionally enables `info!`, `2` (`-vv`) additionally enables `debug!`. Every diagnostic message — discovery progress, the constructed `cmd /U` line, the marker UUID, captured `VsDevCmd.bat` output, the chosen shell, the spawned executable — MUST go through `tracing` and reach stderr only.

#### Scenario: default verbosity emits no info logs

- **WHEN** the user runs `prepare-devenv` with no `-v`
- **THEN** stderr contains no `info!`-level lines (only error messages on failure)

#### Scenario: -v surfaces discovery and capture details

- **WHEN** the user runs `prepare-devenv -v`
- **THEN** stderr includes the resolved install id/path/version, the constructed `cmd /U …` line, the marker token, and the count of added/modified vars

#### Scenario: -vv surfaces full vswhere JSON and parent-process walk

- **WHEN** the user runs `prepare-devenv -vv`
- **THEN** stderr additionally includes the full `vswhere` JSON (one entry per install) and each step of the parent-process ancestor walk

---

### Requirement: stdout is reserved for --emit output

In any mode other than `--emit`, the tool MUST write nothing to stdout under default verbosity. Under `--emit`, the tool MUST write only `format_set` lines (one per env entry) to stdout — no banners, no `tracing` output, no progress text.

#### Scenario: spawn-shell mode writes nothing to stdout

- **WHEN** the user runs `prepare-devenv` from `pwsh` (no `--emit`) and the spawned `pwsh` produces no output before the user exits it
- **THEN** the tool itself wrote nothing to stdout (only the spawned shell's own output, if any, reached stdout)

#### Scenario: --emit -v keeps stdout clean

- **WHEN** the user runs `prepare-devenv --emit -v`
- **THEN** stdout contains only `format_set` lines and stderr contains all `info!` diagnostics

---

### Requirement: Surface VsDevCmd.bat's stdout only at -v or higher

`VsDevCmd.bat`'s own stdout (redirected to the inner `cmd.exe`'s stderr by the `1>&2` in the capture command) SHALL be captured and discarded at default verbosity, and MUST be forwarded to the user's stderr at `-v` or higher.

#### Scenario: default verbosity hides chatty banners

- **WHEN** the tool runs successfully at default verbosity and `VsDevCmd.bat` prints its usual "Initializing developer environment..." banner
- **THEN** none of that text reaches the user's stderr

#### Scenario: -v reveals the banner

- **WHEN** the same scenario is run with `-v`
- **THEN** the banner text is forwarded to the user's stderr (interleaved with `tracing` info logs)

---

### Requirement: Top-level error formatting via anyhow

`main` SHALL be the only place that converts `error::Error` values to user-visible output, MUST format them with `anyhow::Context`-style chaining (root cause + context lines), MUST write the formatted error to stderr, and MUST set the exit code per the exit-code map. Library modules MUST NOT print directly.

#### Scenario: error chain rendered with context

- **WHEN** discovery fails because `vswhere` is missing
- **THEN** the user sees a stderr message naming the probed installer path and `%PATH%`-search outcome, the tool exits with code `3`, and no library module wrote to stderr directly

