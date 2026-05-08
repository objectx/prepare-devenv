# `prepare-devenv` Smoke Checklist

Manual smoke tests to run on a Windows machine with Visual Studio installed
(VS 2017+) before publishing a release. Each line is a one-shot end-to-end
check.

```pwsh
# 1. cl.exe lookup via -- COMMAND mode
prepare-devenv -- where cl.exe
# Expect: full path to MSVC's cl.exe; exit 0.

# 2. --emit mode in pwsh: pipe into the current process
prepare-devenv --emit | iex
where.exe cl.exe
# Expect: cl.exe found in the current pwsh session.

# 3. spawn-shell mode (no args) from each supported shell
#   - cmd.exe -> spawns child cmd.exe with VS env merged
#   - pwsh.exe -> spawns child pwsh.exe with VS env merged
#   - bash (Git Bash) -> spawns bash with VS env merged + POSIX PATH
#   - fish -> spawns fish with VS env merged
#   - nu -> spawns nu with VS env merged
prepare-devenv

# 4. discovery error: bogus --id
prepare-devenv --id bogus
# Expect: exit 3, error message naming the selector and value.

prepare-devenv -v --id bogus
# Expect: same exit 3, plus -v info logs (vswhere candidates listed).

# 5. cross-arch sanity check
prepare-devenv --devcmd-args="-arch=arm64" -- cl.exe -?
# Expect: cl.exe -? help output reports arm64 target.
```

Keep this checklist short — it's for release-time confirmation, not
exhaustive testing. Most coverage is automated via `cargo test
--features live`.
