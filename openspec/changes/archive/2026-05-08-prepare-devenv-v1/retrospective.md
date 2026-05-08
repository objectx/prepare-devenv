# Retrospective: prepare-devenv-v1

> Written: 2026-05-09 (after verify passed with warnings)
> Commit range: `1f9442f..fa393c4`
> Worktree: `D:\pdi\prepare-devenv\.worktrees\change-prepare-devenv-v1` (branch `change/prepare-devenv-v1`, not yet pushed)

---

## 0. Evidence

- **Commit range**: `1f9442f..fa393c4` (26 commits)
- **Diff size**: +5595 / −87 across 50 files
- **Tasks done**: 75/75 (`grep -c '^- \[x\]' tasks.md` → 75; 0 unchecked)
- **Active hours**: ~1 working session, multi-hour, single human + Claude
- **Subagent dispatches**: ~30 (13 implementer + 13 spec reviewer + 13 code-quality reviewer + 6 fix-up + 1 final review + 2 retries on API overload + a handful of ad-hoc reviews); two Tasks were implemented directly by the controller (Task 9, fish-escape spec gap, prerelease filter, Cli docstring fix) when the API was overloaded and dispatching would have wasted minutes
- **New external dependencies**: 11 runtime + 3 dev — `clap` (derive), `serde` (derive), `serde_json`, `thiserror`, `tracing`, `tracing-subscriber` (env-filter, fmt), `windows` (Win32_System_Diagnostics_ToolHelp + Win32_System_Threading + Win32_Foundation); dev: `assert_cmd`, `predicates`, `tempfile`. `anyhow` was initially in runtime deps and moved to dev-deps in `4c0bd00`. All from crates.io, all permissively licensed
- **Bugs encountered post-merge**: none — branch not yet merged. Two correctness bugs were caught DURING the cycle by Task 10's e2e tests (Windows arg-escaping mangling `cmd /C` inner string; `echo X && set` emitting trailing space on the marker line) — see §5 Surprises
- **OpenSpec validate state at archive**: pass (verify §1 confirmed `valid: true`)
- **Test coverage signal**: 116 unit + 15 e2e (`test_hooks`) = 131 tests passing; `cargo doc --no-deps` clean; clippy clean under default + `test_hooks` + `live` features

Commit chain (chronological, oldest first):

```
1f9442f docs(openspec): restore prepare-devenv-v1 change artifacts
daadee4 chore: scaffold prepare-devenv crate (Rust 2024)
79a6d6d chore(openspec): mark Task 1 complete in tasks.md
55724f0 feat(error): introduce error::Error and Result alias
bcfca16 chore(openspec): mark Task 2 complete in tasks.md
1af786a feat(cli): clap-derive arg parsing and mode dispatch
eee798b test(cli): tighten conflict tests; add multi-arg, --shell, -vv coverage
6ed1625 feat(discovery): vswhere lookup, JSON parse, install selection
cf17f6b fix(discovery): deterministic tiebreak; cleaner vswhere stderr error
225ea5b feat(capture): cmd /U capture pipeline + UTF-16LE parser
99a68e4 docs(capture): document quoting/BOM assumptions; harden fresh_marker uniqueness
6329908 feat(diff): added/modified env delta with CI key matching
cc1bb76 refactor(diff): linearize merged_env; remove dead key_eq_ci helper
20bcd0a feat(shell): trait, five adapters, parent-shell detection
c6c8f59 fix(shell/fish): escape backslashes; add bash edge-case tests; doc nu escape order
8065f81 feat(runner): emit / spawn_shell / spawn_command output paths
f557b10 refactor(runner): single-pass build_shell_env; strengthen emit error test
6b9531a feat(main): orchestration, tracing init, exit-code mapping
26dc882 test: end-to-end CLI suite via fake VsDevCmd.bat
29fb395 test: cover --emit -v stdout-cleanliness and trailing-whitespace marker
b028071 test(live): integration tests behind 'live' feature; document in README
38d5ea1 ci: windows runner with fmt/clippy/test, manual live + release dispatch
96003c9 docs: README + CLAUDE.md update for v1, smoke checklist, lessons
4c0bd00 fix: forward VsDevCmd stderr; trace capture/vswhere/parent-walk; richer AmbiguousId
8ec0a52 feat(discovery): exclude prereleases from Latest auto-select
f5ef80e fix(cli): demote Cli rustdoc to non-doc comment
fa393c4 docs(openspec): write verify.md (PASS WITH WARNINGS)
```

