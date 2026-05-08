## 1. Project bootstrap

- [x] 1.1 Run `cargo init --bin --edition 2024` at repo root; commit the resulting `Cargo.toml`, `src/main.rs` (placeholder), and `.gitignore` updates
- [x] 1.2 Add runtime deps to `Cargo.toml`: `clap` (with `derive`), `serde` (with `derive`), `serde_json`, `thiserror`, `anyhow`, `tracing`, `tracing-subscriber` (with `env-filter` + `fmt`), `windows` (selective features for `Win32_System_Diagnostics_ToolHelp` + `Win32_System_Threading`)
- [x] 1.3 Add dev-deps: `assert_cmd`, `predicates`, `tempfile`
- [x] 1.4 Declare Cargo features in `Cargo.toml`: `test_hooks` (default off, gates the hidden `--devcmd-script` flag), `live` (default off, gates VS-installed integration tests)
- [x] 1.5 Add a `[target.'cfg(not(windows))']` build error so non-Windows targets fail at compile time with a clear message
- [x] 1.6 Create empty module files: `src/error.rs`, `src/cli.rs`, `src/discovery.rs`, `src/capture.rs`, `src/diff.rs`, `src/runner.rs`, `src/shell/mod.rs`, `src/shell/{cmd,pwsh,bash,fish,nu}.rs`; wire them into `main.rs` with `mod` declarations
- [x] 1.7 Add `rustfmt.toml` and `clippy.toml` with project conventions (set `clippy.toml` to fail on `pedantic` lints we care about; keep `rustfmt.toml` minimal — defaults plus `edition = "2024"`)

## 2. Error type & result alias

- [ ] 2.1 Define `enum Error` in `src/error.rs` using `#[derive(thiserror::Error, Debug)]` covering: `VsWhereMissing`, `VsWhereFailed { code, stderr }`, `NoInstalls`, `NoMatch { kind, value }`, `AmbiguousId(String)`, `VsDevCmdMissing(PathBuf)`, `VsDevCmdFailed(i32)`, `EnvParse(&'static str)`, `UnknownShell(String)`, `Spawn { cmd, source }`, `Io(io::Error)`, `Json(serde_json::Error)`
- [ ] 2.2 Add a `pub type Result<T> = std::result::Result<T, Error>` alias
- [ ] 2.3 Implement `From<io::Error>` and `From<serde_json::Error>` via `#[from]` on the variants

## 3. CLI module (clap-derive)

- [ ] 3.1 Define `struct Cli` with fields per `specs/cli-surface-and-diagnostics`: `id`, `path` (mutually exclusive), `devcmd_args`, `emit`, `shell` (clap `ValueEnum`), `verbose` (counted), `command` (`last = true` collected after `--`)
- [ ] 3.2 Define `enum ShellKind { Cmd, Pwsh, Bash, Fish, Nu }` deriving `clap::ValueEnum`
- [ ] 3.3 Define `enum Mode { SpawnShell, Emit, RunCommand(Vec<OsString>) }` and `impl Cli { fn mode(&self) -> Mode }` enforcing the `--emit` ⊕ `-- COMMAND` rule via clap's `conflicts_with` plus a manual fallthrough check
- [ ] 3.4 Add the hidden `--devcmd-script <path>` flag gated behind `#[cfg(feature = "test_hooks")]` for end-to-end tests
- [ ] 3.5 Unit tests for `Cli::mode()` and clap conflict-error exit codes (id+path, emit+command, unknown shell)

## 4. Discovery module

