# prepare-devenv - Prepares Visual Studio environment

`prepare-devenv` do following:
 1. searches the specified version of `vsdevcmd.bat`
 2. Run the found `vsdevcmd.bat` with supplied options.
 3. Capture the environment variable changes.
 4. Run specified command (after `--` option) under changed environment variable injected.

 # Usage

```bash
prepare-devenv [options] [-- command args...]
```

|       Option       |                Description                                          |
|:------------------:|:--------------------------------------------------------------------|
| --help             | Show help                                                            |
| -v, --verbose      | Be verbose (`-v` info, `-vv` debug; logs go to stderr)               |
| --id <id>          | Specify the Visual Studio instance ID (prefix match)                 |
| --path <path>      | Specify the Visual Studio installation path                          |
| --devcmd-args      | Argument string forwarded verbatim to `VsDevCmd.bat`                 |
| --emit             | Print shell-specific `set` lines on stdout instead of spawning       |
| --shell <kind>     | Override auto-detected target shell (`cmd`, `pwsh`, `bash`, `fish`, `nu`) |

## Examples
```bash
# Start current shell with detected environment (using the latest installed STABLE Visual Studio).
prepare-devenv
# Run supplied command with detected environment (using the latest installed STABLE Visual Studio).
prepare-devenv -- bash
# Run supplied command using the specified Visual Studio environment.
prepare-devenv --path "C:\Program Files\Microsoft Visual Studio\18" -- bash
# Same as above but using the instance ID — also how you opt into an Insiders / Preview install,
# which the default selector deliberately skips.
prepare-devenv --id d0b481ec -- bash # With Visual Studio 2026 Insiders.
```

### Install selection

With no `--id` / `--path`, `prepare-devenv` picks the **latest stable**
Visual Studio install on the host (numeric `installationVersion` compare,
`installDate` descending as a tie-break). Insiders / Preview installs
(those marked `isPrerelease: true` by `vswhere`) are excluded from this
auto-selection regardless of how new their version number looks — the
intent is "the VS you'd reach for by default", not "the highest version
on disk". Use `--id <prefix>` or `--path <dir>` to explicitly target a
prerelease. If the host has *only* prerelease installs, the selector
falls back to the highest prerelease and emits a `-v` warning.

### `--emit` mode

When you want the VS environment merged into the *current* shell (rather
than a child process), use `--emit` to print eval-able `set` lines and
pipe them into the shell. From PowerShell:

```pwsh
prepare-devenv --emit | iex
```

`--emit` auto-detects the surrounding shell; pass `--shell <kind>` to
override (e.g. when piping the output to a script for a different shell).

### `--shell` override

`prepare-devenv` walks the parent-process chain to detect the calling
shell. Pass `--shell cmd|pwsh|bash|fish|nu` to override the detection —
useful when piping output through an intermediate process whose ancestry
hides the real target shell.

### Verbose output

Diagnostics are written to stderr so they don't pollute the eval-able
stdout of `--emit`. `-v` enables `info` logs (e.g. detected shell,
resolved VS install, env counts); `-vv` adds `debug` (e.g. merged-env
sizes, capture marker values).

## Building from source

`prepare-devenv` is a Rust 2024 Cargo crate. From the repository root:

```pwsh
cargo build --release
# -> target\release\prepare-devenv.exe
```

Run the unit + fixture-driven test suite (works without a Visual Studio install):

```pwsh
cargo test
cargo test --features test_hooks    # also covers the end-to-end CLI suite
```

Run the live tests (require a working Visual Studio install with `vswhere.exe`
on the standard installer path):

```pwsh
cargo test --features live
```
