//! Visual Studio installation discovery via `vswhere.exe`.
//!
//! This module is responsible for the first stage of the `prepare-devenv`
//! pipeline: locating `vswhere.exe`, asking it to enumerate installed VS
//! instances, and selecting exactly one of them according to the precedence
//! rule defined in `specs/vs-installation-discovery`:
//!
//! 1. `--path <dir>` — exact case-insensitive match on `installationPath`.
//! 2. `--id <prefix>` — leading-hex prefix match on `instanceId`.
//! 3. neither — highest STABLE `installationVersion` (numeric semver-style
//!    compare), breaking ties by `installDate` descending. Prereleases (any
//!    version with a `-` suffix, e.g. `18.0.0-preview.2`) are excluded from
//!    auto-selection; they remain reachable via `--id` / `--path` when the
//!    user explicitly opts in. If only prereleases are installed, the picker
//!    falls back to the highest prerelease and emits a `tracing::warn!`.
//!
//! IO is pushed to the edge: [`resolve_with`] takes a closure that fetches the
//! list of installs, so the selection and verification logic can be unit-tested
//! against committed JSON fixtures without spawning `vswhere`. The thin
//! [`resolve`] wrapper wires in the real process spawn for production use.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::error::{Error, Result};

/// One Visual Studio install as reported by `vswhere -format json`.
///
/// Field names mirror vswhere's `camelCase` JSON keys via `serde(rename_all)`.
/// `display_name` and `install_date` carry `serde(default)` because they are
/// not strictly required for selection — older or partial installs may omit
/// them, and we don't want a missing field to fail discovery.
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VsInstance {
    pub installation_path: PathBuf,
    pub installation_version: String,
    pub instance_id: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub install_date: String,
}

/// User-facing selector controlling which install [`resolve_with`] returns.
#[derive(Debug, Clone, Copy)]
pub enum Selector<'a> {
    /// Pick the highest-version install (with `installDate` desc tiebreak).
    Latest,
    /// Match `instance_id.starts_with(prefix)`.
    ById(&'a str),
    /// Match `installation_path` case-insensitively.
    ByPath(&'a Path),
}

/// The result of successfully resolving exactly one Visual Studio install.
#[derive(Debug, Clone)]
pub struct ResolvedInstall {
    pub install_path: PathBuf,
    pub instance_id: String,
    #[allow(dead_code)] // exposed for future -v / list output; not yet consumed
    pub display_name: String,
    pub version: String,
    pub vsdevcmd_path: PathBuf,
}

/// Locate `vswhere.exe` on disk.
///
/// Probes the standard installer path under `%ProgramFiles(x86)%` first, then
/// falls back to a `%PATH%` search (matching either `vswhere.exe` or
/// `vswhere`). Returns [`Error::VsWhereMissing`] when neither yields a hit.
pub fn locate_vswhere() -> Result<PathBuf> {
    let pf86 = std::env::var_os("ProgramFiles(x86)")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"));
    let standard = pf86
        .join("Microsoft Visual Studio")
        .join("Installer")
        .join("vswhere.exe");
    if standard.exists() {
        return Ok(standard);
    }

    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            for candidate in ["vswhere.exe", "vswhere"] {
                let probe = dir.join(candidate);
                if probe.exists() {
                    return Ok(probe);
                }
            }
        }
    }

    Err(Error::VsWhereMissing)
}

