//! Essential-skill listing projection (P0-3 phase 1).
//!
//! The startup skill listing is the single largest fixed per-session token
//! cost: a fresh session injects the full description of every discovered
//! skill (name + description + absolute path). `Essential` mode projects only
//! a small allowlist of core programming skills into the model-facing
//! listing, cutting that cost to near zero.
//!
//! The projection is **listing-only**: it filters what the model is *told*
//! about. Dynamic discovery, slash invocation, and the skill tool remain
//! unrestricted — an excluded skill is still invocable by name or by reading
//! its `SKILL.md`. Projection is a context-cost concern, not a permission
//! boundary.

/// Which skills the model-facing listing advertises.
///
/// Set from `[skills].mode` in `config.toml`. `Essential` (the default)
/// keeps a fresh session cheap; `Full` restores the historical
/// "list every discovered skill" behavior byte-for-byte.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillsMode {
    /// Project only [`DEFAULT_ESSENTIAL_SKILLS`] — or the user's
    /// `[skills].essential_allow` override — into the listing.
    #[default]
    Essential,
    /// List every discovered skill (historical behavior; byte-identical to
    /// the pre-projection renderer).
    Full,
}

/// Built-in allowlist projected in [`SkillsMode::Essential`].
///
/// Entries are skill `name`s (the frontmatter name / command identity). A
/// name that is not present on a given machine simply matches nothing: the
/// list is a projection filter, never a hard requirement, so an unknown
/// entry is harmless.
///
/// Scope discipline (user directive: "only load the most core programming
/// aspect"): office (`bid-*`), NPU-adaptation, and model-deploy skills are
/// deliberately excluded. Only core programming skills stay resident.
///
/// Two entries look surprising but are intentional:
/// - `code-review` stays on the allowlist even though it never announces: its
///   bundled `SKILL.md` carries `disable-model-invocation: true`, so the
///   listing projection (`listing.rs`) filters it out of the model-visible
///   list by design (user-only, via `/code-review`).
/// - Headless-only built-ins (e.g. `best-of-n`) are never listed here: it is a
///   cf-shell compiled-in `SKILL.md` (`BEST_OF_N_SKILL_MD`, "not extracted as
///   a bundled skill"), and legacy cleanup deletes stale user copies on disk,
///   so the discoverer can never see it — such an entry would be permanently
///   dead.
pub const DEFAULT_ESSENTIAL_SKILLS: &[&str] = &[
    // ── Programming flow ────────────────────────────────────────────
    "check-work",
    "auto-bug-fixer",
    "python-refactoring",
    "generate-unit-test",
    "pytest-writer",
    "unittest-writer",
    "coverage",
    "analyse-coverage",
    "code-comprehension",
    "repo-reader",
    "auto-develop-test-gen",
    "code-review",
    // ── Security ────────────────────────────────────────────────────
    "secret-scan",
    "pre-commit-secret-check",
    // ── Orchestration ───────────────────────────────────────────────
    "dev-orchestra",
    "find-skills",
    // ── Audit / review ──────────────────────────────────────────────
    "audit-rubrics",
    "skill-auditor",
    // ── Docs / git workflow ─────────────────────────────────────────
    "gitcode-publish",
    "git-commit-author-fix",
    "gitcode-repo-bootstrap",
    "git-cmd-channel-protocol",
    // ── Skill / tooling meta ────────────────────────────────────────
    "create-skill",
    "plugin-creator",
    "install-skill-dependency",
    "help",
    // ── Guidance / environment ──────────────────────────────────────
    "qoderwork-guidance",
    "vm-error-recovery",
    "computer-use-guidance-windows",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn essential_is_the_default_mode() {
        assert_eq!(SkillsMode::default(), SkillsMode::Essential);
    }

    #[test]
    fn default_allowlist_has_no_duplicates() {
        let mut seen = HashSet::new();
        for name in DEFAULT_ESSENTIAL_SKILLS {
            assert!(
                seen.insert(*name),
                "duplicate allowlist entry: {name}"
            );
        }
    }

    #[test]
    fn default_allowlist_excludes_headless_only_builtins() {
        // `best-of-n` is a compiled-in headless-only skill (cf-shell
        // `BEST_OF_N_SKILL_MD`, "not extracted as a bundled skill"); legacy
        // cleanup deletes user copies on startup, so discovery can never see
        // it — a whitelist entry here would be permanently dead.
        assert!(!DEFAULT_ESSENTIAL_SKILLS.contains(&"best-of-n"));
    }

    #[test]
    fn default_allowlist_excludes_office_and_npu_skills() {
        // Guard the scope discipline: no `bid-*` office skills and no
        // NPU-adaptation / model-deploy families may creep in.
        for name in DEFAULT_ESSENTIAL_SKILLS {
            assert!(
                !name.starts_with("bid-"),
                "office skill must not be essential: {name}"
            );
            assert!(
                !name.contains("-npu") && !name.starts_with("npu-"),
                "NPU skill must not be essential: {name}"
            );
        }
    }
}
