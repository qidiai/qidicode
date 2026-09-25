//! Evolution Phase 1 (Blocker B1): a synthetic permission gate that forces an
//! `Ask` for `Edit`-class tool calls targeting the user skills directory
//! (`~/.qidi/skills/**`).
//!
//! ## Why this layer
//!
//! The rule is **synthetic**: it is never written into the user's config and is
//! never persisted. It is injected at the single choke point every session
//! funnels through — the permission manager, when it compiles the session's
//! [`CompiledPolicy`] — so it applies however the session's
//! `PermissionConfig` was obtained (disk resolution, CLI flags, subagents,
//! tests). Injecting in the resolution layer would only cover the
//! disk-resolution path and would be bypassed by any caller that passes an
//! explicit config.
//!
//! ## Interaction with YOLO (always-approve)
//!
//! The gate is an `Ask` rule, so it follows the semantics of every explicit
//! `Ask` rule rather than acting as a hard floor. **YOLO mode is the user's
//! explicit, global self-grant**: under it an `Ask` gate — this synthetic one
//! included — is auto-approved, consistent with the existing explicit-`Ask`
//! behavior. A user's explicit `Deny` still wins unconditionally in every mode.
//! YOLO direct writes into the skills tree are therefore the user's own accepted
//! risk, not a gate bypass; in the **default** mode the gate blocks them as
//! intended. (Red-team evaluation: K3 final review, M1.)
//!
//! Evaluation is order-independent with `deny > ask > allow`, so a user's
//! explicit `deny` rule on the same path still wins over the injected `ask`
//! (and an `allow` never does).
//!
//! The rules are passed to [`CompiledPolicy::with_synthetic_rules`] rather than
//! appended to the config, so activating the gate does **not** flip the shell
//! file-access scanner on for sessions that had no file rules of their own.
//!
//! [`CompiledPolicy`]: crate::permission::CompiledPolicy
//! [`CompiledPolicy::with_synthetic_rules`]: crate::permission::CompiledPolicy::with_synthetic_rules

use crate::permission::types::{PatternMode, PermissionRule, RuleAction, ToolFilter};

/// Environment variable that switches the evolution feature on/off.
///
/// The feature is **enabled by default**: only the exact value `off`
/// (case-insensitive) disables it.
pub const EVOLUTION_ENV_VAR: &str = "QIDI_EVOLUTION";

/// Glob for the user skills directory as the model usually spells it (a
/// literal `~`, never expanded). Matched against the raw `Edit` path.
pub const TILDE_SKILLS_GLOB: &str = "~/.qidi/skills/**";

/// True when the evolution feature is enabled (the default).
pub fn evolution_enabled() -> bool {
    evolution_enabled_for(std::env::var(EVOLUTION_ENV_VAR).ok().as_deref())
}

/// Pure form of [`evolution_enabled`] over an explicit env value, so the
/// on/off decision is testable without mutating the process environment.
///
/// `None` (unset) and every value except `off` enable the feature.
pub fn evolution_enabled_for(value: Option<&str>) -> bool {
    !matches!(value, Some(v) if v.eq_ignore_ascii_case("off"))
}

/// The synthetic gate rules, unconditionally (deterministic test seam).
///
/// Two `Edit` → `Ask` rules are produced so both spellings of the skills
/// directory are covered:
///
/// 1. the literal `~/.qidi/skills/**` form the model typically emits, and
/// 2. the absolute `<grok_home>/skills/**` form (covers `$QIDI_HOME`
///    overrides and absolute paths), when a user home resolves.
///
/// Both are `Ask` on `ToolFilter::Edit`, which governs every edit-class tool
/// (`search_replace`, `write`, `hashline_edit`, `apply_patch`) because they all
/// map to `AccessKind::Edit`.
pub(crate) fn skill_deploy_gate_rules() -> Vec<PermissionRule> {
    let mut rules = vec![ask_edit(TILDE_SKILLS_GLOB)];

    if let Some(home) = cf_config::user_grok_home() {
        let absolute = format!("{}/**", forward_slashed(&home.join("skills")));
        if absolute != TILDE_SKILLS_GLOB {
            rules.push(ask_edit(&absolute));
        }
    }

    rules
}

/// The gate rules to install into a session, honoring [`evolution_enabled`].
///
/// Returns an empty vector when the feature is disabled, so the caller's policy
/// behaves exactly as if nothing had been injected.
pub fn session_skill_gate_rules() -> Vec<PermissionRule> {
    if evolution_enabled() {
        skill_deploy_gate_rules()
    } else {
        Vec::new()
    }
}

fn ask_edit(pattern: &str) -> PermissionRule {
    PermissionRule {
        action: RuleAction::Ask,
        tool: ToolFilter::Edit,
        pattern: Some(pattern.to_string()),
        pattern_mode: PatternMode::Glob,
    }
}

