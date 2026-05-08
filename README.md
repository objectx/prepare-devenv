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

|     Option    |                Description                  |
|:-------------:|:--------------------------------------------|
| --help        | Show help                                   |
| -v, --verbose | Be verbose                                  |
| --id <id>     | Specify the Visual Studio instance ID       |
| --path <path> | Specify the Visual Studio installation path |
| --devcmd-args | Argument string for `vsdevcmd.bat`          |

## Examples
```bash
# Start current shell with detected environment (using the latest installed Visual Studio).
prepare-devenv
# Run supplied command with detected environment (using the latest installed Visual Studio).
prepare-devenv -- bash
# Run supplied command using the specified Visual Studio environment.
prepare-devenv --path "C:\Program Files\Microsoft Visual Studio\18" -- bash
# Same as above but using the instance ID.
prepare-devenv --id d0b481ec -- bash # With Visual Studio 2026 Insiders.
```

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
