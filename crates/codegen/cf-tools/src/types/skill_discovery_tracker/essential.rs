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

/// Which skill listing a **subagent** session injects at startup (F2).
///
/// Subagent (child) sessions render their baseline skill listing
/// unconditionally, read-only against the cross-session announcement ledger
/// (`SkillManager::announced_names`). The injection point is the spawn path
/// (`spawn_session_actor` -> `inject_subagent_baseline_listing`): a subagent
/// never sends `SessionCommand::Initialize`, so it has no other startup seam.
/// Set from `[skills].subagent_listing` in `config.toml`.
///
/// This is independent of the primary session's [`SkillsMode`]: the primary
/// keeps its own projection, the child has its own knob.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubagentSkillListing {
    /// Inject the essential projection (the resolved essential allowlist —
    /// the user's `[skills].essential_allow`, else
    /// [`DEFAULT_ESSENTIAL_SKILLS`]). This is the default: a child gets the
    /// core programming skills without the full catalog's context tax.
    #[default]
    Essential,
    /// Inject every discovered skill (the full listing).
    Full,
    /// Inject nothing — the child receives no baseline skill listing.
    None,
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
/// - `code-review` is on the allowlist **and model-invocable** as of
///   2026-10-21 (user directive: the strict maintainability review must be
///   available to the model, especially in the main session). Its former
///   `disable-model-invocation: true` was removed, so it now appears in both
///   the model-visible listing and the runtime set; the two projections stay
///   aligned at 29/29.
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
    // ── Model / provider integration ────────────────────────────────
    // 2026-10-22：官方 Agnes 技能（文本/图像/视频/agent 接入 + 错误码表）。
    // 子代理默认只拿 Essential 投影，不加入此表则 skill() 报 not found。
    "agnes-ai-models",
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
    fn subagent_listing_defaults_to_essential() {
        assert_eq!(
            SubagentSkillListing::default(),
            SubagentSkillListing::Essential,
            "an unset subagent_listing must resolve to the Essential projection"
        );
    }

    #[test]
    fn subagent_listing_serde_uses_lowercase_names() {
        assert_eq!(
            serde_json::to_string(&SubagentSkillListing::Essential).unwrap(),
            "\"essential\"",
            "Essential must serialize as the lowercase token `essential`"
        );
        assert_eq!(
            serde_json::to_string(&SubagentSkillListing::Full).unwrap(),
            "\"full\"",
            "Full must serialize as the lowercase token `full`"
        );
        assert_eq!(
            serde_json::to_string(&SubagentSkillListing::None).unwrap(),
            "\"none\"",
            "None must serialize as the lowercase token `none`"
        );
        assert_eq!(
            serde_json::from_str::<SubagentSkillListing>("\"none\"").unwrap(),
            SubagentSkillListing::None,
            "deserializing `none` must yield SubagentSkillListing::None"
        );
        assert_eq!(
            serde_json::from_str::<SubagentSkillListing>("\"full\"").unwrap(),
            SubagentSkillListing::Full,
            "deserializing `full` must yield SubagentSkillListing::Full"
        );
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
