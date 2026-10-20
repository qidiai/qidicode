//! Subagent skill gate (F2S): the default deny-list that keeps a subagent from
//! activating dangerous skills.
//!
//! F1 (commit `362eb07c`) let a subagent activate **every** discovered skill,
//! including the machine-specific dangerous families (image generation, OCR /
//! service bring-up). This module restores a default **permission boundary**: a
//! child session may not activate those skills unless the user explicitly opts
//! in via `[skills].subagent_denied_skills`.
//!
//! This is deliberately **not** the same concern as the essential-skill
//! projection ([`super::essential`]), which is a *context-cost* filter over what
//! the model is told. The gate is a *permission* filter: it is applied to the
//! skill set **before** either subagent projection runs, so it reaches both the
//! child's model-facing listing **and** the runtime set written into
//! `AvailableSkills` (the skill tool's activatable set), under every
//! [`SubagentSkillListing`](super::SubagentSkillListing) value — no listing knob
//! can re-open it. The primary (user) session never consumes this type.

/// Built-in deny-list for subagent skill activation (F2S).
///
/// Each entry denies a skill whose `name` is **equal** to the entry or **starts
/// with** it: `"bid-"` therefore covers the whole `bid-*` family. The name is
/// the frontmatter / command identity — the same identity the essential
/// allowlist and the skill tool use.
///
/// Scope: the machine-specific dangerous families documented in the workspace
/// `AGENTS.md` (§8 skill library, §11.9 GPU services) — GPU / service bring-up,
/// document-OCR pipelines, and outbound-messaging side channels:
/// - `qwen-image-lowvram` — low-VRAM Qwen-Image generation (ComfyUI)
/// - `pdf-to-word-ocr` — HunyuanOCR document pipeline
/// - `seal-extractor` — HunyuanOCR seal / stamp extraction
/// - `bid-` — the `bid-*` office / bid-document parsing family (OCR-based)
/// - `wecom-` — the `wecom-*` enterprise-WeChat bridge family. `wecom-unified`
///   sends messages through the user's own `wecom-cli` credentials, which
///   bypasses the `wecom-bridge` daemon — the only path carrying source
///   validation, human confirmation and an audit log (v3 decision 1). A child
///   that can post to WeChat can also fan conversation content out of the
///   workspace, so the whole family is denied by prefix, exactly like `bid-`.
///
/// A name that is not present on a given machine simply matches nothing: the
/// list is a filter, never a requirement, so an unknown entry is harmless.
pub const DEFAULT_SUBAGENT_DENIED_SKILLS: &[&str] = &[
    "qwen-image-lowvram",
    "pdf-to-word-ocr",
    "seal-extractor",
    "bid-",
    // v3 decision 1: `wecom-unified` is live in the skill library and would let
    // a child post to WeChat with the user's own credentials, bypassing the
    // audited `wecom-bridge` daemon. Prefix form covers `wecom-bridge` too.
    "wecom-",
];

/// The resolved subagent skill deny-list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentSkillDenylist {
    entries: Vec<String>,
}

impl Default for SubagentSkillDenylist {
    /// The built-in [`DEFAULT_SUBAGENT_DENIED_SKILLS`] set — the **safe**
    /// default. A `SkillManager` that is never explicitly configured (e.g. a
    /// child created straight through the registry `finalize()` path) still
    /// gates.
    fn default() -> Self {
        Self {
            entries: DEFAULT_SUBAGENT_DENIED_SKILLS
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
        }
    }
}

impl SubagentSkillDenylist {
    /// A fully **open** gate: nothing is denied.
    ///
    /// This is what `[skills].subagent_denied_skills = []` resolves to, and the
    /// value tests use to isolate a projection from the gate.
    pub fn open() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Resolve the deny-list from `[skills].subagent_denied_skills`.
    ///
    /// - `None` (unset) → [`Self::default`], the built-in dangerous set.
    /// - `Some(entries)` → exactly those entries, replacing the default set.
    ///   `Some(vec![])` opens the gate entirely (the user opting children back
    ///   in to everything).
    pub fn resolve(config: Option<Vec<String>>) -> Self {
        match config {
            Some(entries) => Self::from_entries(entries),
            None => Self::default(),
        }
    }

    /// Build from explicit entries, trimming them and dropping blank ones.
    ///
    /// A stray `""` would otherwise prefix-match **every** name and silently
    /// deny the whole catalog — a foot-gun, not an intent. An empty string is
    /// therefore ignored rather than interpreted.
    fn from_entries(entries: Vec<String>) -> Self {
        Self {
            entries: entries
                .into_iter()
                .map(|entry| entry.trim().to_owned())
                .filter(|entry| !entry.is_empty())
                .collect(),
        }
    }