/// Normalize a path to forward slashes so it can be used as a glob pattern on
/// every platform (the permission matcher compares against `/`-separated text).
fn forward_slashed(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permission::CompiledPolicy;
    use crate::permission::types::{AccessKind, Decision, PermissionConfig};

    fn gate_policy(rules: Vec<PermissionRule>) -> CompiledPolicy {
        // `with_synthetic_rules` is exactly what the permission manager calls;
        // using it here keeps the test on the production path (no env read).
        CompiledPolicy::with_synthetic_rules(PermissionConfig::new(rules), skill_deploy_gate_rules())
    }

    #[test]
    fn evolution_enabled_unless_off() {
        assert!(evolution_enabled_for(None));
        assert!(evolution_enabled_for(Some("on")));
        assert!(evolution_enabled_for(Some("")));
        assert!(!evolution_enabled_for(Some("off")));
        assert!(!evolution_enabled_for(Some("OFF")));
        assert!(!evolution_enabled_for(Some("Off")));
    }

    #[test]
    fn gate_asks_on_edit_of_literal_tilde_skill_path() {
        let policy = gate_policy(Vec::new());

        for path in [
            "~/.qidi/skills/x/SKILL.md",
            "~/.qidi/skills/user-foo/SKILL.md",
            "~/.qidi/skills/user-foo/scripts/run.py",
        ] {
            assert_eq!(
                policy.evaluate(&AccessKind::Edit(path.to_string())),
                Some(Decision::Ask),
                "edit of {path} must be forced to Ask"
            );
        }
    }

    #[test]
    fn gate_leaves_unrelated_access_untouched() {
        let policy = gate_policy(Vec::new());

        assert!(
            policy
                .evaluate(&AccessKind::Edit("src/main.rs".to_string()))
                .is_none(),
            "an unrelated edit must not match the gate"
        );
        assert!(
            policy
                .evaluate(&AccessKind::Read(Some(
                    "~/.qidi/skills/x/SKILL.md".to_string()
                )))
                .is_none(),
            "the gate targets Edit only, not Read"
        );
        assert!(
            policy
                .evaluate(&AccessKind::Bash("ls".to_string()))
                .is_none(),
            "the gate must not affect Bash"
        );
    }

    #[test]
    fn explicit_deny_still_wins_over_injected_ask() {
        let deny = PermissionRule {
            action: RuleAction::Deny,
            tool: ToolFilter::Edit,
            pattern: Some(TILDE_SKILLS_GLOB.to_string()),
            pattern_mode: PatternMode::Glob,
        };
        let policy = gate_policy(vec![deny]);

        assert!(
            matches!(
                policy.evaluate(&AccessKind::Edit(
                    "~/.qidi/skills/x/SKILL.md".to_string()
                )),
                Some(Decision::Reject(_))
            ),
            "a user deny on the same path must outrank the injected ask"
        );
    }

    #[test]
    fn explicit_allow_does_not_override_injected_ask() {
        let allow = PermissionRule {
            action: RuleAction::Allow,
            tool: ToolFilter::Edit,
            pattern: Some("~/.qidi/skills/**".to_string()),
            pattern_mode: PatternMode::Glob,
        };
        let policy = gate_policy(vec![allow]);

        assert_eq!(
            policy.evaluate(&AccessKind::Edit(
                "~/.qidi/skills/x/SKILL.md".to_string()
            )),
            Some(Decision::Ask),
            "ask must outrank allow regardless of rule order"
        );
    }

    #[test]
    fn gate_rules_do_not_flip_shell_file_restrictions() {
        // The synthetic rules must not switch on the shell file-access scanner
        // (which would introduce spurious prompts for ambiguous shell operands
        // in sessions that had no file rules).
        let policy = gate_policy(Vec::new());
        assert!(!policy.has_file_restrictions);
    }

    /// K3 final review M1: pins the YOLO semantics of the gate at the rule layer.
    ///
    /// The gate is an `Ask` rule (never a hard `Deny`), which is exactly why YOLO
    /// — the user's explicit global self-grant — auto-approves it uniformly with
    /// every other explicit `Ask`. An explicit `Deny` on the same path still wins
    /// in every mode. Actor-level YOLO auto-approval is exercised end to end by
    /// the permission-manager tests (the H1 session-grant regression plus the
    /// existing YOLO auto-approve tests); this test guards the rule-layer
    /// invariants that keep that behavior intentional.
    #[test]
    fn gate_is_ask_so_yolo_applies_and_explicit_deny_still_wins() {
        // The gate must be `Ask`: a `Deny` here would (wrongly) make YOLO
        // irrelevant and would also block the sanctioned deploy path.
        for rule in skill_deploy_gate_rules() {
            assert_eq!(
                rule.action,
                RuleAction::Ask,
                "the skill-deploy gate must be an Ask rule, never a Deny"
            );
        }

        // An explicit user deny outranks the gate's ask in every mode.
        let deny = PermissionRule {
            action: RuleAction::Deny,
            tool: ToolFilter::Edit,
            pattern: Some(TILDE_SKILLS_GLOB.to_string()),
            pattern_mode: PatternMode::Glob,
        };
        let policy = gate_policy(vec![deny]);
        assert!(
            matches!(
                policy.evaluate(&AccessKind::Edit(
                    "~/.qidi/skills/user-foo/SKILL.md".to_string()
                )),
                Some(Decision::Reject(_))
            ),
            "an explicit deny must still win over the gate's ask"
        );
    }
}