- [ ] 4.1 Implement `fn locate_vswhere() -> Result<PathBuf>`: probe `%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe`, then fall back to a `%PATH%` search (`which`-style); return `Error::VsWhereMissing` with both probed locations on failure
- [ ] 4.2 Define `struct VsInstance` with `serde::Deserialize`: required fields `installationPath`, `installationVersion`, `instanceId`; `#[serde(default)]` for `displayName` and `installDate`
- [ ] 4.3 Implement `fn run_vswhere(vswhere: &Path) -> Result<Vec<VsInstance>>`: spawn `vswhere -format json -all -prerelease -products *`, capture stdout, deserialize; surface non-zero exit as `Error::VsWhereFailed` with stderr
- [ ] 4.4 Implement `fn resolve_with(selector, fetch_json: F) -> Result<ResolvedInstall>` where `F: FnOnce() -> Result<Vec<VsInstance>>`; apply the precedence rule from the spec (`--path` exact case-insensitive → `--id` prefix → latest by version with `installDate` desc tie-break)
- [ ] 4.5 Implement `pub fn resolve(selector) -> Result<ResolvedInstall>` as a one-line delegate over `resolve_with` + `run_vswhere`
- [ ] 4.6 Compute `vsdevcmd_path = install_path.join("Common7").join("Tools").join("VsDevCmd.bat")`; verify existence and surface `Error::VsDevCmdMissing` when absent
- [ ] 4.7 Add fixture JSON files under `tests/fixtures/vswhere/` for: single install, multiple installs same version, multiple installs different versions, ambiguous-prefix, empty array
- [ ] 4.8 Unit tests against `resolve_with` covering all scenarios in `specs/vs-installation-discovery`

## 5. Capture module

- [ ] 5.1 Implement `fn snapshot_pre_env() -> HashMap<OsString, OsString>` using `std::env::vars_os()`; document Windows case-insensitive lookup behavior
- [ ] 5.2 Implement `fn build_capture_command(bat: &Path, devcmd_args: Option<&str>, marker: &str) -> Command` returning a `std::process::Command` for `cmd.exe /U /D /Q /C` with the constructed `call ... && echo MARKER && set` string
- [ ] 5.3 Implement marker generation: `fn fresh_marker() -> String` returning `__PREPARE_DEVENV_ENV_BEGIN_<random hex>__`
- [ ] 5.4 Implement `fn capture_with(reader: impl Read, marker: &str) -> Result<HashMap<OsString, OsString>>` doing UTF-16LE decoding (strip BOM if present), normalize CRLF → LF, split on marker, parse `KEY=VALUE` lines (split on first `=` only); return `Error::EnvParse` if marker absent
- [ ] 5.5 Implement `pub fn capture(bat: &Path, devcmd_args: Option<&str>) -> Result<CapturedEnv>` wiring the pieces, propagating `VsDevCmd.bat`'s exit code as `Error::VsDevCmdFailed`
- [ ] 5.6 Add committed UTF-16LE fixture bytes under `tests/fixtures/capture/`: minimal-with-bom, minimal-without-bom, value-containing-equals, multibyte-non-ascii, missing-marker
- [ ] 5.7 Unit tests against `capture_with` covering all scenarios in `specs/devenv-environment-capture`

## 6. Diff module

- [ ] 6.1 Define `struct EnvDiff { added: BTreeMap<OsString, OsString>, modified: BTreeMap<OsString, OsString> }` (ordered for stable output)
- [ ] 6.2 Implement `pub fn diff(pre: &HashMap<OsString, OsString>, post: &HashMap<OsString, OsString>) -> EnvDiff` using case-insensitive name comparison (Windows convention) but preserving original casing of post-keys
- [ ] 6.3 Implement `EnvDiff::merged_env(&self, base: &HashMap<OsString, OsString>) -> HashMap<OsString, OsString>` for the spawn paths
- [ ] 6.4 Implement `EnvDiff::iter()` yielding added then modified, each in sorted-key order
- [ ] 6.5 Unit tests covering added / modified / unchanged / removed-ignored / case-insensitive name match

## 7. Shell trait & adapters

