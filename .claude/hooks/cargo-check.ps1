#requires -Version 7
# Stop hook: keeps the working tree formatted and lint-clean before Claude
# ends a turn. Always runs `cargo fmt`; runs `cargo clippy` (default features
# and the `test_hooks` feature) only when the working tree contains dirty
# `.rs` files. On any failure, exits 2 with the captured tool output on
# stderr so Claude Code feeds it back to the assistant.

$ErrorActionPreference = 'Stop'

# --- Loop guard: respect Claude Code's stop_hook_active flag ---
$stdin = [Console]::In.ReadToEnd()
if ($stdin) {
    try {
        $hookInput = $stdin | ConvertFrom-Json
        if ($hookInput.stop_hook_active -eq $true) { exit 0 }
    } catch {
        # Fall through and run anyway if the payload isn't parseable.
    }
}

# --- Snapshot .rs dirtiness BEFORE fmt runs, so fmt's own writes don't
#     trigger clippy on an otherwise non-Rust turn.
$gitStatus = git status --porcelain 2>&1
if ($LASTEXITCODE -ne 0) {
    [Console]::Error.WriteLine("cargo-check hook: git status failed (exit $LASTEXITCODE):")
    [Console]::Error.WriteLine(($gitStatus | Out-String).TrimEnd())
    exit 2
}
$rsDirty = $false
foreach ($line in $gitStatus) {
    if ($line -match '\.rs$') { $rsDirty = $true; break }
}

# --- Always: cargo fmt ---
$fmtOut  = cargo fmt 2>&1 | Out-String
$fmtExit = $LASTEXITCODE
if ($fmtExit -ne 0) {
    [Console]::Error.WriteLine("cargo fmt failed (exit ${fmtExit}):")
    [Console]::Error.WriteLine($fmtOut.TrimEnd())
    exit 2
}

if (-not $rsDirty) { exit 0 }

# --- cargo clippy: default features and test_hooks. Run both before
#     reporting so the assistant sees every problem in one round-trip.
$clippy1Out  = cargo clippy --all-targets -- -D warnings 2>&1 | Out-String
$clippy1Exit = $LASTEXITCODE
$clippy2Out  = cargo clippy --all-targets --features test_hooks -- -D warnings 2>&1 | Out-String
$clippy2Exit = $LASTEXITCODE

if ($clippy1Exit -ne 0 -or $clippy2Exit -ne 0) {
    [Console]::Error.WriteLine("cargo clippy failed.")
    [Console]::Error.WriteLine("--- cargo clippy --all-targets -- -D warnings (exit ${clippy1Exit}) ---")
    [Console]::Error.WriteLine($clippy1Out.TrimEnd())
    [Console]::Error.WriteLine("--- cargo clippy --all-targets --features test_hooks -- -D warnings (exit ${clippy2Exit}) ---")
    [Console]::Error.WriteLine($clippy2Out.TrimEnd())
    exit 2
}

exit 0