---

## 1. Wins

- [evidence: §0 commit chain] **Per-Task two-stage review caught real spec gaps before they reached the final review.** Out of 13 Tasks, 6 produced a follow-up `fix(...)` or `refactor(...)` commit driven by reviewer feedback (Tasks 4, 5, 6, 7, 8, 10) before moving on. The fish-trailing-backslash escape (commit `c6c8f59`) and the diff merged-env quadratic (`cc1bb76`) would have been silent latent bugs without that loop.
- [evidence: `tests/cli_emit.rs`, `tests/cli_spawn_command.rs`, commit `26dc882`] **The `test_hooks` Cargo feature + fake `.bat` end-to-end harness paid off immediately.** Two real correctness bugs in `src/capture.rs` (Windows arg-escaping; trailing-space marker line) only surfaced once Task 10 wired up `assert_cmd` against the real binary. Unit-test fixtures had been hand-crafted to match the broken implementation, so they couldn't catch what they were unwittingly accommodating. Lesson recorded in [tasks/lessons.md](tasks/lessons.md).
- [evidence: §0 test coverage; `tests/fixtures/`] **The `_with(...)` testability seam pattern is repeated across discovery / capture / diff / shell::detect / runner::build_shell_env, and pays for itself.** 116/131 tests run with no Visual Studio, no `vswhere`, no `cmd /U` invocation — the fixture-driven and closure-injected paths cover the vast majority of behavior. Live tests (gated behind `--features live`) only carry the irreducible "real VS install" assertions.
- [evidence: `src/diff.rs::merged_env`, `src/runner.rs::build_shell_env`, commits `cc1bb76` and `f557b10`] **Single-pass O(n+m) implementations replaced O(n*m) merge-then-overwrite versions, and the codebase ended up shorter, not longer.** The clippy `-D warnings` gate plus the Important-level reviewer feedback caught both passes' wastefulness on first review.
- [evidence: `openspec/changes/prepare-devenv-v1/specs/`, all 5 capabilities] **The spec-driven test naming made the verify §4 spec-coherence check mechanical.** Every spec scenario maps to a named test (`well_formed_capture_output` ↔ `Scenario: well-formed capture output`, etc.); auditing coverage was a grep-and-walk exercise rather than re-derivation.
- [evidence: commits `4c0bd00`, `8ec0a52`, `f5ef80e`] **The final reviewer's holistic pass caught three real spec gaps that 13 per-Task reviews missed.** The diagnostics surface (VsDevCmd stderr forwarding under `-v`; tracing on capture/vswhere/parent-walk; AmbiguousId candidate listing) is fundamentally cross-cutting, so it was invisible to a Task-scoped reviewer. The user-driven prerelease filter and `--help` cleanup arrived AFTER the final review — both pure UX issues that only surface when someone actually runs the binary, not when they read the diff.

## 2. Misses

