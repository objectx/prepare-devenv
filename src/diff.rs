//! Compute the env-var delta between the pre- and post-`VsDevCmd.bat` snapshots.
//!
//! [`diff`] produces an [`EnvDiff`] containing two [`BTreeMap`]s — `added`
//! (keys present in `post` but not in `pre`) and `modified` (keys present
//! in both with byte-different values). Removals (keys present only in
//! `pre`) are deliberately ignored: the spec under
//! `specs/devenv-environment-capture/spec.md` says we only emit
//! present-and-different or present-only-in-post entries.
//!
//! ## Why `BTreeMap` and not `HashMap`
//!
//! Downstream consumers — `runner::emit` for `--emit` mode and the spawn
//! paths for `--` and shell-launch modes — iterate the diff. Using
//! `BTreeMap` gives a stable sorted-by-key iteration order, which keeps
//! `--emit` output deterministic across runs (important for snapshot tests
//! and for users diffing emit output).
//!
//! ## Windows env-var name case
//!
//! Windows looks up env-var names case-insensitively but stores them with
//! whatever casing the writer used. `HashMap` and `BTreeMap` are
//! case-sensitive, so [`diff`] folds names to lower-case via
//! `OsStr::to_string_lossy().to_ascii_lowercase()` for comparison while
//! preserving the post side's original casing in the output. The
//! `to_string_lossy` call assumes env-var *names* are ASCII-only on
//! Windows in practice — values are not constrained, but names from
//! `VsDevCmd.bat` and the inherited environment are uniformly ASCII. If a
//! non-ASCII name ever surfaced, the lossy conversion would still produce
//! a deterministic key for both sides of the comparison; correctness
//! degrades only if two distinct non-ASCII names lower-case to the same
//! lossy form, which is not a real-world concern.
//!
//! ## `merged_env` case-folding
//!
//! [`EnvDiff::merged_env`] is what the spawn paths in Task 8 use to build
//! the child process's environment. Plain `HashMap::insert` would happily
//! keep both `Path=...` (from the base/pre snapshot) and `PATH=...` (from
//! the diff) as distinct entries; on `Command::envs(...)`, Windows folds
//! them at process-creation time and the spawned process sees only one of
//! them, with implementation-defined precedence. To avoid that footgun,
//! `merged_env` filters base entries against the set of (lower-cased) diff
//! keys before inserting the diff entries — the merged map then has at
//! most one entry per logical name, and that entry uses the post side's
//! casing. See [`EnvDiff::merged_env`] for the implementation details.

#![allow(dead_code)] // wired into runner.rs by Task 8 and main.rs by Task 9

use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;

/// The set of env-var changes produced by running `VsDevCmd.bat`.
///
/// `added` holds keys present in `post` but not in `pre`; `modified` holds
/// keys present in both with byte-different values. Both maps use the
/// post side's original casing as the key. Iteration via
/// [`EnvDiff::iter`] yields `added` first then `modified`, each sorted
/// alphabetically by the lossy-string form of the key.
#[derive(Debug, Default, Clone)]
pub struct EnvDiff {
    /// Keys present in `post` but not in `pre`.
    pub added: BTreeMap<OsString, OsString>,
    /// Keys present in both `pre` and `post` with byte-different values.
    pub modified: BTreeMap<OsString, OsString>,
}

/// Compute the added / modified env delta between `pre` and `post`.
///
/// Name comparison is ASCII case-insensitive (Windows convention); when a
/// post key matches a pre key under that comparison, the post side's
/// original casing is what lands in the resulting `EnvDiff`. Values are
/// compared byte-for-byte (`OsStr: PartialEq`), so any change — even a
/// trailing-whitespace tweak — is treated as a modification.
///
/// Removals (keys in `pre` but not in `post`) are explicitly ignored per
/// the spec: a typical `VsDevCmd.bat` run does not unset inherited vars,
/// and propagating removals would force every shell adapter to also
/// emit unset directives.
pub fn diff(pre: &HashMap<OsString, OsString>, post: &HashMap<OsString, OsString>) -> EnvDiff {
    // Build a lookup keyed by lower-cased name → (original key, value)
    // from `pre`. We retain the value so we can compare byte-for-byte
    // without re-hashing.
    let mut pre_lc: HashMap<String, &OsString> = HashMap::with_capacity(pre.len());
    for (k, v) in pre {
        let key_lc = k.to_string_lossy().to_ascii_lowercase();
        pre_lc.insert(key_lc, v);
    }

    let mut diff = EnvDiff::default();
    for (k, v) in post {
        let key_lc = k.to_string_lossy().to_ascii_lowercase();
        match pre_lc.get(&key_lc) {
            None => {
                diff.added.insert(k.clone(), v.clone());
            }
            Some(pre_v) => {
                if (*pre_v).as_os_str() != v.as_os_str() {
                    diff.modified.insert(k.clone(), v.clone());
                }
                // Else: present-and-equal — skip per spec.
            }
        }
    }
    diff
}

