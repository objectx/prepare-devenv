# prepare-devenv v1 Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development to implement this plan task-by-task. Each Task below corresponds to a top-level group in `tasks.md`; each Step is a discrete edit, file write, or command. Verify against the matching scenarios in `specs/<capability>/spec.md` after each Task.

**Goal:** Implement `prepare-devenv` v1 — a single Windows `.exe` (Rust 2024) that locates a Visual Studio install, runs its `VsDevCmd.bat`, captures the env delta, and bridges it into the user's shell or a child process.

**Architecture:** Single Cargo crate (`prepare-devenv`) with internal modules `cli`, `discovery`, `capture`, `diff`, `shell/{cmd,pwsh,bash,fish,nu}`, `runner`, `error`. IO is pushed to module edges via `_with`-suffixed entry points so the logic-heavy code is unit-testable without an installed VS. End-to-end tests use a `test_hooks` Cargo feature exposing a hidden `--devcmd-script` flag; live tests are gated behind a `live` feature.

**Tech Stack:** Rust 2024 edition · clap (derive) · serde + serde_json · thiserror · anyhow · tracing + tracing-subscriber · windows (Toolhelp + Threading features) · assert_cmd + predicates + tempfile (dev). Build/test on a single Windows GitHub Actions runner.

---

## Task 1: Project bootstrap

- [ ] **Step 1:** Run `cargo init --bin --edition 2024` at repo root; confirm `Cargo.toml` and `src/main.rs` were created and `.gitignore` contains `/target`.
- [ ] **Step 2:** Edit `Cargo.toml` to add the `[package]` block (`name = "prepare-devenv"`, `version = "0.1.0"`, `edition = "2024"`, `description`, `license` per repo policy).
- [ ] **Step 3:** Append `[dependencies]` block with `clap` (with `derive` feature), `serde` (with `derive`), `serde_json`, `thiserror`, `anyhow`, `tracing`, `tracing-subscriber` (with `env-filter` and `fmt` features), `windows` with selected features `["Win32_System_Diagnostics_ToolHelp", "Win32_System_Threading", "Win32_Foundation"]`.
- [ ] **Step 4:** Append `[dev-dependencies]` with `assert_cmd`, `predicates`, `tempfile`.
- [ ] **Step 5:** Append `[features]` block declaring `test_hooks = []` and `live = []` (both default off).
- [ ] **Step 6:** In `src/main.rs`, add a top-of-file `#[cfg(not(windows))] compile_error!("prepare-devenv targets Windows only");`.
- [ ] **Step 7:** Create empty stub files: `src/error.rs`, `src/cli.rs`, `src/discovery.rs`, `src/capture.rs`, `src/diff.rs`, `src/runner.rs`, `src/shell/mod.rs`, `src/shell/cmd.rs`, `src/shell/pwsh.rs`, `src/shell/bash.rs`, `src/shell/fish.rs`, `src/shell/nu.rs`. Each can hold a `// TODO: <module>` comment as the only content.
- [ ] **Step 8:** In `src/main.rs`, declare `mod cli; mod discovery; mod capture; mod diff; mod error; mod runner; mod shell;` so the build sees them.
- [ ] **Step 9:** Add `rustfmt.toml` with `edition = "2024"` and `clippy.toml` (empty stub for now — pedantic config tuned later).
- [ ] **Step 10:** Run `cargo build` to confirm a clean compile of the empty skeleton; commit with message `chore: scaffold prepare-devenv crate (Rust 2024)`.

## Task 2: Error type & result alias