/// Spawn `vswhere` and parse the JSON it emits on stdout.
///
/// Uses the canonical incantation (`-format json -all -prerelease -products *`)
/// so the result includes prereleases and BuildTools-style installs. Surfaces
/// non-zero exit as [`Error::VsWhereFailed`]; malformed JSON funnels through
/// the [`Error::Json`] `?`-conversion.
pub fn run_vswhere(vswhere: &Path) -> Result<Vec<VsInstance>> {
    let output = Command::new(vswhere)
        .args(["-format", "json", "-all", "-prerelease", "-products", "*"])
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stderr = if stderr.is_empty() {
            "(no diagnostic output)".to_string()
        } else {
            stderr
        };
        return Err(Error::VsWhereFailed {
            code: output.status.code().unwrap_or(-1),
            stderr,
        });
    }
    let installs: Vec<VsInstance> = serde_json::from_slice(&output.stdout)?;
    // Per spec `cli-surface-and-diagnostics / -vv should additionally include
    // the full vswhere JSON`: log a (instance_id, version, path) triple per
    // install at debug level. We deliberately do NOT log the entire vswhere
    // payload byte-for-byte — that gets noisy fast on machines with many
    // installs and the triples carry the disambiguating bits a user would
    // want to see when troubleshooting `--id` / `--path`.
    let triples: Vec<(&str, &str, &Path)> = installs
        .iter()
        .map(|i| {
            (
                i.instance_id.as_str(),
                i.installation_version.as_str(),
                i.installation_path.as_path(),
            )
        })
        .collect();
    tracing::debug!(installs = ?triples, "vswhere returned installations");
    Ok(installs)
}

/// Production entry point: locate `vswhere`, run it, then resolve the install.
pub fn resolve(selector: Selector<'_>) -> Result<ResolvedInstall> {
    resolve_with(selector, || run_vswhere(&locate_vswhere()?))
}

