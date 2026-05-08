## Context

The repository is greenfield: only `README.md`, `CLAUDE.md`, `docs/WORKFLOW.md`, and the OpenSpec / superpowers-bridge scaffolding exist — no source code, build system, or chosen language. The README defines a Windows utility called `prepare-devenv` that locates a specific Visual Studio install, runs its `VsDevCmd.bat`, captures the resulting environment delta, and either spawns a child process or applies the env to the calling shell.

The interesting design constraints come from Windows specifics:

- **`VsDevCmd.bat` only mutates the batch's own process env.** It must run inside a child `cmd.exe` that prints its env afterward, and the env must be parsed back from that child's stdout. It cannot be sourced into a non-`cmd` shell directly.
- **Visual Studio installs are enumerated via `vswhere.exe`**, which ships with the VS Installer at a known path (`%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe`). Its `-format json` output is the canonical source of truth.
- **`cmd.exe` writes its `set` output in the active console code page by default**, which makes round-tripping non-ASCII env values fragile. Forcing UTF-16 stdout via `cmd /U` is the documented escape hatch.
- **Different shells need different output syntax** for env vars (`set NAME=VAL`, `$env:NAME = 'VAL'`, `export NAME=VAL`, `set -gx NAME VAL`, `$env.NAME = 'VAL'`), and `bash` running on MSYS / Git Bash expects POSIX-style `PATH` (`/c/...`, `:` separator) even though `INCLUDE`/`LIB`/etc. must remain in Windows form for the MSVC toolchain to consume them.

Stakeholders are the project author (single user, dev-tools workflow) and any future contributors. There is no production deployment surface — the artifact is a single Windows `.exe` distributed by whatever channel the author chooses later.

## Goals / Non-Goals

**Goals:**

- A single static `prepare-devenv.exe` (Rust 2024 edition) that, given no arguments, drops the user back into their detected parent shell with the latest installed VS's developer environment merged in.
- Selectable VS install via `--id <prefix>` or `--path <dir>`, falling back to "latest by version" when neither is given.
- An `--emit` mode that prints eval-able set/export commands for the parent shell, so the user can `prepare-devenv --emit | iex` (or shell-equivalent) and persist the env in their *current* process.
- A pass-through mode (`-- COMMAND ARGS...`) that spawns `COMMAND` with the merged env and forwards its exit code unchanged.
- First-class shell support for `cmd`, `pwsh`, `bash` (incl. MSYS / Git Bash with `PATH`-only POSIX translation), `fish`, and `nu`, with auto-detection of the parent shell and a `--shell` override.
- Predictable, parsable error reporting: distinct exit codes for "tool broken" vs "VS not found" vs "vsdevcmd failed" vs "user CLI error".
- Verbose logging via `tracing` (`-v` info, `-vv` debug) to stderr without polluting stdout (which is reserved for `--emit`).
- Internal seams that allow unit-testing the discovery / capture / diff / shell-format paths without an installed VS.

**Non-Goals:**

- Cross-platform support. Windows-only; no `cfg(unix)` matrix.
- Shipping `vswhere.exe` ourselves. We require it at the standard install location (or on `%PATH%`).
- Reimplementing VS install enumeration (no registry / `state.json` parsing). `vswhere` is the only source of truth.
- Translating non-`PATH` variables to POSIX form for `bash`. `INCLUDE`, `LIB`, `LIBPATH`, etc. are consumed by Windows binaries and stay in Windows form.
- Honoring vars *removed* by `VsDevCmd.bat` (rare, and a buggy detection there could nuke unrelated user vars).
- Persisting the env into the parent shell process when no `--emit` flag is given. The default no-args mode spawns a *new* shell instance — a child cannot mutate its parent.
- Distribution / packaging (Scoop manifest, GitHub Releases workflow) and signing — out of v1 scope.
- A library API for embedding from another Rust program. Internal modules are the only consumer for v1.

## Decisions

### D1: Language / toolchain — Rust 2024 edition

**Chosen** because it produces a single static `.exe` with no runtime dependency, gives us strong typing for the env-diff and shell-formatting logic, and has good Windows process / encoding APIs via the `windows` crate.

**Alternatives considered:** Go (simpler but more awkward for Windows-specific quirks like UTF-16 stdout); C# .NET 8 AOT (first-class on Windows but larger artifact, AOT caveats); PowerShell script (no build but harder to ship as one binary, awkward spawn-with-merged-env story).

### D2: No-`--` mode is "spawn the parent shell"; `--emit` is opt-in