- [ ] **Step 1:** In `src/error.rs`, define `pub enum Error` with `#[derive(thiserror::Error, Debug)]` and the variants listed in `tasks.md` 2.1 (`VsWhereMissing`, `VsWhereFailed`, `NoInstalls`, `NoMatch`, `AmbiguousId`, `VsDevCmdMissing`, `VsDevCmdFailed`, `EnvParse`, `UnknownShell`, `Spawn`, `Io`, `Json`).
- [ ] **Step 2:** Annotate `Io` and `Json` variants with `#[from]` so `?` propagation just works.
- [ ] **Step 3:** Annotate the `Spawn` variant with `#[source]` on its inner `io::Error`; verify `anyhow::Error::chain()` walks into it.
- [ ] **Step 4:** Add `pub type Result<T> = std::result::Result<T, Error>;` and re-export both from `error::*` via `pub use error::{Error, Result};` in `main.rs`.
- [ ] **Step 5:** Write a small `error_message_format` unit test asserting that `format!("{}", Error::VsWhereMissing)` matches the documented user-facing wording.
- [ ] **Step 6:** `cargo test`; commit with message `feat(error): introduce error::Error and Result alias`.

## Task 3: CLI module (clap-derive)

- [ ] **Step 1:** In `src/cli.rs`, `use clap::{Parser, ValueEnum};` and define `#[derive(Copy, Clone, Debug, ValueEnum)] pub enum ShellKind { Cmd, Pwsh, Bash, Fish, Nu }`.
- [ ] **Step 2:** Define `#[derive(Parser, Debug)] pub struct Cli` with the fields listed in `tasks.md` 3.1, applying `#[arg(long, conflicts_with = "path")]` on `id`, `#[arg(long, conflicts_with = "id")]` on `path`, `#[arg(long)]` on `devcmd_args`, `#[arg(long, conflicts_with = "command")]` on `emit`, `#[arg(long)]` on `shell`, `#[arg(short, long, action = clap::ArgAction::Count)]` on `verbose`, and `#[arg(last = true)] command: Vec<OsString>`.
- [ ] **Step 3:** Behind `#[cfg(feature = "test_hooks")]`, add a `#[arg(long, hide = true)] pub devcmd_script: Option<PathBuf>` that overrides the resolved `VsDevCmd.bat` path during tests.
- [ ] **Step 4:** Define `pub enum Mode { SpawnShell, Emit, RunCommand(Vec<OsString>) }` and implement `pub fn mode(&self) -> Mode` on `Cli`: `if !self.command.is_empty() { RunCommand(self.command.clone()) }` else if `self.emit { Emit }` else `SpawnShell`.
- [ ] **Step 5:** Write unit tests in `src/cli.rs` (under `#[cfg(test)] mod tests`) using `Cli::try_parse_from` for: no args → `SpawnShell`; `--emit` → `Emit`; `-- bash` → `RunCommand(["bash"])`; `--id X --path Y` → clap error; `--emit -- foo` → clap error; unknown `--shell` value → clap error.
- [ ] **Step 6:** `cargo test`; commit with message `feat(cli): clap-derive arg parsing and mode dispatch`.

## Task 4: Discovery module

