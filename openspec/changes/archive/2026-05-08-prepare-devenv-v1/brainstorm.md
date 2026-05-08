## Design Summary

`prepare-devenv` is a Windows-only Rust 2024 CLI that bridges Visual Studio's developer environment (the side-effects of `VsDevCmd.bat`) into other shells and child processes. A single invocation:

1. Locates a specific Visual Studio install (latest, by `--id`, or by `--path`) using `vswhere.exe` at its standard installer path.
2. Runs that install's `VsDevCmd.bat` inside a captured `cmd.exe` child, forwarding `--devcmd-args` to it.
3. Diffs the child's post-`VsDevCmd` environment against the parent's pre-environment and keeps only added or modified variables.
4. Emits the diff via one of three output modes: spawn the user's parent shell with the merged env (default), `--emit` eval-able commands to stdout for the parent shell to source, or spawn an explicit `-- COMMAND` with the merged env and pass through its exit code.

It's organized as a single Rust crate with focused internal modules (`cli`, `discovery`, `capture`, `diff`, `shell/{cmd,pwsh,bash,fish,nu}`, `runner`, `error`). Shell support covers `cmd`, `pwsh`, `bash`/MSYS, `fish`, and `nu`, with auto-detection of the parent shell and a `--shell` override.

## Alternatives Considered

### Option A: Single-crate binary

- **Approach**: One Cargo crate, internal modules for discovery / capture / diff / shell / runner. Single `prepare-devenv.exe` artifact.
- **Pros**: Simplest to navigate, fewest moving parts, fastest to build, sufficient internal layering via Rust's module system.
- **Cons**: Discovery / capture logic isn't exposed for reuse from another Rust program without restructuring.
- **Why not adopted as runner-up**: This *is* the chosen approach (see Agreed Approach).

### Option B: Cargo workspace with `prepare-devenv-core` library + thin binary crate

- **Approach**: Split logic between a reusable `core` library crate and a `bin` crate that wires up `clap` + IO.
- **Pros**: Cleaner unit-test isolation, embeddable from another Rust program later.
- **Cons**: Roughly twice the `Cargo.toml` ceremony, extra cognitive overhead, no current consumer that needs the library form.
- **Why not adopted**: Premature factoring — YAGNI. We can carve out a `core` crate later when there's a real second consumer.

### Option C: Two-process design (helper `.bat` + thin `.exe`)

- **Approach**: Ship `prepare-devenv-helper.bat` alongside the `.exe`; the Rust binary shells out to the helper rather than driving `cmd.exe` directly.
- **Pros**: Less Rust code for the env-capture step.
- **Cons**: Two artifacts to distribute, harder to atomically version, batch script is brittle and harder to test.
- **Why not adopted**: Saves little code, costs significant deploy and testing complexity.

## Agreed Approach

**Option A — single-crate Rust 2024 binary**, with the pipeline detailed in §"Design Summary" and the contract surface defined in `design.md`.

Rationale: the surface is well-bounded (one CLI, one platform), the components compose linearly, and Rust modules already provide the internal isolation that a workspace would. Future reuse is supportable by promoting modules to a `core` crate without changing user-facing behavior.

## Key Decisions

| # | Decision | Notes |
|---|---|---|
| 1 | **Language**: Rust, edition 2024 | Single static `.exe`, strong typing for env-diff and shell formatting |
| 2 | **Platform**: Windows-only | No `cfg(unix)` matrix; the `windows` crate is unconditional |
| 3 | **Default no-`--` mode**: spawn parent shell as child with merged env | `--emit` opts in to eval-able stdout output instead |
| 4 | **Shell scope**: `cmd`, `pwsh`, `bash` (MSYS / Git Bash), `fish`, `nu` | First-class for both spawn and emit |
| 5 | **Shell selection**: auto-detect parent process, `--shell` overrides | Detection walks parent-process chain + checks `MSYSTEM` / `COMSPEC` / `$env:SHELL` |
| 6 | **`vswhere` strategy**: require at standard install path | `%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe`, fallback to `%PATH%`; clean error if missing |
| 7 | **Env-diff scope**: forward added + modified vars only | Removals ignored (rare from `VsDevCmd.bat`; safer default) |
| 8 | **POSIX `PATH` translation**: only `PATH` for `bash` target | Convert `C:\…` → `/c/…`, swap `;` → `:`. Leave `INCLUDE`/`LIB`/etc. in Windows form (consumed by Windows tools) |
| 9 | **Architecture**: single-crate binary with internal modules | `cli`, `discovery`, `capture`, `diff`, `shell/*`, `runner`, `error` |
| 10 | **Capture mechanism**: `cmd.exe /U /D /Q /C "call <bat> <args> 1>&2 && echo <MARKER> && set"`, parse UTF-16LE stdout after marker | `/U` forces UTF-16 stdout; marker prevents collision with bat output |
| 11 | **vsdevcmd output visibility**: suppressed by default, surfaced under `-v` | Redirected to our stderr inside the child cmd |
| 12 | **Verbose levels**: `-v` info, `-vv` debug | Implemented via `tracing` + `tracing-subscriber` |
| 13 | **Exit codes**: 0 ok / 1 generic / 2 bad-CLI / 3 discovery-failed / 4 vsdevcmd-failed / pass-through for `-- COMMAND` | Lets CI distinguish "tool broken" from "your VS install is wrong" |
| 14 | **Testability seams**: `discovery::resolve_with(fetch_json)` and `capture::capture_with(reader)` | Push IO to edges so the logic is unit-testable without VS installed |
| 15 | **Live tests**: behind a `live` Cargo feature | Manual / CI-on-self-hosted only |

## Open Questions

None blocking. Items the team should revisit during implementation but that have working defaults:

- **`set` line edge cases under `cmd /U`**: confirm whether multi-line values (rare) appear and how UTF-16 BOM is reported by `cmd.exe`. Working assumption: no multi-line values, BOM stripped before line splitting.
- **Single-quote escaping for `bash`/`fish`/`pwsh` `format_set`**: working scheme is to wrap values in single quotes and replace `'` with `'\''` (or shell-equivalent); confirm against fixtures during implementation.
- **`--devcmd-args` quoting**: forwarded as a single string to `cmd.exe`; confirm Windows command-line quoting rules don't mangle values like `-arch="x64 with spaces"`. Probably fine but worth a fixture.
- **Optional future extensions** (explicitly out of v1 scope): `--json` output, multiple-install enumeration command, an `--apply` mode that imports env into the calling shell via a wrapper script, support for non-Windows shells via WSL, packaging (Scoop manifest, GitHub Releases workflow).