**Chosen** because spawning a fresh interactive shell with the merged env "just works" everywhere — the user is dropped back into their familiar prompt. `--emit` covers the in-process use case (`prepare-devenv --emit | iex`) when surviving past the child shell matters.

**Alternatives:** "always emit, never spawn" (forces every user to learn the eval pattern); "spawn only" (no path to persist env in the calling process). The combination is the only one that handles both interactive and scripting needs without surprising the user.

### D3: Shell scope — `cmd`, `pwsh`, `bash` (MSYS), `fish`, `nu`

**Chosen** because the README's own example (`-- bash`) implies Git Bash use, and the project author runs all five in different contexts.

**Trade-off:** five `Shell` trait impls and five quoting/escaping rules to maintain. Mitigated by a small, well-defined `trait Shell` surface and a shared fixture suite per shell.

### D4: Shell selection — auto-detect parent, override with `--shell`

**Chosen** so `prepare-devenv` (no args) drops the user back into the same shell they invoked from. Detection walks the parent-process chain via Windows toolhelp / NT APIs and consults `MSYSTEM` / `COMSPEC` / `$env:SHELL`. `--shell` short-circuits detection and is required for piping into a different shell (`prepare-devenv --emit --shell bash > setup.sh`).

**Alternatives:** "always require `--shell`" (predictable but verbose); "auto-detect for spawn, require for emit" (asymmetric, more flags to remember). Auto-detect-with-override matches the principle of least surprise.

### D5: `vswhere.exe` — required at standard install path

**Chosen** because `vswhere` ships with the VS Installer for VS 2017+ and is essentially always present on machines that have VS at all. Looking at `%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe` (and `%PATH%`) covers ≥99 % of cases.

**Alternatives:** bundling `vswhere.exe` via `include_bytes!` (~250 KB, plus tracking upstream releases — overkill); reimplementing enumeration ourselves against the installer's `state.json` (highest maintenance cost, breaks across VS versions).

### D6: Env diff — added + modified vars only, ignore removals

**Chosen** because `VsDevCmd.bat` is overwhelmingly additive and the few vars it does mutate are fully replaced by it (so the post-value is already correct to forward). Honoring removals is theoretically more faithful but creates a footgun if the diff misclassifies a transient/local var as "removed" and we then unset it on the user.

### D7: POSIX `PATH` translation, only `PATH`, only for `bash`

**Chosen** because:

- `bash` interprets `PATH` directly; it expects `:`-separated POSIX-form paths or it can't find anything.
- `INCLUDE`, `LIB`, `LIBPATH`, `WindowsSdkDir`, `VCINSTALLDIR`, etc. are read by Windows binaries (`cl.exe`, `link.exe`); they must stay in `;`-separated Windows form.

The translation: split the `PATH` value on `;`, convert each `[A-Za-z]:\…` segment to `/<lower-drive>/…` (forward slashes), join with `:`. Everything else passes through verbatim.

**Alternatives:** "translate everything path-shaped" (heuristic, breaks `INCLUDE` for `cl.exe`); "translate nothing, document the workaround" (default UX is broken for the headline `-- bash` example).

### D8: Capture mechanism — `cmd /U /D /Q /C "call <bat> <args> 1>&2 && echo <MARKER> && set"`

- `/U` forces UTF-16LE stdout — code-page-independent.
- `/D` skips `AutoRun` registry hooks that could pollute the captured env.
- `/Q` mutes the prompt.
- `1>&2` redirects `VsDevCmd.bat`'s chatty stdout to *our* stderr, where it's discarded by default and surfaced under `-v`.
- A unique per-invocation marker line lets us robustly find where `set` output begins, even if some buggy step writes to stdout before `set`.
- `&&` short-circuits on `VsDevCmd.bat` failure → exit code 4.

**Alternatives:** redirecting env to a temp file (extra IO, extra cleanup, no fundamental gain); using `cmd /A` ANSI output (locale-dependent, breaks non-ASCII paths).

### D9: Architecture — single Cargo crate, internal modules

Modules: `cli`, `discovery`, `capture`, `diff`, `shell/{cmd,pwsh,bash,fish,nu,mod}`, `runner`, `error`. One `main.rs` orchestrates, with all errors funneling through `error::Error` (`thiserror`-derived) and surfaced via `anyhow::Context` at the boundary.

**Alternative considered:** workspace with `prepare-devenv-core` lib + thin bin crate. Deferred: no second consumer today, splitting can be done later without changing user-facing behavior.

### D10: Testability seams — IO at the edges

Public `discovery::resolve(selector)` delegates to `discovery::resolve_with(selector, fetch_json)`, where `fetch_json` is the function that runs `vswhere`. Tests call `resolve_with` with fixture data.