- [ ] **Step 1:** In `src/discovery.rs`, define `pub struct VsInstance` with `#[derive(serde::Deserialize, Debug)]` and fields `installation_path`, `installation_version`, `instance_id`, `display_name` (with `#[serde(default)]`), `install_date` (with `#[serde(default)]`); use `#[serde(rename_all = "camelCase")]` on the struct so JSON keys map.
- [ ] **Step 2:** Implement `fn locate_vswhere() -> Result<PathBuf>`: read `%ProgramFiles(x86)%` env var (fall back to literal `C:\Program Files (x86)` if absent), join `Microsoft Visual Studio\Installer\vswhere.exe`, check existence; on miss, scan `%PATH%` directories for `vswhere.exe`; return `Error::VsWhereMissing` otherwise.
- [ ] **Step 3:** Implement `fn run_vswhere(vswhere: &Path) -> Result<Vec<VsInstance>>`: `Command::new(vswhere).args(["-format", "json", "-all", "-prerelease", "-products", "*"]).output()?`; on non-zero exit emit `Error::VsWhereFailed { code, stderr }`; on success deserialize stdout via `serde_json::from_slice` (note: vswhere emits UTF-8, no BOM concerns here).
- [ ] **Step 4:** Define `pub enum Selector<'a> { Latest, ById(&'a str), ByPath(&'a Path) }` and `pub struct ResolvedInstall { pub install_path, pub instance_id, pub display_name, pub version, pub vsdevcmd_path }`.
- [ ] **Step 5:** Implement `fn pick<'a>(installs: &'a [VsInstance], selector: Selector<'_>) -> Result<&'a VsInstance>` applying the precedence rule from `specs/vs-installation-discovery`: `ByPath` case-insensitive equality (use `Path::components` comparison or normalized lower-cased string compare); `ById` prefix match on `instance_id` with ambiguity check; `Latest` sorts by `installation_version` numerically (`semver::Version::parse` if available, else split-on-dot to numbers) descending then `install_date` descending.
- [ ] **Step 6:** Implement `pub fn resolve_with<F>(selector: Selector<'_>, fetch_json: F) -> Result<ResolvedInstall> where F: FnOnce() -> Result<Vec<VsInstance>>`: call `fetch_json()`, error if empty, call `pick`, derive `vsdevcmd_path = install_path.join("Common7").join("Tools").join("VsDevCmd.bat")`, verify existence (`Error::VsDevCmdMissing` otherwise), wrap into `ResolvedInstall`.
- [ ] **Step 7:** Implement `pub fn resolve(selector) -> Result<ResolvedInstall>` as `resolve_with(selector, || run_vswhere(&locate_vswhere()?))`.
- [ ] **Step 8:** Add fixture JSON files under `tests/fixtures/vswhere/`: `single.json`, `multiple_same_version.json`, `multiple_different_versions.json`, `ambiguous_prefix.json`, `empty.json`. Each is a hand-written `Vec<VsInstance>` JSON dump; commit them to git.
- [ ] **Step 9:** Add unit tests in `src/discovery.rs` driving `resolve_with` against the fixtures (load via `include_str!`) for every scenario in the spec — including the `latest by version` tie-breaking via `installDate`. Use `tempfile::TempDir` for the `VsDevCmd.bat`-existence check.
- [ ] **Step 10:** `cargo test`; commit with message `feat(discovery): vswhere lookup, JSON parse, install selection`.

## Task 5: Capture module

