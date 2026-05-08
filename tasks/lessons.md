# Lessons learned

Captured during the prepare-devenv v1 implementation cycle. Each entry
names a non-obvious failure mode and the fix, so future development
in this codebase doesn't relearn the same gotcha.

## `Command::arg` mangles inner cmd.exe scripts

Rust's `std::process::Command::arg` on Windows escapes embedded `"` as
`\"`. cmd.exe does NOT understand `\"` inside the inner script of
`cmd /C "..."` — it parses the resulting CommandLine as a broken path.

**Use** `std::os::windows::process::CommandExt::raw_arg(format!("\"{inner}\""))`
for the inner-script arg of `cmd /C`. The flag args (`/U /D /Q /C`)
themselves contain no quotes/spaces, so plain `.arg()` is fine for them.

Surfaced by: Task 10 e2e test suite (`tests/cli_emit.rs` etc.) — every
real spawn of `VsDevCmd.bat` failed with `VsDevCmdFailed(1)` until the
fix landed.

## `echo X && set` emits the marker with a trailing space

cmd.exe tokenizes `echo MARKER && set` as `echo` + arglist `["MARKER",
"", "&&", ...]` — the literal whitespace between `echo MARKER` and
`&&` becomes part of `echo`'s output. So the captured stdout has
`MARKER \r\n`, NOT `MARKER\r\n`.

Marker-line comparisons must `line.trim_end() == marker`, never
`line == marker`.

Surfaced by: same Task 10 e2e tests; the unit test fixtures had
clean markers and didn't catch this.

## `cmd.exe` synthesizes vars that aren't in the parent env

`cmd.exe` invents a handful of variables on the fly when the parent
process didn't pass them down — most notably `PROMPT=$P$G`. They show
up in `set` output but are absent from `std::env::vars_os()`. So
sampling pre-env via Rust's `std::env` and post-env via `cmd /U ... &&
set` is **not symmetric**: synthesized vars appear as "added" in the
diff even though no batch added them.

This was invisible on developer machines (the user's interactive shell
already exports `PROMPT`, so it was in both sides), but on the bare
`windows-latest` runner the parent service env had no `PROMPT` and the
`cli_emit` fixture compare failed for all five shells with an extra
`PROMPT='$P$G'` line.

**Use `cmd.exe`-via-`set` for both pre AND post snapshots** so any
shell-synthesized var lives in both maps and cancels in the diff. The
same `cmd /U /D /Q /C "echo MARKER && set"` shape that `capture()` runs
for the post side, but with no batch in the inner script, works for
the pre side too.

Surfaced by: first `cli_emit` run on GitHub Actions (run 25575729376) —
passed locally, all 5 tests failed on CI with an extra `PROMPT='$P$G'`
line in stdout. The first attempted fix (`.gitattributes` pinning
fixtures to LF) was a misdiagnosis — line endings were never the
issue, but the assertion message only said "diverged" without
showing the raw bytes.

Diagnostic tip: when a fixture-compare test fails on CI but not
locally, run `--log-failed` on the failed `gh run` and look for the
`left:` and `right:` lines from the panic — Rust's default
`assert_eq!` does include both sides in the panic message.