Public `capture::capture(bat, args)` similarly delegates to `capture_with(reader)`, where `reader` provides the bytes that *would* come from the `cmd` child. Tests feed in committed UTF-16LE fixture bytes.

A `test_hooks` Cargo feature exposes a hidden `--devcmd-script <path>` flag so end-to-end tests can drive the full pipeline against a fake `.bat` whose only job is to `set` known vars.

A `live` Cargo feature gates tests that hit the real `vswhere` and `VsDevCmd.bat`; manual / self-hosted runner only.

### D11: Diagnostics — `tracing`, exit codes, suppression policy

`tracing-subscriber` writes to stderr at a level set by `-v` (`info`) / `-vv` (`debug`). Stdout is reserved for `--emit` output so it remains pipe-clean.

Exit code map:

- `0` — success or `-- COMMAND` exited 0
- `1` — generic failure (IO, env-parse, internal bug)
- `2` — bad CLI usage (clap-emitted)
- `3` — discovery failed (`vswhere` missing, no installs, no/ambiguous match, `VsDevCmd.bat` missing)
- `4` — `VsDevCmd.bat` itself returned non-zero
- *N* — pass-through of the spawned `-- COMMAND`'s exit code

`VsDevCmd.bat`'s stdout (redirected to our stderr by the inner `cmd` via `1>&2`) is captured-and-discarded by default and forwarded to the user's stderr at `-v` or higher.

## Risks / Trade-offs

- **[Risk] `cmd /U` UTF-16LE output handling has BOM and line-ending quirks** that may differ across Windows versions / regional settings → **Mitigation**: commit a fixture set capturing real `cmd /U` output from at least Windows 10 + Windows 11; parser strips BOM and normalizes both `\r\n` and lone `\n`.
- **[Risk] Parent-process detection is racy** — the parent could exit between our `getppid` and the toolhelp snapshot, or run under a launcher (e.g., Windows Terminal) that obscures the real shell → **Mitigation**: walk *up* the chain (not just the immediate parent); fall back to env hints (`MSYSTEM`, `COMSPEC`, `$env:SHELL`); ultimate fallback to `cmd`. `--shell` is always available as a manual override.
- **[Risk] `VsDevCmd.bat` arg quoting** — a user passing `--devcmd-args="-arch=x64 -host_arch=x64"` expects the inner string to reach the bat verbatim, but Windows command-line quoting can mangle embedded quotes → **Mitigation**: forward the string as a single argument to `cmd /C` inside outer double-quotes; document that embedded `"` must be doubled per `cmd` rules; commit a fixture covering common cases.
- **[Risk] PATH translation edge cases for MSYS** — UNC paths (`\\server\share`), `/d/...` ambiguity, paths with spaces or parens → **Mitigation**: dedicated unit tests in `shell::bash::translate_path`; UNC paths pass through verbatim (rare in `PATH`); `format_set` handles quoting after translation.
- **[Risk] `vswhere` JSON schema drift between VS versions** — fields could be renamed/removed → **Mitigation**: deserialize into a struct that uses `#[serde(default)]` for non-essential fields; only `installationPath`, `installationVersion`, `instanceId` are required; pin a `vswhere` schema fixture for tests.
- **[Trade-off] Single-crate vs workspace** — chose simplicity over upfront reuse. Carving out a `core` crate is a mechanical refactor when (and if) a second consumer arrives.
- **[Trade-off] Five-shell support burden** — five `format_set` impls and quoting rules to maintain. Acceptable: each impl is small and the trait surface is narrow.
- **[Trade-off] `--emit` reserves stdout for shell commands** — diagnostics must go to stderr. Already enforced by routing all output through `tracing-subscriber` (stderr) and the `runner::emit` writer (stdout). A test asserts stdout is empty for non-`--emit` modes.

## Open Questions

These do not block implementation; each has a working default that can be revisited during build.

- **Single-quote escaping conventions per shell**: working scheme is to wrap in single quotes and escape embedded `'` (`'\''` for `bash`, `''` for `pwsh`). Verify against fixtures.
- **Whether to special-case `cmd` `set "NAME=VAL"`** when `VAL` contains a literal `"` — best practice is to error rather than emit broken syntax. Confirm during implementation.
- **Distribution channel** (Scoop, Chocolatey, GitHub Releases, Cargo crate) — explicitly out of v1 scope but worth a note before a real release.
- **`--json` output format** — useful for IDE / tooling integration; deferred to v2.
- **Multiple-install enumeration command** (e.g., `prepare-devenv list`) — also v2.