- [ ] **Step 1:** In `src/capture.rs`, implement `pub fn snapshot_pre_env() -> HashMap<OsString, OsString>` returning `std::env::vars_os().collect()`.
- [ ] **Step 2:** Implement `fn fresh_marker() -> String` using a small RNG (e.g., import `getrandom` once via the `windows` crate's `BCryptGenRandom`, or pull `rand` as a dep — avoid extra deps if possible) producing `__PREPARE_DEVENV_ENV_BEGIN_<16 hex>__`.
- [ ] **Step 3:** Implement `fn build_capture_command(bat: &Path, devcmd_args: Option<&str>, marker: &str) -> Command`: a `Command::new("cmd.exe")` with args `["/U", "/D", "/Q", "/C", &format!("call \"{}\" {} 1>&2 && echo {} && set", bat.display(), devcmd_args.unwrap_or(""), marker)]`. Handle inner double-quote rules carefully — write a unit test that constructs and inspects argv string.
- [ ] **Step 4:** Implement `fn decode_utf16le(bytes: &[u8]) -> String`: strip optional `[0xFF, 0xFE]` BOM, treat remainder as `&[u16]` (chunked), feed to `String::from_utf16_lossy`. Normalize `\r\n` → `\n` post-decode.
- [ ] **Step 5:** Implement `pub fn capture_with(reader: impl Read, marker: &str) -> Result<HashMap<OsString, OsString>>`: read full bytes, decode, find marker line; if missing return `Error::EnvParse("marker not found")`; for each subsequent non-empty line, `split_once('=')` (split on first `=` only); accumulate into the map. Reject lines without `=`.
- [ ] **Step 6:** Implement `pub fn capture(bat: &Path, devcmd_args: Option<&str>) -> Result<CapturedEnv>`: build command, spawn, wait, propagate `VsDevCmd.bat`'s exit code as `Error::VsDevCmdFailed(code)` when non-zero (note: the inner `cmd /C` rolls up to the same code on `&&` short-circuit), pass stdout into `capture_with`.
- [ ] **Step 7:** Build the UTF-16LE fixture bytes under `tests/fixtures/capture/`. Easiest is to generate them once via a small one-off Rust binary — write `examples/gen_capture_fixtures.rs` that emits the raw bytes for: `minimal_with_bom.bin`, `minimal_no_bom.bin`, `value_with_equals.bin`, `multibyte_non_ascii.bin`, `missing_marker.bin`. Run it once via `cargo run --example gen_capture_fixtures -- tests/fixtures/capture/`, check the resulting binaries into git, then commit the example as well.
- [ ] **Step 8:** Add unit tests in `src/capture.rs` driving `capture_with` against the fixtures (loaded via `include_bytes!`).
- [ ] **Step 9:** `cargo test`; commit with message `feat(capture): cmd /U capture pipeline + UTF-16LE parser`.

## Task 6: Diff module

- [ ] **Step 1:** In `src/diff.rs`, define `pub struct EnvDiff { pub added: BTreeMap<OsString, OsString>, pub modified: BTreeMap<OsString, OsString> }`.
- [ ] **Step 2:** Implement helper `fn key_eq_ci(a: &OsStr, b: &OsStr) -> bool` doing case-insensitive ASCII comparison via `to_string_lossy().eq_ignore_ascii_case(...)` (Windows env names are ASCII in practice; document the assumption).
- [ ] **Step 3:** Build `pre_lc: HashMap<String, &OsStr>` keyed by lower-cased name → original-cased key, used for the lookup; this preserves casing for output but allows case-insensitive comparison.
- [ ] **Step 4:** Implement `pub fn diff(pre: &HashMap<OsString, OsString>, post: &HashMap<OsString, OsString>) -> EnvDiff`: for each `(k, v)` in `post`, look up `k` (case-insensitively) in `pre`; if absent → `added`; if present and value differs byte-for-byte → `modified`; else skip.
- [ ] **Step 5:** Implement `EnvDiff::iter()` yielding `added.iter().chain(modified.iter())` (both `BTreeMap` so already key-sorted).
- [ ] **Step 6:** Implement `EnvDiff::merged_env(&self, base: &HashMap<OsString, OsString>) -> HashMap<OsString, OsString>` for the spawn paths: `let mut out = base.clone(); for (k, v) in self.iter() { out.insert(k.clone(), v.clone()); } out`.
- [ ] **Step 7:** Add unit tests in `src/diff.rs`: added-only, modified-only, unchanged-skipped, removed-ignored, case-insensitive name match (e.g. `Path` vs `PATH`).
- [ ] **Step 8:** `cargo test`; commit with message `feat(diff): added/modified env delta with CI key matching`.

## Task 7: Shell trait & adapters

- [ ] **Step 1:** In `src/shell/mod.rs`, define `pub use crate::cli::ShellKind;` and `pub trait Shell { fn kind(&self) -> ShellKind; fn executable(&self) -> &OsStr; fn format_set(&self, name: &OsStr, value: &OsStr) -> Result<String>; fn translate_path(&self, _name: &OsStr, value: &OsStr) -> OsString { value.to_owned() } }`.
- [ ] **Step 2:** Implement `pub fn for_kind(kind: ShellKind) -> Box<dyn Shell>` factory.
- [ ] **Step 3:** Implement `src/shell/cmd.rs`: `pub struct Cmd; impl Shell for Cmd`, `kind() = ShellKind::Cmd`, `executable() = OsStr::new("cmd.exe")`, `format_set` returns `Err(Error::EnvParse("cmd cannot represent embedded \""))` if value contains `"`, else `Ok(format!("set \"{name}={value}\""))`.
- [ ] **Step 4:** Implement `src/shell/pwsh.rs` per spec — `format_set` doubles single quotes (`'` → `''`) inside `'...'`. `executable` returns `OsStr::new("pwsh.exe")` (fallback to `powershell.exe` is the runner's job).
- [ ] **Step 5:** Implement `src/shell/bash.rs` `format_set` per spec — wrap in single quotes, replace `'` with `'\''`. Add a private `fn translate_path_value(value: &OsStr) -> OsString` doing the split-on-`;`, drive-letter conversion, UNC passthrough, join-on-`:` algorithm. Override `translate_path` to call it when `name.eq_ignore_ascii_case(OsStr::new("PATH"))`.
- [ ] **Step 6:** Implement `src/shell/fish.rs`: `format_set` emits `set -gx NAME 'VALUE'` with `'` → `\'`. Verify against fish's docs in `Context7` if any escape edge cases apply.
- [ ] **Step 7:** Implement `src/shell/nu.rs`: `format_set` emits `$env.NAME = 'VALUE'` with `'` → `\''`. Verify against nu's docs in `Context7`.
- [ ] **Step 8:** Implement `pub fn detect() -> ShellKind` in `src/shell/mod.rs`: walk parent-process chain via `windows::Win32::System::Diagnostics::ToolHelp::CreateToolhelp32Snapshot` + `Process32FirstW`/`Process32NextW`, match lower-cased `szExeFile` against the supported names; if no match, consult `MSYSTEM`/`COMSPEC`/`SHELL` env vars; ultimate fallback `ShellKind::Cmd` with `tracing::warn!("falling back to cmd; could not detect parent shell")`.
- [ ] **Step 9:** Add `#[cfg(test)] pub fn detect_with(parents: &[String], env: &EnvHints) -> ShellKind` exposing the inner logic; the public `detect()` calls this with real OS data.
- [ ] **Step 10:** Unit tests for every adapter's `format_set` (per scenarios in `specs/shell-adapter-suite`); unit tests for `bash::translate_path_value` (mixed slashes, drive-letter casing, UNC, empty, non-PATH passthrough).
- [ ] **Step 11:** Unit tests for `detect_with` covering the full table from the spec.
- [ ] **Step 12:** `cargo test`; commit with message `feat(shell): trait, five adapters, parent-shell detection`.

## Task 8: Runner module

- [ ] **Step 1:** In `src/runner.rs`, implement `pub fn emit(diff: &EnvDiff, shell: &dyn Shell, out: &mut dyn Write) -> Result<()>`: iterate `diff.iter()`, for each `(name, value)` compute `value' = shell.translate_path(name, value)`, call `shell.format_set(name, &value')?`, write `line + "\n"`.
- [ ] **Step 2:** Implement `pub fn spawn_shell(diff: &EnvDiff, base: &HashMap<OsString, OsString>, shell: &dyn Shell) -> Result<i32>`: build merged env (apply `translate_path` per var), call `Command::new(shell.executable()).envs(...).status()` with default inherited stdio; return `status.code().unwrap_or(1)`. Wrap spawn errors as `Error::Spawn { cmd: shell.executable().to_string_lossy().into(), source: e }`.
- [ ] **Step 3:** Add a pwsh-fallback helper inside the pwsh-spawn path: try `pwsh.exe` first, on `ErrorKind::NotFound` retry with `powershell.exe`. Test both paths via `which`-style checks.
- [ ] **Step 4:** Implement `pub fn spawn_command(diff: &EnvDiff, base: &HashMap<OsString, OsString>, argv: &[OsString]) -> Result<i32>`: similar to `spawn_shell` but no `translate_path` and `Command::new(&argv[0]).args(&argv[1..])`.
- [ ] **Step 5:** Wire all three `runner::*` paths to use `tracing::info!`/`debug!` for the chosen executable, env-var counts, and exit code.
- [ ] **Step 6:** Unit-level test for `runner::emit` using `Vec<u8>` as the writer and a hand-built `EnvDiff`; assert byte-for-byte output for each shell adapter.
- [ ] **Step 7:** `cargo test`; commit with message `feat(runner): emit / spawn_shell / spawn_command output paths`.

## Task 9: main.rs orchestration

- [ ] **Step 1:** Replace `src/main.rs` body with: `fn main() { match run() { Ok(code) => std::process::exit(code), Err(e) => { eprintln!("{:?}", e); std::process::exit(error_to_code(&e)); } } }`.
- [ ] **Step 2:** Implement `fn run() -> Result<i32>`: parse CLI via `Cli::parse()`; call `init_tracing(cli.verbose)`; call `discovery::resolve(...)`; call `capture::snapshot_pre_env()` then `capture::capture(...)`; call `diff::diff(...)`; dispatch by `Mode`.
- [ ] **Step 3:** Implement `fn init_tracing(verbose: u8)`: `EnvFilter::new` with `error` (verbose=0), `info` (1), `debug` (2+); `tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).with_ansi(false).compact().init()`.
- [ ] **Step 4:** Implement `fn error_to_code(e: &Error) -> i32` per the documented exit-code map: `VsWhereMissing | VsWhereFailed | NoInstalls | NoMatch | AmbiguousId | VsDevCmdMissing` → 3; `VsDevCmdFailed(_)` → 4; everything else → 1. Spawn-command pass-through is handled by returning the child's code from `runner::spawn_command`, not by this function.
- [ ] **Step 5:** Wire the `test_hooks`-feature `--devcmd-script` override: when present and the feature is enabled, skip `discovery::resolve` and use the supplied path directly as `vsdevcmd_path`.
- [ ] **Step 6:** Manual smoke: `cargo run -- --help` should print clap help with the documented flags.
- [ ] **Step 7:** Commit with message `feat(main): orchestration, tracing init, exit-code mapping`.

## Task 10: End-to-end CLI tests (test_hooks feature)

- [ ] **Step 1:** Create `tests/fixtures/devcmd-script/fake.bat` whose body emits a known set of vars (e.g., `set INCLUDE=C:\fake\include`, `set LIB=C:\fake\lib`, `set PATH=C:\fake\bin;%PATH%`, then `exit /b 0`).
- [ ] **Step 2:** Create per-shell expected-output fixtures under `tests/fixtures/emit/<shell>.txt` containing the exact stdout that `runner::emit` should produce for the fake-bat scenario, sorted with `added` before `modified`.
- [ ] **Step 3:** Write `tests/cli_emit.rs` parameterized over the five shells: build the binary with `--features test_hooks`, run via `assert_cmd::Command::cargo_bin("prepare-devenv")`, set env to match a known baseline, pass `--emit --shell <kind> --devcmd-script tests/fixtures/devcmd-script/fake.bat`, assert stdout equals the fixture.
- [ ] **Step 4:** Write `tests/cli_spawn_command.rs`: invoke `prepare-devenv --devcmd-script <fake.bat> -- cmd /c set`; assert the spawned `cmd /c set` stdout contains `INCLUDE=C:\fake\include`.
- [ ] **Step 5:** Write `tests/cli_exit_codes.rs` covering: `--id <bogus>` → 3; `--devcmd-script <missing-file>` → 3; a fake bat that does `exit /b 7` → 4; `--id X --path Y` → 2; `-- cmd /c exit 17` → 17; happy path with `--emit` → 0.
- [ ] **Step 6:** Write `tests/cli_stdout_clean.rs`: assert stdout is empty for `prepare-devenv -- cmd /c rem` (with and without `-v`). Use `assert_cmd`'s `stdout(predicates::str::is_empty())`.
- [ ] **Step 7:** `cargo test --features test_hooks`; commit with message `test: end-to-end CLI suite via fake VsDevCmd.bat`.

## Task 11: Live tests (live feature)

- [ ] **Step 1:** Add `tests/live_discover.rs` with `#[cfg(feature = "live")] #[test] fn resolve_latest_install_exists()`: call `discovery::resolve(Selector::Latest)?`, assert `result.install_path.exists()` and `result.vsdevcmd_path.exists()`.
- [ ] **Step 2:** Add `tests/live_capture.rs` with `#[cfg(feature = "live")] #[test] fn capture_includes_msvc_vars()`: resolve latest, call `capture::capture`, diff, assert `INCLUDE`, `LIB`, `VCINSTALLDIR` keys are present in the diff (case-insensitive lookup).
- [ ] **Step 3:** Update `README.md` "Building from source" section adding: "Live tests require an installed Visual Studio: `cargo test --features live`."
- [ ] **Step 4:** Manual run: `cargo test --features live` on the dev machine; commit with message `test: live VS-installed integration tests behind 'live' feature`.

## Task 12: CI workflow

- [ ] **Step 1:** Create `.github/workflows/ci.yml` with `name: CI`, `on: [push, pull_request]`, single job `build-and-test` on `runs-on: windows-latest`.
- [ ] **Step 2:** Steps: `actions/checkout@v4`; `dtolnay/rust-toolchain@stable` (or `@1.85` once Rust 2024 is in stable) with `components: rustfmt, clippy`; `Swatinem/rust-cache@v2`; `cargo fmt -- --check`; `cargo clippy --all-targets --features test_hooks -- -D warnings`; `cargo test --features test_hooks`.
- [ ] **Step 3:** Create `.github/workflows/live.yml` with `on: workflow_dispatch`, a single job on `runs-on: [self-hosted, windows, vs-installed]` running `cargo test --features live`. Document the runner labels in the workflow file's `name` description.
- [ ] **Step 4:** Create `.github/workflows/release.yml` with `on: workflow_dispatch`, a single job on `windows-latest` running `cargo build --release` and `actions/upload-artifact@v4` for `target/release/prepare-devenv.exe`.
- [ ] **Step 5:** Push the branch to a GitHub remote (or fork) once and confirm CI passes; commit any tweaks. Commit message: `ci: windows runner with fmt/clippy/test, manual live + release dispatch`.

## Task 13: Documentation polish

- [ ] **Step 1:** Update `README.md`: add a "Building from source" section (`cargo build --release` on Windows, requires Rust 2024+); confirm every CLI example matches the implemented flags; cross-reference `--shell` and `--emit`.
- [ ] **Step 2:** Update root `CLAUDE.md` "Project status" section: replace "Greenfield..." paragraph with a one-paragraph description of the implemented layout (modules, features, build/test commands).
- [ ] **Step 3:** Replace `## Lessons / corrections` placeholder reference with a working `tasks/lessons.md` if any lessons have accumulated during implementation.
- [ ] **Step 4:** Add a `docs/SMOKE.md` (or expand `README.md`) with the manual smoke checklist from `tasks.md` 13.3: `where cl.exe` test, `--emit | iex` from pwsh, no-args run from each supported shell, `--id <bogus>` discovery error, `--devcmd-args="-arch=arm64"` cross-arch sanity check.
- [ ] **Step 5:** Run `cargo doc --no-deps` and skim the output; tighten any module-level doc comments that look weak.
- [ ] **Step 6:** Final commit: `docs: README + CLAUDE.md update for v1, smoke checklist`.

---

## Verification

After each Task: run `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, and the relevant `cargo test [--features ...]` invocation. After Task 13: run the manual smoke checklist on a real machine with VS installed; cross-check every scenario in `specs/<capability>/spec.md` against an implemented test (unit, integration, or live) and confirm none are uncovered.

## Implementation tips

- **One commit per Task** keeps history reviewable and lets `superpowers:subagent-driven-development` parallelize independent Tasks (e.g., Task 4 and Task 5 share no files; Tasks 7's adapter sub-files can be parallelized).
- **Tasks 4–8 are pure logic and unit-testable without VS.** Save the live tests (Task 11) and CI runner setup (Task 12) for after the unit + integration suite is green.
- **`cmd.exe` quoting is finicky.** When constructing the inner command string in Task 5, write the test from Step 3 *before* the implementation — it pins the exact argv shape and prevents quote-mangling regressions.
- **The `windows` crate's parent-process walk** (Task 7 Step 8) is the only place that's hard to unit-test cross-platform; the `detect_with` seam in Step 9 is what makes the rest testable.