- [ ] 7.1 Define `trait Shell` in `src/shell/mod.rs` with `kind`, `executable`, `format_set(name, value) -> Result<String>`, default `translate_path(name, value) -> OsString`
- [ ] 7.2 Implement `shell::for_kind(ShellKind) -> Box<dyn Shell>` factory
- [ ] 7.3 `shell::cmd::Cmd`: `format_set` emits `set "NAME=VALUE"`, returns `Error::EnvParse`-class error if value contains literal `"`; `executable` returns `cmd.exe`
- [ ] 7.4 `shell::pwsh::Pwsh`: `format_set` emits `$env:NAME = 'VALUE'` with `'` doubled; `executable` returns `pwsh.exe` (fallback to `powershell.exe` if `pwsh` absent — handled at spawn time, not in trait)
- [ ] 7.5 `shell::bash::Bash`: `format_set` emits `export NAME='VALUE'` with `'` → `'\''`; `executable` returns `bash.exe`; override `translate_path` to apply POSIX path translation when `name == "PATH"`
- [ ] 7.6 `shell::bash::translate_path_value(value) -> OsString`: split on `;`, convert `[A-Za-z]:[\\/]…` → `/<lower>/…` with `\` → `/`, leave UNC and non-drive segments verbatim, join on `:`
- [ ] 7.7 `shell::fish::Fish`: `format_set` emits `set -gx NAME 'VALUE'` with `'` → `\'`; `executable` returns `fish.exe`
- [ ] 7.8 `shell::nu::Nu`: `format_set` emits `$env.NAME = 'VALUE'` with `'` → `\''` (per nu's escaping rules — verify with nu docs during implementation); `executable` returns `nu.exe`
- [ ] 7.9 Implement `shell::detect() -> ShellKind`: walk parent-process chain via `CreateToolhelp32Snapshot` + `Process32First/Next`, match on lowercased `szExeFile`; fall back to `MSYSTEM`/`COMSPEC`/`$env:SHELL`; final fallback `ShellKind::Cmd` with `tracing::warn!`
- [ ] 7.10 Add a test-only seam (`#[cfg(test)] fn detect_with(parents: &[String]) -> ShellKind`) so tests can drive detection without OS calls
- [ ] 7.11 Unit tests for every `format_set` adapter covering simple value, embedded single quote, embedded double quote, value with `;`, value with `%`, value with newline (rejected if shell can't represent), and the `cmd` double-quote rejection
- [ ] 7.12 Unit tests for `bash::translate_path_value` covering mixed slashes, drive-letter casing, UNC, empty, and non-PATH passthrough

## 8. Runner module

- [ ] 8.1 Implement `pub fn emit(diff: &EnvDiff, shell: &dyn Shell, out: &mut dyn Write) -> Result<()>`: iterate `diff.iter()`, apply `translate_path`, format via `format_set`, write each line + `\n` to `out`
- [ ] 8.2 Implement `pub fn spawn_shell(diff: &EnvDiff, base: &HashMap<OsString, OsString>, shell: &dyn Shell) -> Result<i32>`: build merged env (apply `translate_path` per var), spawn `shell.executable()` with `Stdio::inherit()` for stdin/stdout/stderr, wait, return exit code; surface `Error::Spawn` on failure-to-start
- [ ] 8.3 Implement `pub fn spawn_command(diff: &EnvDiff, base: &HashMap<OsString, OsString>, argv: &[OsString]) -> Result<i32>`: build merged env without per-shell `translate_path`, spawn `argv[0]` with the rest of `argv` and `Stdio::inherit()`, wait, return exit code
- [ ] 8.4 Decide pwsh fallback in `spawn_shell` when `pwsh.exe` is absent on PATH (try `powershell.exe`); document and test
- [ ] 8.5 Integration test (under `tests/`) using the `test_hooks` feature + a fake `.bat` to drive `runner::emit` end-to-end for each shell and diff stdout against a committed expected-output fixture
- [ ] 8.6 Integration test for `runner::spawn_command` using the `test_hooks` feature to launch `cmd /c set` and assert the spawned process sees the diffed vars

## 9. main.rs orchestration

- [ ] 9.1 Initialize `tracing-subscriber` from `Cli::verbose` (off / `info` / `debug`) writing to stderr; configure compact format, no ANSI by default
- [ ] 9.2 Pipeline call sequence: parse CLI → resolve install → snapshot pre-env → capture → diff → dispatch by `Mode`
- [ ] 9.3 `Mode::Emit`: call `runner::emit` against `io::stdout().lock()` with the chosen shell adapter
- [ ] 9.4 `Mode::SpawnShell`: call `runner::spawn_shell`; propagate exit code
- [ ] 9.5 `Mode::RunCommand(argv)`: call `runner::spawn_command`; propagate exit code
- [ ] 9.6 Top-level error handling: convert `Error` → exit code per the documented map (3 for discovery, 4 for `VsDevCmdFailed`, 1 for spawn / IO / EnvParse, 0 for success); print `anyhow`-formatted chain to stderr
- [ ] 9.7 Verify stdout is untouched by anything other than `runner::emit` via an integration test that checks `assert_cmd`'s `stdout()` is empty for every non-emit invocation

## 10. End-to-end CLI tests (test_hooks feature)

- [ ] 10.1 Add `tests/cli_emit.rs` running the binary via `assert_cmd` with `--devcmd-script <fake.bat>` for each of `cmd`, `pwsh`, `bash`, `fish`, `nu`; assert stdout matches the per-shell expected fixture
- [ ] 10.2 Add `tests/cli_spawn_command.rs` invoking `prepare-devenv -- cmd /c set` against the fake bat, asserting the dumped env contains the diffed vars
- [ ] 10.3 Add `tests/cli_exit_codes.rs` covering the 0/1/2/3/4/N matrix
- [ ] 10.4 Add `tests/cli_stdout_clean.rs` asserting stdout is empty for non-emit modes (with and without `-v`)
- [ ] 10.5 Add `tests/fixtures/devcmd-script/fake.bat` that emits a known set of env vars, plus per-shell expected-output fixtures under `tests/fixtures/emit/<shell>.txt`

## 11. Live tests (live feature)

- [ ] 11.1 Add `tests/live_discover.rs` calling `discovery::resolve(Selector::Latest)` and asserting the returned path exists; gated by `#[cfg(feature = "live")]`
- [ ] 11.2 Add `tests/live_capture.rs` calling `capture::capture` against the resolved install and asserting `INCLUDE`, `LIB`, `VCINSTALLDIR`, `WindowsSdkDir` are present in the diff; gated by `#[cfg(feature = "live")]`
- [ ] 11.3 Document in repo `README.md` how to run live tests (`cargo test --features live`) and that they require an installed VS

## 12. CI workflow

- [ ] 12.1 Create `.github/workflows/ci.yml` with a single job on `windows-latest` running: `cargo fmt -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --features test_hooks`
- [ ] 12.2 Cache `~/.cargo/registry`, `~/.cargo/git`, `target/` with the standard rust-cache action keyed on `Cargo.lock`
- [ ] 12.3 Add a separate, manually-triggered workflow (`workflow_dispatch`) for the `live` job intended to run on a self-hosted Windows-with-VS runner; document the runner-label requirement
- [ ] 12.4 Add a `release` job (also `workflow_dispatch`) that builds a release binary with `cargo build --release` and uploads it as a workflow artifact (full Releases / signing wiring is out of v1 scope)

## 13. Documentation polish

- [ ] 13.1 Update `README.md` with a short "Building from source" section (`cargo build --release`) and confirm the existing usage examples match the implemented CLI
- [ ] 13.2 Update root `CLAUDE.md` "Project status" to reflect that v1 is implemented, replacing the greenfield-language-pick instruction; add the actual build/test/lint commands
- [ ] 13.3 Smoke-checklist (`README.md` or `docs/SMOKE.md`): `prepare-devenv -- where cl.exe`, `prepare-devenv --emit | iex` from pwsh, `prepare-devenv` from each supported shell, `prepare-devenv --id <bogus>` exits 3 with candidates listed at `-v`, `prepare-devenv --devcmd-args="-arch=arm64"` succeeds with `cl.exe -?` reporting arm64
