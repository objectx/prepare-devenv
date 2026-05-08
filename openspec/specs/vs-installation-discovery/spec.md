# vs-installation-discovery Specification

## Purpose
TBD - created by archiving change prepare-devenv-v1. Update Purpose after archive.
## Requirements
### Requirement: Locate vswhere.exe at standard install path

The tool SHALL locate `vswhere.exe` by first probing `%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe` and, on absence, falling back to a `%PATH%` search; failure to locate it MUST surface as a clean discovery error (exit code 3) rather than a panic, deserialization failure, or downstream symptom.

#### Scenario: vswhere present at standard installer path

- **WHEN** `%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe` exists and is executable
- **THEN** the tool uses that binary and does not consult `%PATH%`

#### Scenario: vswhere absent from installer path but on %PATH%

- **WHEN** the standard installer-path probe fails but `where vswhere` resolves a binary on `%PATH%`
- **THEN** the tool uses the `%PATH%`-resolved binary

#### Scenario: vswhere absent everywhere

- **WHEN** neither the installer-path probe nor a `%PATH%` lookup yields `vswhere.exe`
- **THEN** the tool exits with code `3` and an error message naming both probed locations and pointing the user at the VS Installer

---

### Requirement: Enumerate Visual Studio installations via vswhere JSON

The tool SHALL invoke `vswhere -format json -all -prerelease -products *`, MUST deserialize stdout into a typed list of installations exposing at minimum `installationPath`, `installationVersion`, and `instanceId`, and MUST surface a non-zero exit from `vswhere` or malformed JSON as a discovery error (exit code 3).

#### Scenario: vswhere returns one or more installations

- **WHEN** `vswhere` exits 0 with a JSON array of installs
- **THEN** the tool deserializes them into the internal install list and proceeds to selection

#### Scenario: vswhere returns an empty array

- **WHEN** `vswhere` exits 0 with `[]`
- **THEN** the tool exits with code `3` and an error message indicating no Visual Studio installations were found

#### Scenario: vswhere exits non-zero or emits malformed JSON

- **WHEN** `vswhere` exits non-zero, or its stdout is not parseable as JSON in the expected shape
- **THEN** the tool exits with code `3` and includes the underlying error (and `vswhere` stderr at `-v`) in the message

---

### Requirement: Resolve a single install by --id, --path, or latest

The tool SHALL resolve exactly one Visual Studio install per invocation according to a deterministic precedence: `--path <dir>` MUST match `installationPath` case-insensitively (Windows path-equivalent); `--id <prefix>` MUST match the leading hex characters of `instanceId`; absent both, the tool MUST select the install with the highest **stable** `installationVersion` (semver-style numeric compare, not lexical) with `installDate` descending as a tie-breaker. Prerelease installs (those with `isPrerelease: true` in vswhere's JSON output — note that `installationVersion` itself does NOT carry a `-preview` / `-insiders` suffix, so the boolean is the only reliable signal) MUST be excluded from auto-selection but MUST remain reachable via `--id` and `--path`. If the host has only prerelease installs, the tool MUST fall back to the highest prerelease (with a `tracing::warn!` emitted) rather than refusing to pick. Zero matches or ambiguous `--id` MUST surface as a discovery error (exit code 3).

#### Scenario: --path matches exactly one install

- **WHEN** `--path "C:\Program Files\Microsoft Visual Studio\18"` is given and exactly one install has that `installationPath` (case-insensitive)
- **THEN** the tool selects that install and proceeds

#### Scenario: --id prefix matches exactly one install

- **WHEN** `--id d0b481ec` is given and exactly one install's `instanceId` begins with `d0b481ec`
- **THEN** the tool selects that install

#### Scenario: --id prefix matches multiple installs

- **WHEN** `--id ab` is given and two or more installs have an `instanceId` beginning with `ab`
- **THEN** the tool exits with code `3`, lists the matching candidates' short ids and paths in the error, and does not pick arbitrarily

#### Scenario: neither --id nor --path given

- **WHEN** no selector flag is provided and at least one **stable** install exists
- **THEN** the tool selects the install with the highest stable `installationVersion`, breaking ties by `installDate` descending

#### Scenario: neither --id nor --path given, prereleases mixed with stable

- **WHEN** the install list contains both prereleases (`isPrerelease: true`) and stable installs, including the case where a prerelease has a numerically higher `installationVersion` than every stable install
- **THEN** the tool selects the highest stable install and ignores prereleases regardless of their version

#### Scenario: neither --id nor --path given, only prereleases installed

- **WHEN** the install list contains only prereleases
- **THEN** the tool falls back to the highest prerelease (numeric version compare, `installDate` tie-break) and emits a `tracing::warn!` so a `-v` run surfaces the choice

#### Scenario: --id or --path can target a prerelease

- **WHEN** the user passes `--id <prefix>` or `--path <dir>` matching a prerelease install
- **THEN** the tool selects that prerelease without applying the auto-selection prerelease filter

#### Scenario: --path or --id matches zero installs

- **WHEN** `--path P` or `--id I` is provided and no install matches
- **THEN** the tool exits with code `3` with an error naming the selector kind and value

---

### Requirement: Compute VsDevCmd.bat path from resolved install

The tool SHALL derive the resolved install's `VsDevCmd.bat` path as `{installation_path}\Common7\Tools\VsDevCmd.bat` and MUST verify the file exists before invoking it; absence MUST surface as a discovery error (exit code 3) distinct from a `VsDevCmd.bat` runtime failure.

#### Scenario: VsDevCmd.bat exists at the computed path

- **WHEN** the resolved install's computed `VsDevCmd.bat` path exists on disk
- **THEN** the tool proceeds to the env-capture stage with that path

#### Scenario: VsDevCmd.bat missing at the computed path

- **WHEN** the computed path does not exist (e.g., a corrupt install or a non-IDE install lacking the developer tools workload)
- **THEN** the tool exits with code `3` with an error naming the missing path and the resolved install's id and version

