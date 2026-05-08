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
