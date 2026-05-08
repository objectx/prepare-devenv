## Why

Visual Studio's developer environment is gated behind `VsDevCmd.bat`, which only mutates a `cmd.exe` child's own process env — useless if you live in PowerShell, Git Bash, fish, or nu. Today, getting `cl.exe` and the Windows SDK on PATH inside a non-`cmd` shell means either launching the "Developer Command Prompt" and losing your shell history, or hand-maintaining a fragile `eval`-able dump per machine. There is also no clean way to run a single command (e.g. `ninja`, `cmake`, a CI step) under that env without ceremony. The repository is greenfield with the design already converged, so v1 lands the headline tool: locate a VS install, capture its env delta, and surface it to whatever shell or child process the user actually uses.

## What Changes

This is a greenfield change — the entire `prepare-devenv` v1 is being introduced. Listed as additions; nothing is being modified or removed because nothing exists yet.

**Build / repo bootstrap**
- From: empty repo (only docs + OpenSpec scaffolding)
- To: Cargo project (`Cargo.toml`, `src/main.rs`), Rust 2024 edition, Windows-only target, CI workflow on a Windows runner running `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`
- Reason: nothing to build against today
- Impact: non-breaking (new files only)

**CLI surface**
- From: no executable
- To: `prepare-devenv [--id <id>|--path <path>] [--devcmd-args <s>] [--shell <kind>] [--emit] [-v|-vv] [-- COMMAND ARGS...]` with the exit-code map defined in `design.md` §D11
- Reason: user-facing contract for every downstream feature
- Impact: non-breaking

**VS-environment bridging behavior**
- From: users manually source `VsDevCmd.bat` from `cmd` and lose their shell on transition
- To: a single binary that captures the env delta and applies it to either the parent shell, an explicit `-- COMMAND`, or stdout-as-eval-able-script
- Reason: see "Why"
- Impact: net-new capability for users; they can wrap arbitrary commands or shells without leaving their workflow

## Capabilities

### New Capabilities

- `vs-installation-discovery`: locate `vswhere.exe` at the standard install path or on `%PATH%`, parse `vswhere -format json` output, and resolve a single Visual Studio install by `--id <prefix>`, `--path <dir>`, or "latest by version" with deterministic tie-breaking.
- `devenv-environment-capture`: invoke a resolved install's `VsDevCmd.bat` inside a `cmd /U /D /Q /C` child with a unique marker line, capture UTF-16LE stdout, parse the post-run env, and compute the added-and-modified-only delta against the parent process's env.
- `shell-adapter-suite`: a `trait Shell` plus concrete adapters for `cmd`, `pwsh`, `bash` (MSYS / Git Bash), `fish`, and `nu`, each providing parent-process detection, var-set syntax, single-quote escaping, executable name for spawn, and (for `bash` only) `PATH`-only POSIX path translation.
- `output-mode-dispatch`: three mutually-exclusive output strategies — spawn detected/overridden parent shell with merged env (default), `--emit` eval-able commands to stdout, `-- COMMAND` spawn with merged env and exit-code pass-through.
- `cli-surface-and-diagnostics`: clap-derived argument parsing with mutual-exclusion enforcement, the documented exit-code map (0/1/2/3/4/N), `tracing`-based stderr logging at `-v`/`-vv` levels, and the auto-detect-with-`--shell`-override convention.

### Modified Capabilities

None — greenfield project.

## Impact

- **New code**: entire `src/` tree (`cli.rs`, `discovery.rs`, `capture.rs`, `diff.rs`, `shell/{cmd,pwsh,bash,fish,nu,mod}.rs`, `runner.rs`, `error.rs`, `main.rs`), `tests/` integration suite with committed UTF-16LE and JSON fixtures, and a `Cargo.toml` declaring Rust 2024 edition.
- **Dependencies**: `clap` (derive), `serde` + `serde_json`, `thiserror`, `anyhow`, `tracing` + `tracing-subscriber`, `windows` (selective features for parent-process inspection), `assert_cmd` + `predicates` (dev-only). All from crates.io, all unconditional (Windows-only target).
- **External requirements**: `vswhere.exe` at `%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe` or on `%PATH%`. A working VS install (any 2017+) for live tests and end-user use. Neither is required for the unit / fixture-driven integration suite.
- **Cargo features**: `test_hooks` (gates a hidden `--devcmd-script <path>` flag for end-to-end tests), `live` (gates tests that hit real `vswhere` / `VsDevCmd.bat`).
- **CI**: a single Windows GitHub Actions runner job for fmt + clippy + cargo test; a separate, manually-triggered "live" job for self-hosted Windows-with-VS runners.
- **Distribution / packaging** (Scoop manifest, GitHub Releases, signing): explicitly out of v1 scope.
- **Backward compatibility**: not applicable — first release.