impl EnvDiff {
    /// Iterate `added` (sorted) then `modified` (sorted).
    ///
    /// Order is stable across runs because both backing maps are
    /// `BTreeMap`s. Callers that need a single sorted stream regardless
    /// of bucket should re-sort the chained output themselves; we
    /// preserve the bucket separation because emit output benefits from
    /// grouping new vars before mutated ones.
    pub fn iter(&self) -> impl Iterator<Item = (&OsString, &OsString)> {
        self.added.iter().chain(self.modified.iter())
    }

    /// Overlay this diff onto `base`, returning a fresh map.
    ///
    /// The returned map is guaranteed to have at most one entry per
    /// logical (case-folded) env-var name, with the diff's casing
    /// winning when both sides supply a name.
    ///
    /// ## Deviation from the plan
    ///
    /// The naive overlay the plan sketched is:
    ///
    /// ```text
    /// let mut out = base.clone();
    /// for (k, v) in self.iter() { out.insert(k.clone(), v.clone()); }
    /// ```
    ///
    /// That implementation is wrong on Windows. If `base` carries `Path`
    /// (the casing the parent shell happened to use) and the diff carries
    /// `PATH` (`VsDevCmd.bat`'s casing), `HashMap::insert` treats them as
    /// distinct keys — so `out` ends up with both `Path=<old>` and
    /// `PATH=<new>`. When that map is later handed to
    /// `Command::envs(...)`, Windows folds the names case-insensitively
    /// at process-creation time and the spawned process sees only one of
    /// the two values, with implementation-defined precedence — a silent
    /// regression where the developer-environment `PATH` is randomly
    /// shadowed by the inherited one.
    ///
    /// The deliberate strategy below sidesteps that by:
    ///   1. Building a `HashSet` of lower-cased diff key names once
    ///      (`O(d)`).
    ///   2. Copying base entries into `out` only when their lower-cased
    ///      name is NOT in that set (`O(b)`).
    ///   3. Inserting every diff entry with its original (post-side)
    ///      casing (`O(d)`).
    ///
    /// Total cost is `O(b + d)` instead of the previous retain-loop's
    /// `O(b * d)`. At env sizes of ~100–200 vars the difference is
    /// negligible in wall time, but the linear form reads cleanly and
    /// removes the only quadratic in the merge path.
    pub fn merged_env(&self, base: &HashMap<OsString, OsString>) -> HashMap<OsString, OsString> {
        use std::collections::HashSet;
        let diff_keys_lc: HashSet<String> = self
            .iter()
            .map(|(k, _)| k.to_string_lossy().to_ascii_lowercase())
            .collect();
        let mut out: HashMap<OsString, OsString> = HashMap::with_capacity(base.len());
        for (k, v) in base {
            if !diff_keys_lc.contains(&k.to_string_lossy().to_ascii_lowercase()) {
                out.insert(k.clone(), v.clone());
            }
        }
        for (k, v) in self.iter() {
            out.insert(k.clone(), v.clone());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Convenience: build a `HashMap<OsString, OsString>` from string
    /// literals so test setup stays compact.
    fn map<const N: usize>(entries: [(&str, &str); N]) -> HashMap<OsString, OsString> {
        entries
            .into_iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect()
    }

    #[test]
    fn diff_added_only() {
        // Spec scenario: variable added — INCLUDE absent in pre, present
        // in post → goes into `added`.
        let pre = HashMap::new();
        let post = map([("INCLUDE", "/foo")]);
        let d = diff(&pre, &post);
        assert_eq!(d.added.len(), 1);
        assert_eq!(
            d.added.get(&OsString::from("INCLUDE")),
            Some(&OsString::from("/foo"))
        );
        assert!(d.modified.is_empty());
    }

    #[test]
    fn diff_modified_only() {
        // Spec scenario: variable modified — PATH changes from "A;B" to
        // "X;A;B" → goes into `modified` with the post value.
        let pre = map([("PATH", "A;B")]);
        let post = map([("PATH", "X;A;B")]);
        let d = diff(&pre, &post);
        assert!(d.added.is_empty());
        assert_eq!(d.modified.len(), 1);
        assert_eq!(
            d.modified.get(&OsString::from("PATH")),
            Some(&OsString::from("X;A;B"))
        );
    }

    #[test]
    fn diff_unchanged_skipped() {
        // Spec scenario: variable unchanged — USER identical pre/post →
        // not in either bucket.
        let pre = map([("USER", "alice")]);
        let post = map([("USER", "alice")]);
        let d = diff(&pre, &post);
        assert!(d.added.is_empty());
        assert!(d.modified.is_empty());
    }

    #[test]
    fn diff_removed_ignored() {
        // Spec scenario: variable removed in post — OLDVAR present in
        // pre, absent in post → MUST NOT appear in either bucket
        // (removals are explicitly ignored).
        let pre = map([("OLD", "x")]);
        let post = HashMap::new();
        let d = diff(&pre, &post);
        assert!(d.added.is_empty());
        assert!(d.modified.is_empty());
    }

    #[test]
    fn diff_case_insensitive_name_match() {
        // pre uses `Path`; post uses `PATH`. They're the same Windows
        // env var, so the diff must classify this as `modified` (using
        // post's casing), NOT as `added`.
        let pre = map([("Path", "A")]);
        let post = map([("PATH", "X;A")]);
        let d = diff(&pre, &post);
        assert!(
            d.added.is_empty(),
            "case-different name treated as added: {:?}",
            d.added
        );
        assert_eq!(d.modified.len(), 1);
        // The key in the output uses post's casing.
        assert_eq!(
            d.modified.get(&OsString::from("PATH")),
            Some(&OsString::from("X;A"))
        );
        assert!(
            !d.modified.contains_key(&OsString::from("Path")),
            "modified key kept pre's casing"
        );
    }

    #[test]
    fn merged_env_overlays_diff_on_base() {
        // base = pre-style snapshot; merged map must contain the
        // unchanged base entries plus the diff's added + modified
        // entries, with no case-variant duplicates.
        let base = map([("PATH", "A;B"), ("USER", "alice")]);
        let mut d = EnvDiff::default();
        d.added
            .insert(OsString::from("INCLUDE"), OsString::from("/foo"));
        d.modified
            .insert(OsString::from("PATH"), OsString::from("X;A;B"));

        let merged = d.merged_env(&base);

        // Unchanged base entry survives.
        assert_eq!(
            merged.get(&OsString::from("USER")),
            Some(&OsString::from("alice"))
        );
        // Added entry is present.
        assert_eq!(
            merged.get(&OsString::from("INCLUDE")),
            Some(&OsString::from("/foo"))
        );
        // Modified entry overwrites the base value.
        assert_eq!(
            merged.get(&OsString::from("PATH")),
            Some(&OsString::from("X;A;B"))
        );
        // No case-variant duplicate of PATH.
        let path_keys: Vec<&OsString> = merged
            .keys()
            .filter(|k| k.to_string_lossy().eq_ignore_ascii_case("PATH"))
            .collect();
        assert_eq!(
            path_keys.len(),
            1,
            "expected one PATH-like key, got {path_keys:?}"
        );
    }

    #[test]
    fn iter_yields_added_then_modified_each_sorted() {
        // Build an EnvDiff with deliberately out-of-insertion-order
        // keys; iter() must yield added first (sorted), then modified
        // (sorted).
        let mut d = EnvDiff::default();
        d.added.insert(OsString::from("ZULU"), OsString::from("za"));
        d.added
            .insert(OsString::from("ALPHA"), OsString::from("al"));
        d.modified
            .insert(OsString::from("YANKEE"), OsString::from("ya"));
        d.modified
            .insert(OsString::from("BRAVO"), OsString::from("br"));

        let order: Vec<&OsString> = d.iter().map(|(k, _)| k).collect();
        assert_eq!(
            order,
            vec![
                &OsString::from("ALPHA"),
                &OsString::from("ZULU"),
                &OsString::from("BRAVO"),
                &OsString::from("YANKEE"),
            ],
            "expected added (ALPHA, ZULU) then modified (BRAVO, YANKEE)"
        );
    }

    #[test]
    fn merged_env_replaces_case_variant_keys() {
        // Base has `Path` (pre's casing); diff modifies `PATH` (post's
        // casing). Merged must have only `PATH=X;A`, with no leftover
        // `Path` entry that Windows would silently fold into the same
        // var at process-creation time.
        let base = map([("Path", "A")]);
        let mut d = EnvDiff::default();
        d.modified
            .insert(OsString::from("PATH"), OsString::from("X;A"));

        let merged = d.merged_env(&base);

        assert_eq!(
            merged.get(&OsString::from("PATH")),
            Some(&OsString::from("X;A"))
        );
        assert!(
            !merged.contains_key(&OsString::from("Path")),
            "expected `Path` to be removed in favor of `PATH`, got {merged:?}"
        );
        // Sanity: exactly one key, regardless of casing.
        assert_eq!(merged.len(), 1);
    }
}