- 🟡 [painful | evidence: commit `26dc882` report] **Two correctness bugs in `src/capture.rs` got past 5 per-Task reviews and lived for 4 Task cycles before Task 10 surfaced them.** Both were Windows-platform-specific (`Command::arg`'s `\"` escaping; `echo X && set` trailing space). The unit tests at Task 5 / 7 used hand-crafted UTF-16LE fixture bytes that happened to match the broken implementation byte-for-byte, so they passed AND covered the spec scenarios on paper. **Real lesson**: when fixture-driven testing is the primary mechanism, make sure at least one e2e test forces fixtures to match REAL output, not author-inferred output. The lessons are now in [tasks/lessons.md](tasks/lessons.md) so this doesn't recur.

- 🟡 [painful | evidence: final-review report at commit `4c0bd00`] **The diagnostics-surface spec was missed by 13 per-Task reviewers and only caught at the holistic final review.** Three MUST-level items (VsDevCmd stderr forwarding under `-v`, tracing on capture/vswhere/parent-walk, AmbiguousId candidate listing) are cross-cutting — none of them lives in a single Task's scope. **Real lesson**: cross-cutting spec scenarios (anything starting with "everywhere" or "all paths" or "every diagnostic") need to be either (a) listed against the specific Task that introduces the affected code, AND (b) explicitly cross-referenced in the final-review prompt. The current per-Task review prompts didn't surface them.

- 🟡 [painful | evidence: commits `8ec0a52`, `f5ef80e`] **Two user-facing UX issues only surfaced when the user actually invoked the binary.** The `Cli` rustdoc leaking into `--help` was flagged at the final review (Minor #6) but I deferred it; the user complained immediately. The "Latest" selector picking prereleases auto-magically wasn't even on the spec's radar — the spec said "highest version", which technically `18.0.0-preview.2` is. **Real lesson**: defer fewer review minors; even "cosmetic" UX issues become blockers the moment a user runs the binary. And: spec language matters — "highest version" reads as a math statement but users mean "the one I should use by default."

- 📌 [nit | evidence: `Cargo.toml` history; commit `4c0bd00`] **`anyhow` was initially in runtime deps but only used in tests.** Caught by the final reviewer. Cost: ~0; moving it was a one-line change. Logged here so a future cycle's pre-Task-1 dep-list review remembers to ask "is each runtime dep actually called from non-test code?".

- 📌 [nit | evidence: `src/discovery.rs:131`, `src/shell/mod.rs:198`] **Tracing was added in two places to satisfy the spec, but the call-sites were chosen mechanically rather than by walking the user's `-v`-driven debugging journey.** A user trying to debug why a wrong VS was picked needs to see the candidate list AT pick time, not just the deserialization step. Acceptable for v1; the call sites do satisfy the spec.

- 📌 [nit | evidence: `openspec/changes/prepare-devenv-v1/design.md` D6] **`design.md` D6 was not updated to reflect the prerelease filter.** The spec was updated in commit `8ec0a52`, but design.md still says "highest `installationVersion`". `verify.md` §4 flagged this; it's a one-line edit but never landed because we moved straight from the user's directive into apply, and the schema's "fluid workflow" lets spec amendments through without an explicit design.md re-read step.

## 3. Plan deviations

| Plan task | What changed | Why |
|---|---|---|
| 5.5 (`capture` return type) | Returns `HashMap<OsString, OsString>` directly instead of a `CapturedEnv` newtype; later expanded to take a `forward_stderr_on_success: bool` parameter | The wrapper newtype added zero value (only consumer is `diff::diff` which takes `&HashMap`); the bool was added in commit `4c0bd00` to forward `VsDevCmd.bat`'s stderr to user stderr at `-v` per the spec's MUST. Documented in module docs. |
| 5.3 (`fresh_marker`) | Implementation hardened with a process-local `AtomicU64` counter on top of `SystemTime + pid` | Task 5 reviewer flagged that the original implementation could collide on slow CI runners with coarse-grained clocks. Counter guarantees uniqueness within a single SystemTime tick. |
| 6.6 (`merged_env`) | Implementation collapses case-variant base keys (e.g. `Path` → `PATH`) before insert; doc explains Windows case-folding rationale | The plan's pseudocode `for (k, v) in self.iter() { out.insert(k.clone(), v.clone()); }` would have left both `Path` and `PATH` in the map. `Command::envs(...)` then lets Windows pick a winner non-deterministically at process-creation time. Reviewer #6 flagged the original two-pass implementation as wasteful (O(n*m)); commit `cc1bb76` linearized it to O(n+m). |
| 7.7 (fish escape) | Escape rule changed from `'` → `\'` only to `\\` → `\\\\` first, then `'` → `\'` | Reviewer #7 noted that VS env vars routinely end in `\` (every `*INSTALLDIR` path has trailing `\`), and the original escape produced unterminated fish strings for them. Fixed in `c6c8f59`. The spec scenario text in `shell-adapter-suite/spec.md` still shows the old form — flagged in `verify.md` §4 for retrospective amendment. |
| 7.8 (nu escape) | Plan said `'` → `\''`; implementation uses single-quoted form (no escape needed) when value lacks `'`, switches to double-quoted with `\\` / `\"` / `\$` escapes when value has `'` | Verified against the official nushell book: nushell single-quoted strings are FULLY LITERAL with no escape sequence at all. The plan's `\''` rule was incorrect. Documented in module docs. |
| 8.4 (pwsh fallback) | `pwsh.exe` → `powershell.exe` fallback only triggers on `ErrorKind::NotFound` AND `shell.kind() == ShellKind::Pwsh` | Reviewer caution: fallback shouldn't paper over permission errors or other failure modes. Implementation matches the user expectation. |
| 9.5 (test_hooks `--devcmd-script` override) | Synthesizes a fake `ResolvedInstall` rather than going through `discovery::resolve` | Lets tests bypass `vswhere` entirely; expected for the test seam. |
| 10.x (capture bug fixes inline) | Task 10's e2e tests immediately failed; bugs in `src/capture.rs` were fixed inline within the Task 10 commit (`26dc882`) instead of a separate "fix(capture)" commit | The implementer subagent's call: the fixes were prerequisites for the Task 10 tests passing, and bundling them kept the e2e suite committable as one unit. `tasks/lessons.md` captures the fixes for future reference. |
| Latest selector (post-final-review user directive) | Filter prereleases from `Latest`; only `--id`/`--path` can target a prerelease | User clarification mid-apply, after the final review. Spec was updated to match (`8ec0a52`). `design.md` D6 was NOT updated — flagged in verify §4. |
| Cli rustdoc | Demoted to `//` non-doc comment so it doesn't leak into `--help` | Reviewer Minor #6, deferred during final-review fix-up; user reported it as an annoyance immediately after launching. Fixed in `f5ef80e`. |

## 4. Skill / workflow compliance

| Skill | Used |
|---|---|
| superpowers:brainstorming | ✓ |
| superpowers:writing-plans | ✓ (within `/opsx:propose`) |
| superpowers:using-git-worktrees | ✓ |
| superpowers:subagent-driven-development | ✓ |
| (transitive) superpowers:test-driven-development | ✓ (each implementer subagent followed RED-GREEN, with a few pragmatic "write all tests then implement" deviations called out by the implementer themselves) |
| (transitive) superpowers:requesting-code-review | ✓ |
| superpowers:finishing-a-development-branch | (pending — runs after archive) |

### Deliberately Skipped Skills

(none — every apply-phase skill was used. Two carve-outs worth noting for transparency, both within the bounds of the skill's escape hatches:)

- **Task 9 (`main.rs` orchestration) was implemented by the controller directly, not via an implementer subagent.** The Anthropic API was repeatedly returning 529 Overloaded after 3 retry attempts; continuing to retry would have wasted minutes per Task. The controller had full context (it had reviewed all 8 prior Tasks, knew the imports, and the work was bounded — one file rewrite plus removing 6 module-level `#![allow(dead_code)]` lines). I went direct, ran all gates, then continued with normal subagent dispatch from Task 10 onward. **Why not a §4 skip**: the skill explicitly allows the controller to take direct action when subagent dispatch would be net-negative; this was that case. The decision is captured in commit `6b9531a`'s message and in this retro for audit.

- **Several Task fix-ups were applied by the controller directly** (final-review fixes for items #4, #5, #6, #7, #8 plus the prerelease-filter and Cli-docstring fixes). Same rationale: these were 3–10-line targeted changes with the controller having full context, and dispatching a subagent for "delete two lines and add a comment" wastes more time than it saves. The skill's "If subagent fails task: don't try to fix manually (context pollution)" rule applies to *implementation failures*, not to small fixes after a successful task. Captured here so future cycles can see the threshold.

## 5. Surprises

- **Windows arg-escaping for `cmd /C "..."` is a footgun, not a corner case.** Rust's `Command::arg` rewrites embedded `"` as `\"` per Microsoft's documented quoting algorithm, which `cmd.exe` itself does NOT understand inside `/C`'s inner script. The fix (`std::os::windows::process::CommandExt::raw_arg`) is well-known but not the obvious first reach for someone writing `cmd.exe /C "..."` in idiomatic Rust. Recorded in `tasks/lessons.md`.

- **`echo MARKER && set` produces `MARKER\n` PLUS a trailing space before `&&`.** `cmd.exe`'s tokenization keeps the literal whitespace between `echo MARKER` and `&&` as part of `echo`'s argument list. The unit tests in Task 5 had clean markers because they were hand-crafted; only Task 10's real spawn surfaced this. Fix is `line.trim_end() == marker`. Recorded in `tasks/lessons.md`.

- **`thiserror`'s `#[from]` plus a struct-shaped variant produces a `From` impl that conflicts with multiple `#[from]` variants of the same wrapped type.** Not encountered in this cycle, but worth a passing note for a future cycle that adds another `Json(serde_json::Error)`-shaped variant.

- **`cargo clippy --all-targets -- -D warnings` does NOT auto-enable `dead_code` warnings on `#![allow(dead_code)]`-decorated modules.** I'd assumed Task 9's "remove the allow and run clippy" would surface every still-unused symbol, but the module-level allow had been masking everything. After removing it, only `ResolvedInstall::display_name` and `Error::UnknownShell` remained — both narrowed to per-item allows with explanations. The "what's actually dead" question gets answered correctly, but only if you actually remove the module-level allow.

- **Fish single-quoted strings DO recognize `\\` and `\'` escapes** — but ONLY those two. Other backslashes are literal. So a path like `C:\foo\bar` doesn't strictly NEED escaping, until you hit the trailing-backslash case (`C:\foo\`) where the `\'` accidental sequence eats the closing quote. This is the kind of corner case that only shows up with real VS data; a tighter doc-driven test would have caught it earlier.

- **`Cli` rustdoc was masking `--help` output.** I expected `#[command(about = "...")]` to override the rustdoc, but clap-derive uses the rustdoc as `long_about` and the `about` attribute as `about`; both appear under `--help`, with the rustdoc taking visual priority because it's longer. Reviewer Minor #6 caught it; the user caught it within minutes of running `--help`.

## 6. Promote candidates → long-term learning

- [ ] 🟡 **Cross-cutting spec scenarios need a final-review checklist that walks them by hand** → **Promote to schema** (`openspec/schemas/superpowers-bridge/templates/verify.md` §4 or a new §4.5)
  > **Why**: 13 per-Task reviewers missed three MUST-level diagnostic spec gaps because each scenario's affected code lives across multiple Tasks. The final reviewer caught them only because they did a holistic pass.
  > **How to apply**: Add a `### Cross-cutting requirements` row to the verify template's §4 (or a new §4.5) that explicitly lists "diagnostic surfaces", "exit-code mappings", "pristine-stdout invariants" as a category. Reviewers prompted at archive time will then verify each has at least one assertion across the codebase.

- [ ] 🟡 **Hand-crafted fixtures should be paired with at least one "fixture matches real output" assertion** → **Promote to memory (feedback)**
  > **Why**: Task 5 / 7 unit tests passed AND covered spec scenarios on paper, but the fixtures were author-inferred and accidentally matched a broken `cmd /C` argv shape and missed the `echo X && set` trailing-space quirk. Two correctness bugs lived for 4 Task cycles.
  > **How to apply**: When a module's spec coverage is fixture-driven (capture-style modules, anything that round-trips bytes through an external process), require at least one e2e test that runs the real production code path against real output, not author-fabricated bytes. The e2e harness can live behind a Cargo feature.

- [ ] 📌 **`design.md` should be re-read whenever a spec is amended mid-apply** → **Promote to schema** (apply-phase instruction in `archive.md` or a new step in verify §4)
  > **Why**: User directed mid-apply that `Latest` should skip prereleases. Spec was updated (`8ec0a52`); `design.md` D6 was not. Now `design.md` and the spec disagree until the retrospective amendment lands.
  > **How to apply**: At verify time, if the apply-phase commit log shows any spec-only edits without a corresponding design.md edit, flag for retrospective review. Or: bake into the bridge schema's archive step a "diff design.md vs synced specs" sanity check.

- [ ] 📌 **Don't defer reviewer-flagged UX minors that affect the binary's first-run output** → **Promote to memory (feedback)**
  > **Why**: Final reviewer Minor #6 flagged the `Cli` rustdoc leaking into `--help`. I deferred it. The user complained within minutes of running the binary. Cost to fix: 4 lines.
  > **How to apply**: When a Minor specifically affects user-visible output (--help, --version, default error messages, default stdout), promote to "fix before merge" rather than "follow-up". The 5-minute fix is always cheaper than the user-feedback ping.

- [ ] 📌 **`anyhow` in a binary crate's runtime deps is suspicious** → **Memory (feedback)** (project-specific)
  > **Why**: `anyhow` was added to runtime deps at Task 1, but its only call site was a `#[cfg(test)]` block in `src/error.rs`. Caught at final review (Minor #5), moved to dev-deps.
  > **How to apply**: When running `cargo add anyhow` in a binary crate's bootstrap, check whether the `main()`/`run()` paths actually consume `anyhow`'s features. If they don't, move to dev-deps.

- [ ] 📌 **Module-level `#![allow(dead_code)]` masks future-introduced dead code** → **One-off** (specific to the bootstrap-and-wire sequencing in this kind of pipeline)
  > **Why**: We added the allows because Tasks 2–8 produced symbols before Task 9 had a chance to call them. Removing them at Task 9 surfaced two genuinely-dead items, but only because they were the residue. Future-introduced dead code in the same module wouldn't have been flagged until the next clippy run after the allow came off.
  > **How to apply**: Doesn't generalize. The pattern is fine for "build-up-then-wire" cycles, just remove the allows AT the wire-up step (Task 9 in this cycle's case) so subsequent cycles get clippy's dead-code coverage.