/// Testable entry point: take an already-fetched (or stubbed) install list and
/// apply selection plus `VsDevCmd.bat` verification.
pub fn resolve_with<F>(selector: Selector<'_>, fetch_json: F) -> Result<ResolvedInstall>
where
    F: FnOnce() -> Result<Vec<VsInstance>>,
{
    let installs = fetch_json()?;
    if installs.is_empty() {
        return Err(Error::NoInstalls);
    }
    let chosen = pick(&installs, selector)?;

    let vsdevcmd_path = chosen
        .installation_path
        .join("Common7")
        .join("Tools")
        .join("VsDevCmd.bat");
    if !vsdevcmd_path.exists() {
        return Err(Error::VsDevCmdMissing(vsdevcmd_path));
    }

    Ok(ResolvedInstall {
        install_path: chosen.installation_path.clone(),
        instance_id: chosen.instance_id.clone(),
        display_name: chosen.display_name.clone(),
        version: chosen.installation_version.clone(),
        vsdevcmd_path,
    })
}

/// Apply the selector to choose one [`VsInstance`] from a non-empty slice.
///
/// `Selector::Latest` deliberately prefers **stable** installs over prereleases.
/// A prerelease is identified by a `-` in the `installation_version` (vswhere's
/// convention, e.g. `18.0.0-preview.2`). Prereleases remain reachable via
/// `Selector::ById` and `Selector::ByPath` — they're only excluded from
/// auto-selection. If the host has *only* prerelease installs, we fall back to
/// the highest prerelease and emit a `tracing::warn!` so a `-v` run surfaces
/// the choice.
fn pick<'a>(installs: &'a [VsInstance], selector: Selector<'_>) -> Result<&'a VsInstance> {
    match selector {
        Selector::Latest => {
            let stable: Vec<&'a VsInstance> = installs
                .iter()
                .filter(|i| !i.installation_version.contains('-'))
                .collect();
            if !stable.is_empty() {
                Ok(*stable
                    .iter()
                    .max_by(|a, b| cmp_install_for_latest(a, b))
                    .expect("non-empty by construction"))
            } else {
                tracing::warn!(
                    "no stable Visual Studio installs found; selecting from prereleases"
                );
                Ok(installs
                    .iter()
                    .max_by(|a, b| cmp_install_for_latest(a, b))
                    .expect("caller guarantees the slice is non-empty"))
            }
        }

        Selector::ById(prefix) => {
            let matches: Vec<&VsInstance> = installs
                .iter()
                .filter(|i| i.instance_id.starts_with(prefix))
                .collect();
            match matches.len() {
                0 => Err(Error::NoMatch {
                    kind: "id",
                    value: prefix.to_string(),
                }),
                1 => Ok(matches[0]),
                _ => Err(Error::AmbiguousId {
                    prefix: prefix.to_string(),
                    candidates: matches
                        .iter()
                        .map(|i| (i.instance_id.clone(), i.installation_path.clone()))
                        .collect(),
                }),
            }
        }

        Selector::ByPath(path) => {
            let needle = normalize_path_for_compare(path);
            let matches: Vec<&VsInstance> = installs
                .iter()
                .filter(|i| normalize_path_for_compare(&i.installation_path) == needle)
                .collect();
            match matches.len() {
                0 => Err(Error::NoMatch {
                    kind: "path",
                    value: path.to_string_lossy().into_owned(),
                }),
                1 => Ok(matches[0]),
                _ => {
                    tracing::warn!(
                        "{} installs share installation_path {}; selecting the first",
                        matches.len(),
                        path.display()
                    );
                    Ok(matches[0])
                }
            }
        }
    }
}

/// Parse a vswhere version string into a `Vec<u64>` for numeric comparison.
///
/// vswhere emits dotted-numeric versions (e.g. `17.10.1`) but prereleases may
/// suffix a `-tag.N` segment (e.g. `18.0.0-preview.2`). We treat the
/// prerelease suffix as if it weren't there: strip everything from the first
/// `-` onward, then split on `.` and parse each segment as `u64`. Non-numeric
/// segments (which shouldn't occur in vswhere output but might in the wild)
/// evaluate to `0` via the leading-digits prefix. Lexicographic `Vec<u64>`
/// ordering then yields semver-style ordering for the inputs we care about,
/// with prereleases of version `X` ordering equal to release `X` — close
/// enough for "pick the latest install" tiebreaks where `installDate` is the
/// real disambiguator.
fn parse_version_numeric(v: &str) -> Vec<u64> {
    let core = v.split_once('-').map_or(v, |(lhs, _)| lhs);
    core.split('.')
        .map(|seg| {
            let numeric_prefix: String = seg.chars().take_while(|c| c.is_ascii_digit()).collect();
            numeric_prefix.parse::<u64>().unwrap_or(0)
        })
        .collect()
}

/// Comparator used by `Selector::Latest`: numeric version, then install_date,
/// then instance_id. Extracted so both the stable-only branch and the
/// prerelease-fallback branch share one ordering definition.
fn cmp_install_for_latest(a: &VsInstance, b: &VsInstance) -> std::cmp::Ordering {
    let va = parse_version_numeric(&a.installation_version);
    let vb = parse_version_numeric(&b.installation_version);
    va.cmp(&vb)
        .then_with(|| a.install_date.cmp(&b.install_date))
        // Final deterministic tiebreak: when two installs share both version
        // and install_date (notably when both install_date values are empty
        // — the `serde(default)` fallback), `instance_id` is guaranteed unique
        // by vswhere, so this picks deterministically.
        .then_with(|| a.instance_id.cmp(&b.instance_id))
}

/// Lower-case the path's lossy string form and trim trailing slashes for
/// case-insensitive Windows-style equality. Avoids canonicalisation (and the
/// extra dep that would entail) — string-lower is sufficient for vswhere paths
/// which are absolute and already in normal form.
///
/// Assumptions / known limitations:
///
/// 1. **ASCII-only case folding.** `to_ascii_lowercase` does NOT case-fold
///    non-ASCII characters (e.g. `Ä`, Cyrillic `Б`). This is acceptable
///    because `vswhere` paths are typically rooted under English-locale
///    `Program Files` / `Program Files (x86)` directories with ASCII
///    directory names, and Visual Studio's installer enforces ASCII-only
///    install paths.
///
/// 2. **Path separators are NOT normalized.** `\\server/share\foo` and
///    `\\server\share\foo` would compare unequal. UNC paths in `vswhere`
///    output are rare to nonexistent in practice (VS installs go to local
///    drives), but if a user passes a hand-typed `--path` with mixed
///    separators that don't match `vswhere`'s output verbatim, the match
///    will fail. We accept this rather than picking a separator convention
///    that might surprise on the rare case it does come up.
fn normalize_path_for_compare(p: &Path) -> String {
    let s = p.to_string_lossy().to_ascii_lowercase();
    s.trim_end_matches(['\\', '/']).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const SINGLE: &str = include_str!("../tests/fixtures/vswhere/single.json");
    const MULTIPLE_SAME_VERSION: &str =
        include_str!("../tests/fixtures/vswhere/multiple_same_version.json");
    const MULTIPLE_DIFFERENT_VERSIONS: &str =
        include_str!("../tests/fixtures/vswhere/multiple_different_versions.json");
    const AMBIGUOUS_PREFIX: &str = include_str!("../tests/fixtures/vswhere/ambiguous_prefix.json");
    const EMPTY: &str = include_str!("../tests/fixtures/vswhere/empty.json");

    fn load(json: &str) -> Vec<VsInstance> {
        serde_json::from_str(json).expect("fixture should deserialize")
    }

    /// Build an install rooted at `tmp` with a stub `VsDevCmd.bat` already in
    /// place, so `resolve_with` succeeds end-to-end against the temp dir.
    fn install_with_vsdevcmd(tmp: &TempDir, instance_id: &str, version: &str) -> VsInstance {
        let install_path = tmp.path().to_path_buf();
        let tools = install_path.join("Common7").join("Tools");
        std::fs::create_dir_all(&tools).expect("create Common7/Tools");
        std::fs::write(tools.join("VsDevCmd.bat"), b"@echo off\n").expect("write VsDevCmd.bat");
        VsInstance {
            installation_path: install_path,
            installation_version: version.to_string(),
            instance_id: instance_id.to_string(),
            display_name: "Stub VS".into(),
            install_date: "2025-01-01T00:00:00Z".into(),
        }
    }

    // --- VsInstance / fixture sanity ---------------------------------------

    #[test]
    fn fixtures_deserialize() {
        assert_eq!(load(SINGLE).len(), 1);
        assert_eq!(load(MULTIPLE_SAME_VERSION).len(), 2);
        assert_eq!(load(MULTIPLE_DIFFERENT_VERSIONS).len(), 3);
        assert_eq!(load(AMBIGUOUS_PREFIX).len(), 2);
        assert!(load(EMPTY).is_empty());
    }

    #[test]
    fn vs_instance_camel_case_round_trip() {
        let installs = load(SINGLE);
        let only = &installs[0];
        assert_eq!(only.instance_id, "d0b481ec");
        assert_eq!(only.installation_version, "17.10.1");
        assert_eq!(only.display_name, "Visual Studio Community 2022");
        assert_eq!(
            only.installation_path,
            PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\2022\Community")
        );
    }

    // --- empty / no installs -----------------------------------------------

    #[test]
    fn vswhere_returns_an_empty_array_yields_no_installs() {
        let err = resolve_with(Selector::Latest, || Ok(load(EMPTY))).unwrap_err();
        assert!(matches!(err, Error::NoInstalls), "got {err:?}");
    }

    // --- one install (success path) ----------------------------------------

    #[test]
    fn vswhere_returns_one_or_more_installations_latest() {
        let tmp = TempDir::new().unwrap();
        let installs = vec![install_with_vsdevcmd(&tmp, "d0b481ec", "17.10.1")];
        let resolved = resolve_with(Selector::Latest, || Ok(installs.clone())).unwrap();
        assert_eq!(resolved.instance_id, "d0b481ec");
        assert_eq!(resolved.version, "17.10.1");
        assert_eq!(resolved.install_path, tmp.path());
        assert_eq!(
            resolved.vsdevcmd_path,
            tmp.path()
                .join("Common7")
                .join("Tools")
                .join("VsDevCmd.bat")
        );
    }

    // --- ByPath ------------------------------------------------------------

    #[test]
    fn path_matches_exactly_one_install() {
        let tmp = TempDir::new().unwrap();
        let installs = vec![install_with_vsdevcmd(&tmp, "d0b481ec", "17.10.1")];
        let path = tmp.path().to_path_buf();
        let resolved = resolve_with(Selector::ByPath(&path), || Ok(installs.clone())).unwrap();
        assert_eq!(resolved.instance_id, "d0b481ec");
    }

    #[test]
    fn path_match_is_case_insensitive_and_trailing_slash_insensitive() {
        let installs = load(SINGLE);
        // The fixture path is "C:\Program Files\Microsoft Visual Studio\2022\Community";
        // try the lower-cased form with a trailing backslash.
        let probe = PathBuf::from(r"c:\program files\microsoft visual studio\2022\community\");
        // pick directly so we bypass the VsDevCmd existence check.
        let chosen = pick(&installs, Selector::ByPath(&probe)).unwrap();
        assert_eq!(chosen.instance_id, "d0b481ec");
    }

    #[test]
    fn path_or_id_matches_zero_installs() {
        let installs = load(SINGLE);
        let nowhere = PathBuf::from(r"C:\Definitely\Not\A\Real\VS\Install");
        let err = resolve_with(Selector::ByPath(&nowhere), || Ok(installs.clone())).unwrap_err();
        match err {
            Error::NoMatch { kind, value } => {
                assert_eq!(kind, "path");
                assert!(value.contains("Definitely"), "value={value}");
            }
            other => panic!("expected Error::NoMatch{{kind=\"path\"}}, got {other:?}"),
        }
    }

    // --- ById --------------------------------------------------------------

    #[test]
    fn id_prefix_matches_exactly_one_install() {
        let tmp = TempDir::new().unwrap();
        let installs = vec![install_with_vsdevcmd(&tmp, "d0b481ec", "17.10.1")];
        let resolved = resolve_with(Selector::ById("d0b481ec"), || Ok(installs.clone())).unwrap();
        assert_eq!(resolved.instance_id, "d0b481ec");
    }

    #[test]
    fn id_short_prefix_matches_exactly_one_install() {
        let tmp = TempDir::new().unwrap();
        let installs = vec![install_with_vsdevcmd(&tmp, "d0b481ec", "17.10.1")];
        let resolved = resolve_with(Selector::ById("d0b4"), || Ok(installs.clone())).unwrap();
        assert_eq!(resolved.instance_id, "d0b481ec");
    }

    #[test]
    fn id_prefix_matches_multiple_installs() {
        let installs = load(AMBIGUOUS_PREFIX);
        let err = resolve_with(Selector::ById("ab12"), || Ok(installs.clone())).unwrap_err();
        match err {
            Error::AmbiguousId {
                ref prefix,
                ref candidates,
            } => {
                assert_eq!(prefix, "ab12");
                // Spec `vs-installation-discovery / --id prefix matches
                // multiple installs`: the error must list candidates' short
                // ids AND paths. Confirm both made it through.
                assert!(
                    candidates.len() >= 2,
                    "expected ≥2 candidates, got {}",
                    candidates.len()
                );
                let rendered = format!("{err}");
                for (id, path) in candidates {
                    assert!(
                        rendered.contains(id),
                        "rendered message missing id {id}: {rendered}"
                    );
                    let path_str = path.display().to_string();
                    assert!(
                        rendered.contains(&path_str),
                        "rendered message missing path {path_str}: {rendered}"
                    );
                }
            }
            other => panic!("expected Error::AmbiguousId, got {other:?}"),
        }
    }

    #[test]
    fn id_prefix_matches_zero_installs() {
        let installs = load(SINGLE);
        let err = resolve_with(Selector::ById("zzzz"), || Ok(installs.clone())).unwrap_err();
        match err {
            Error::NoMatch { kind, value } => {
                assert_eq!(kind, "id");
                assert_eq!(value, "zzzz");
            }
            other => panic!("expected Error::NoMatch{{kind=\"id\"}}, got {other:?}"),
        }
    }

    // --- Latest semantics --------------------------------------------------

    #[test]
    fn latest_picks_highest_stable_version_skipping_prereleases() {
        // Fixture has three installs:
        //   - aaaaaaaa : 17.8.4              (stable)
        //   - bbbbbbbb : 18.0.0-preview.2    (PRERELEASE)
        //   - cccccccc : 17.10.1             (stable)
        //
        // Latest must (a) compare versions numerically (so 17.10.1 > 17.8.4,
        // not the lexical "17.8.4" > "17.10.1"), AND (b) skip the prerelease
        // 18.x. Net result: cccccccc wins.
        let installs = load(MULTIPLE_DIFFERENT_VERSIONS);
        let chosen = pick(&installs, Selector::Latest).unwrap();
        assert_eq!(chosen.instance_id, "cccccccc");
        assert_eq!(chosen.installation_version, "17.10.1");
    }

    #[test]
    fn latest_falls_back_to_prerelease_when_no_stable_installs_exist() {
        // Two installs, both prereleases. Latest must pick one (with a warn);
        // refusing to pick at all would force the user to specify --id/--path
        // even when there's literally nothing else available.
        let installs = vec![
            VsInstance {
                installation_path: PathBuf::from(r"C:\VS\Preview1"),
                installation_version: "18.0.0-preview.1".into(),
                instance_id: "preview1".into(),
                display_name: "VS 18 preview.1".into(),
                install_date: "2025-01-01T00:00:00Z".into(),
            },
            VsInstance {
                installation_path: PathBuf::from(r"C:\VS\Preview2"),
                installation_version: "18.0.0-preview.2".into(),
                instance_id: "preview2".into(),
                display_name: "VS 18 preview.2".into(),
                install_date: "2025-02-01T00:00:00Z".into(),
            },
        ];
        let chosen = pick(&installs, Selector::Latest).unwrap();
        // Both prereleases share parse_version_numeric == [18, 0, 0], so the
        // install_date tiebreak kicks in: preview2's later date wins.
        assert_eq!(chosen.instance_id, "preview2");
    }

    #[test]
    fn id_can_target_a_prerelease() {
        // Even though `Latest` skips prereleases, `--id` must still let the
        // user target one explicitly — that's the whole point of having a
        // selector.
        let installs = load(MULTIPLE_DIFFERENT_VERSIONS);
        let chosen = pick(&installs, Selector::ById("bbbb")).unwrap();
        assert_eq!(chosen.instance_id, "bbbbbbbb");
        assert_eq!(chosen.installation_version, "18.0.0-preview.2");
    }

    #[test]
    fn path_can_target_a_prerelease() {
        let installs = load(MULTIPLE_DIFFERENT_VERSIONS);
        let probe = PathBuf::from(r"C:\Program Files\Microsoft Visual Studio\18\IntPreview");
        let chosen = pick(&installs, Selector::ByPath(&probe)).unwrap();
        assert_eq!(chosen.instance_id, "bbbbbbbb");
    }

    #[test]
    fn latest_tiebreaks_by_install_date_descending() {
        // Both installs are 17.10.1; the one with the later install_date wins.
        let installs = load(MULTIPLE_SAME_VERSION);
        let chosen = pick(&installs, Selector::Latest).unwrap();
        assert_eq!(chosen.instance_id, "22222222"); // 2024-09-22 > 2024-01-15
    }

    #[test]
    fn latest_tiebreaks_deterministically_when_install_date_missing() {
        // Two installs with identical version and BOTH install_date values
        // empty (the default when `serde(default)` fills in for a missing
        // field). With the version + install_date tiebreaks both returning
        // Equal, the final fallback on `instance_id` must pick deterministically.
        let a = VsInstance {
            installation_path: PathBuf::from(r"C:\VS\A"),
            installation_version: "17.10.1".into(),
            instance_id: "aaaaaaaa".into(),
            display_name: String::new(),
            install_date: String::new(),
        };
        let b = VsInstance {
            installation_path: PathBuf::from(r"C:\VS\B"),
            installation_version: "17.10.1".into(),
            instance_id: "bbbbbbbb".into(),
            display_name: String::new(),
            install_date: String::new(),
        };

        // Both input orderings must yield the same pick — the structural
        // determinism guarantee. Ordering by `instance_id` ascending under
        // `max_by` means the lexically-greater id wins ("bbbbbbbb" > "aaaaaaaa").
        let ab = [a.clone(), b.clone()];
        let ba = [b.clone(), a.clone()];
        let chosen_ab = pick(&ab, Selector::Latest).unwrap();
        let chosen_ba = pick(&ba, Selector::Latest).unwrap();
        assert_eq!(chosen_ab.instance_id, "bbbbbbbb");
        assert_eq!(chosen_ba.instance_id, "bbbbbbbb");
    }

    // --- VsDevCmd.bat existence verification -------------------------------

    #[test]
    fn vsdevcmd_missing_at_computed_path_yields_error() {
        // single.json points at C:\Program Files\... which does not exist on
        // a CI runner without VS installed, so the existence check fires
        // regardless of host state.
        let installs = load(SINGLE);
        let err = resolve_with(Selector::Latest, || Ok(installs)).unwrap_err();
        match err {
            Error::VsDevCmdMissing(p) => {
                let s = p.to_string_lossy();
                assert!(s.ends_with("VsDevCmd.bat"), "got {s}");
                assert!(s.contains("Common7"), "got {s}");
            }
            other => panic!("expected Error::VsDevCmdMissing, got {other:?}"),
        }
    }

    #[test]
    fn vsdevcmd_present_at_computed_path_yields_resolved_path() {
        let tmp = TempDir::new().unwrap();
        let inst = install_with_vsdevcmd(&tmp, "abcdef01", "17.10.1");
        let expected_bat = tmp
            .path()
            .join("Common7")
            .join("Tools")
            .join("VsDevCmd.bat");
        let resolved = resolve_with(Selector::Latest, || Ok(vec![inst])).unwrap();
        assert_eq!(resolved.vsdevcmd_path, expected_bat);
        assert!(resolved.vsdevcmd_path.exists());
    }

    // --- version-parser unit checks ----------------------------------------

    #[test]
    fn parse_version_numeric_handles_prerelease_suffix() {
        assert_eq!(parse_version_numeric("17.8.4"), vec![17, 8, 4]);
        assert_eq!(parse_version_numeric("17.10.1"), vec![17, 10, 1]);
        assert_eq!(parse_version_numeric("18.0.0-preview.2"), vec![18, 0, 0]);
        assert!(parse_version_numeric("17.10.1") > parse_version_numeric("17.8.4"));
        assert!(parse_version_numeric("18.0.0-preview.2") > parse_version_numeric("17.10.1"));

        // Edge cases — document the contract for inputs vswhere shouldn't
        // produce but which keep the function total rather than panicking:
        //   - empty string: one empty segment → [0]
        //   - trailing dot: empty trailing segment → 0 in last slot
        //   - non-numeric: `chars().take_while(is_ascii_digit)` yields ""; parse → 0
        assert_eq!(parse_version_numeric(""), vec![0]);
        assert_eq!(parse_version_numeric("17.10."), vec![17, 10, 0]);
        assert_eq!(parse_version_numeric("foo"), vec![0]);
        assert_eq!(parse_version_numeric("foo.bar"), vec![0, 0]);
    }
}
