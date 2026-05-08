# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

v1 implemented (Rust 2024 edition, Windows-only). Single binary `prepare-devenv.exe`. Source layout under `src/`: `main.rs` (orchestration), `cli.rs` (clap-derive), `discovery.rs` (vswhere lookup), `capture.rs` (`cmd /U` pipeline), `diff.rs` (env delta), `shell/{cmd,pwsh,bash,fish,nu}.rs` (per-shell adapters), `runner.rs` (output dispatch), `error.rs` (`thiserror`-based `Error`/`Result`).

Build/test/lint:

```pwsh
cargo build [--release]
cargo test                          # unit + fixture-driven (no VS required)
cargo test --features test_hooks    # adds e2e CLI suite via fake .bat
cargo test --features live          # requires installed VS
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --features test_hooks -- -D warnings
```

Cargo features: `test_hooks` (gates the hidden `--devcmd-script` flag for e2e tests), `live` (gates VS-installed integration tests). Both default off.

## What `prepare-devenv` does

A Windows-targeted CLI that bridges Visual Studio's developer environment into other shells/tools:

1. Locate a specific `vsdevcmd.bat` — selected by VS install path (`--path`) or instance ID (`--id`); otherwise the latest installed VS.
2. Invoke that `vsdevcmd.bat`, forwarding `--devcmd-args` to it.
3. Diff the environment before vs. after the batch ran (the batch mutates `PATH`, `INCLUDE`, `LIB`, `LIBPATH`, etc.).
4. Either spawn the command after `--` with the captured environment merged in, or — when no command is given — print/apply the env so the current shell inherits it.

The interesting design constraints live in steps 1 and 3:

- **Discovery**: Visual Studio installs are enumerated via `vswhere.exe` (typically `%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe`). The instance ID shown in `--id` examples (e.g. `d0b481ec`) is the short form of the GUID `vswhere -format json` returns.
- **Env capture**: `vsdevcmd.bat` only mutates the batch's own process env, so it must be run inside a child `cmd.exe` that prints the env afterward (e.g. `cmd /c "vsdevcmd.bat <args> && set"`), then parsed back. Don't try to source it into a non-cmd shell directly.

# Workflows

See @docs/WORKFLOW.md

## Lessons / corrections

Track recurring corrections in `tasks/lessons.md` (per the user's global workflow rules). File doesn't exist yet — create it on the first correction rather than pre-emptively.
