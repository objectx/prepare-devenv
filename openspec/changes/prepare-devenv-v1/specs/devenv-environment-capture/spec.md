## ADDED Requirements

### Requirement: Snapshot pre-VsDevCmd environment

The tool SHALL snapshot the current process's environment as an `OsString → OsString` map before invoking `VsDevCmd.bat`, and MUST treat env-var name comparison as case-insensitive (Windows convention) while preserving the original casing of names for downstream output.

#### Scenario: pre-snapshot captures all visible vars

- **WHEN** the tool starts and the parent process has env vars `Foo=1`, `BAR=2`
- **THEN** the snapshot contains both, and a later lookup of `foo` (any case) MUST find the entry whose original key was `Foo`

---

### Requirement: Invoke VsDevCmd.bat inside a captured cmd.exe child

The tool SHALL invoke `VsDevCmd.bat` via `cmd.exe /U /D /Q /C "call \"<bat>\" <devcmd-args> 1>&2 && echo <MARKER> && set"` with: `/U` forcing UTF-16LE stdout, `/D` skipping `AutoRun` registry hooks, `/Q` muting the prompt, `1>&2` redirecting `VsDevCmd.bat`'s own stdout to the inner `cmd`'s stderr, a unique per-invocation marker line preceding the `set` output, and `&&` short-circuiting on `VsDevCmd.bat` failure.

#### Scenario: command line is constructed correctly

- **WHEN** the tool prepares the capture command for VsDevCmd at path `P` with `--devcmd-args "-arch=x64"`
- **THEN** the constructed argv to `cmd.exe` is exactly `/U /D /Q /C` followed by the single string `call "P" -arch=x64 1>&2 && echo <UNIQUE_MARKER> && set` where `<UNIQUE_MARKER>` is a fresh per-invocation token

#### Scenario: VsDevCmd.bat exits non-zero

- **WHEN** `VsDevCmd.bat` returns a non-zero exit code (so the `&&` chain short-circuits and `set` is never reached)
- **THEN** the tool exits with code `4` carrying `VsDevCmd.bat`'s exit code, surfaces the captured stderr at `-v`, and does not attempt to parse env

---

### Requirement: Parse UTF-16LE post-env from cmd.exe stdout

The tool SHALL read the inner `cmd.exe` child's stdout as UTF-16LE bytes, MUST strip a leading BOM if present, MUST normalize line endings to `\n`, MUST split on the unique marker line, MUST parse only the bytes after the marker as `KEY=VALUE` lines splitting on the first `=` only, and MUST surface a missing marker or otherwise unparseable output as exit code 1.

#### Scenario: well-formed capture output

- **WHEN** the inner `cmd.exe` writes a marker line followed by `INCLUDE=C:\foo;C:\bar` and `PATH=...`
- **THEN** the parser produces an env map containing those keys with the literal values after the first `=`

#### Scenario: value contains '=' characters

- **WHEN** a captured line is `MYVAR=a=b=c`
- **THEN** the parsed value is `a=b=c` (split on first `=` only)

#### Scenario: marker not found in stdout

- **WHEN** the marker line never appears in stdout (e.g., `cmd.exe` itself failed before reaching `echo`)
- **THEN** the tool exits with code `1` and emits an error suggesting the user re-run with `-v` to see `cmd`/`VsDevCmd.bat` output

#### Scenario: BOM and CRLF handling

- **WHEN** stdout begins with the UTF-16LE BOM `0xFF 0xFE` and uses `\r\n` line endings
- **THEN** the parser strips the BOM and normalizes to `\n` before splitting

---

### Requirement: Compute added/modified env diff

The tool SHALL compute the env delta against the pre-snapshot, MUST emit only entries that are present-and-different (modified) or present-only-in-post (added), MUST ignore entries removed in post (present-only-in-pre), and MUST preserve the captured value casing/contents byte-for-byte.

#### Scenario: variable added

- **WHEN** `INCLUDE` is absent in pre and present in post
- **THEN** the diff's `added` map contains `INCLUDE` with the post value

#### Scenario: variable modified

- **WHEN** `PATH` is `A;B` in pre and `X;A;B` in post
- **THEN** the diff's `modified` map contains `PATH` with value `X;A;B`

#### Scenario: variable unchanged

- **WHEN** `USERNAME` is identical in pre and post
- **THEN** the diff contains no entry for `USERNAME`

#### Scenario: variable removed in post

- **WHEN** `OLDVAR` is present in pre and absent in post
- **THEN** the diff contains no entry for `OLDVAR` (removals are explicitly ignored)

---

### Requirement: Provide injection seam for tests

The capture module SHALL expose a `capture_with(reader)`-style entry point taking a byte-source argument, with the public `capture(...)` constructed on top of it; tests MUST be able to drive the parser end-to-end using committed UTF-16LE fixture bytes and without spawning `cmd.exe` or requiring an installed Visual Studio.

#### Scenario: fixture bytes drive the parser without cmd.exe

- **WHEN** a test calls `capture_with` with a UTF-16LE byte vector loaded from `tests/fixtures/` containing a marker line followed by realistic `set` output
- **THEN** the test obtains a parsed env map identical to what a live `cmd /U` invocation would have produced