    /// The resolved entries, in configuration order.
    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    /// `true` when the gate is fully open (`Some(vec![])`, or only blank entries
    /// were configured).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `true` when `name` is denied — equal to an entry, or prefixed by one.
    pub fn denies(&self, name: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| name == entry || name.starts_with(entry.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_set_covers_the_documented_dangerous_families() {
        let gate = SubagentSkillDenylist::default();
        for denied in [
            "qwen-image-lowvram",
            "pdf-to-word-ocr",
            "seal-extractor",
            "bid-write",
            "bid-parse",
            "wecom-unified",
            "wecom-bridge",
        ] {
            assert!(gate.denies(denied), "default gate must deny {denied}");
        }
        for kept in ["check-work", "dev-orchestra", "audit-rubrics", "bidding"] {
            assert!(!gate.denies(kept), "default gate must not deny {kept}");
        }
    }

    /// v3 decision 1: the `wecom-` prefix keeps both the live `wecom-unified`
    /// skill and the future `wecom-bridge` daemon client out of a child's reach
    /// — a subagent must never post to WeChat on the user's credentials, since
    /// that path bypasses the daemon's source validation, confirmation and audit
    /// log. The bare entry is denied too (equality arm), and unrelated names
    /// that merely *contain* `wecom` stay clear: matching is anchored, and the
    /// hyphen is part of the pattern.
    #[test]
    fn wecom_prefix_is_denied_by_the_default_gate() {
        let gate = SubagentSkillDenylist::default();
        for denied in ["wecom-", "wecom-unified", "wecom-bridge", "wecom-anything"] {
            assert!(
                gate.denies(denied),
                "the default `wecom-` entry must deny {denied}"
            );
        }
        for kept in ["wecomxyz", "my-wecom-unified", "audit-rubrics"] {
            assert!(
                !gate.denies(kept),
                "{kept} must not be caught by the `wecom-` prefix"
            );
        }
    }

    #[test]
    fn entry_matches_exactly_or_as_a_prefix_of_the_skill_name() {
        let gate = SubagentSkillDenylist::resolve(Some(vec!["bid-".to_owned()]));
        assert!(gate.denies("bid-write"), "`bid-` must cover bid-write");
        assert!(gate.denies("bid-parse"), "`bid-` must cover bid-parse");
        assert!(
            gate.denies("bid-"),
            "the bare entry name itself matches (equality arm)"
        );
        assert!(
            !gate.denies("bidding"),
            "no `-` delimiter ⇒ no prefix match, so unrelated names stay clear"
        );
        assert!(
            !gate.denies("my-bid-write"),
            "matching is anchored at the start, never a substring search"
        );
        assert!(!gate.denies("check-work"));

        let exact = SubagentSkillDenylist::resolve(Some(vec!["check-work".to_owned()]));
        assert!(exact.denies("check-work"), "an exact entry denies its name");
        assert!(
            exact.denies("check-work-extra"),
            "entries are prefix patterns, not literal-only names"
        );
    }

    #[test]
    fn unset_resolves_to_the_builtin_default_set() {
        let resolved = SubagentSkillDenylist::resolve(None);
        assert_eq!(
            resolved,
            SubagentSkillDenylist::default(),
            "unset config must resolve to the built-in default set"
        );
        let expected: Vec<String> = DEFAULT_SUBAGENT_DENIED_SKILLS
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        assert_eq!(
            resolved.entries(),
            expected.as_slice(),
            "the resolved set must be the documented default set, in order"
        );
    }

    #[test]
    fn explicit_empty_override_opens_the_gate() {
        let gate = SubagentSkillDenylist::resolve(Some(Vec::new()));
        assert!(gate.is_empty(), "an empty override leaves nothing denied");
        assert!(
            !gate.denies("qwen-image-lowvram"),
            "an empty override must re-open the dangerous skills"
        );
        assert_eq!(
            gate,
            SubagentSkillDenylist::open(),
            "`[]` and the explicit `open()` constructor must agree"
        );
    }

    #[test]
    fn explicit_override_replaces_the_default_set() {
        let gate = SubagentSkillDenylist::resolve(Some(vec!["random-tool".to_owned()]));
        assert!(gate.denies("random-tool"));
        assert!(
            !gate.denies("qwen-image-lowvram"),
            "an explicit override replaces the default set instead of extending it"
        );
    }

    #[test]
    fn blank_entries_are_dropped_instead_of_denying_everything() {
        let gate = SubagentSkillDenylist::resolve(Some(vec![
            String::new(),
            "   ".to_owned(),
        ]));
        assert!(gate.is_empty(), "blank entries must not survive resolution");
        assert!(
            !gate.denies("anything"),
            "a blank entry must never match every skill name"
        );
    }

    #[test]
    fn entries_are_trimmed() {
        let gate = SubagentSkillDenylist::resolve(Some(vec!["  bid-  ".to_owned()]));
        assert_eq!(gate.entries(), ["bid-".to_owned()]);
        assert!(gate.denies("bid-write"), "a padded entry must still match");
    }
}
