# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

Greenfield. The repository currently contains only [README.md](README.md) describing the intended tool — no source code, build system, tests, or language choice has been committed yet. When implementing, ask the user for the language/toolchain rather than assuming, then update this file with the resulting build/test/lint commands.

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
