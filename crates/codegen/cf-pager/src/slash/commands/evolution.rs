//! `/evolution` — manage skills deployed by the evolution loop (evo-1.5).
//!
//! The evolution deploy gate (`deploy_skill`) intentionally never prompts in
//! YOLO mode (K3 terminal-audit M1: explicit global grant semantics). The
//! user-facing safety net for that choice lives here: every deploy leaves a
//! timestamped backup under `<grok_home>/evolution/backups/`, and this command
//! exposes the post-hoc management surface — list, show, rollback, disable,
//! enable — with zero prompting during normal operation.
//!
//! Design constraints honored:
//! - Disk is the source of truth (no event-log parsing); survives crashes and
//!   machines without a session folder.
//! - Every mutation is itself a rename that preserves the prior state, so any
//!   single step is reversible by a follow-up command.
//! - All mutations stay inside `<grok_home>`; `name` arguments are validated
//!   with the same character class as `deploy_skill`
//!   (`^[a-z0-9][a-z0-9-]{0,63}$`).

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

/// Validate a skill name argument (mirrors `deploy_skill`'s rule).
fn is_valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return false;
    }
    match bytes[0] {
        b'a'..=b'z' | b'0'..=b'9' => {}
        _ => return false,
    }
    bytes[1..]
        .iter()
        .all(|&b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-'))
}

/// `<grok_home>` or an explanatory error (never a cwd-relative fallback).
fn grok_home_root() -> Result<std::path::PathBuf, String> {
    cf_config::user_grok_home()
        .ok_or_else(|| "`grok home` not resolvable (no QIDI_HOME, no home dir)".to_string())
}

fn skills_dir(root: &std::path::Path) -> std::path::PathBuf {
    root.join("skills")
}

fn backups_dir(root: &std::path::Path) -> std::path::PathBuf {
    root.join("evolution").join("backups")
}

fn disabled_dir(root: &std::path::Path) -> std::path::PathBuf {
    root.join("evolution").join("disabled")
}

/// Compact UTC timestamp for rollback-site dirs (YYYYMMDD-HHMMSS).
fn utc_timestamp(now: std::time::SystemTime) -> String {
    let secs = now
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Howard Hinnant's civil_from_days (same algorithm as deploy.rs).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}{month:02}{d:02}-{h:02}{m:02}{s:02}")
}

/// One deployed evolution skill (`skills/user-<name>/`).
struct DeployedSkill {
    name: String,
    path: std::path::PathBuf,
}

fn deployed_skills(root: &std::path::Path) -> Vec<DeployedSkill> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(skills_dir(root)) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(name) = dir_name.strip_prefix("user-") else {
                continue;
            };
            if !path.join("SKILL.md").is_file() {
                continue;
            }
            out.push(DeployedSkill {
                name: name.to_string(),
                path,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Backup dirs for `name` under <grok_home>/evolution/backups/. K3 B1:
/// - <ts>-<name> — bare deploy backup (exactly this skill).
/// - <ts>-<name>-pre-rollback — preserved pre-rollback state (this skill).
/// - <ts>-<name>~<n> — deploy.rs collision suffix since evo-1.5 (the ~ is
///   outside the skill-name alphabet, so it can never be a sibling's name).
/// - <ts>-<name>-<n> — HISTORICAL collision shape (pre-evo-1.5 deploys);
///   ambiguous against a sibling literally named <name>-<n>, so it only
///   counts when no such sibling is known (deployed or disabled). When in
///   doubt, the backup is NOT claimed: better to miss one than mis-restore.
fn backups_for(root: &std::path::Path, name: &str) -> Vec<std::path::PathBuf> {
    let known_siblings: std::collections::BTreeSet<String> = deployed_skills(root)
        .into_iter()
        .map(|d| d.name)
        .chain(
            std::fs::read_dir(disabled_dir(root))
                .into_iter()
                .flatten() // Option<ReadDir> -> ReadDir
                .flatten() // Result<DirEntry> -> DirEntry
                .filter_map(|e| e.file_name().into_string().ok()),
        )
        .collect();
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(backups_dir(root)) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let bytes = dir_name.as_bytes();
            // Real layout (matches deploy.rs `utc_timestamp`): `YYYYMMDD-HHMMSS`
            // is 15 chars with a dash at index 8, then `-<name>` (dash at 15).
            if bytes.len() < 17 || bytes[8] != b'-' || bytes[15] != b'-' {
                continue;
            }
            if !bytes[..8]
                .iter()
                .chain(&bytes[9..15])
                .all(|b| b.is_ascii_digit())
            {
                continue;
            }
            let Some(tail) = dir_name[16..].strip_prefix(name) else {
                continue;
            };
            let matches_name = match tail {
                // Bare backup: exactly <name>.
                "" => true,
                // This skill's pre-rollback site.
                "-pre-rollback" => true,
                // evo-1.5 deploy collision suffix: unambiguous by alphabet.
                t if t.starts_with('~')
                    && t.len() > 1
                    && t[1..].bytes().all(|b| b.is_ascii_digit()) =>
                {
                    true
                }
                // Historical -N: only when <name><tail> is not a known
                // sibling (deployed or disabled). Empty digits (-) never
                // matches (all() on empty is vacuously true — guard len).
                t if t.len() > 1
                    && t.starts_with('-')
                    && t[1..].bytes().all(|b| b.is_ascii_digit()) =>
                {
                    !known_siblings.contains(&format!("{name}{t}"))
                }
                _ => false,
            };
            if matches_name {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn disabled_path(root: &std::path::Path, name: &str) -> std::path::PathBuf {
    disabled_dir(root).join(name)
}

/// Rename rom to 	o, failing closed on any error.
///
/// Windows: rapid successive renames can transiently fail while AV/indexer
/// handles on the just-renamed directory are still open (sharing violations
/// within the same second). Retry with backoff before giving up; the final
/// error stays fail-closed.
fn rename_dir(from: &std::path::Path, to: &std::path::Path) -> Result<(), String> {
    const MAX_ATTEMPTS: usize = 4;
    let mut last_err = String::new();
    for attempt in 0..MAX_ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(100 * attempt as u64));
        }
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = format!(
                    "rename {} -> {} failed: {e}",
                    from.display(),
                    to.display()
                );
            }
        }
    }
    Err(last_err)
}

/// `list` — disk-state summary of deployed / disabled skills and backups.
fn list(root: &std::path::Path) -> String {
    let deployed = deployed_skills(root);
    let mut lines = Vec::new();
    if deployed.is_empty() {
        lines.push("No evolved skills deployed (`skills/user-*` is empty).".to_string());
    } else {
        lines.push(format!("Deployed (skills/user-*): {}", deployed.len()));
        for d in &deployed {
            let file_count = std::fs::read_dir(&d.path)
                .map(|it| it.flatten().count())
                .unwrap_or(0);
            lines.push(format!(
                "  {}  ({} entries)  show: /evolution show {}",
                d.name, file_count, d.name
            ));
        }
    }
    let disabled = std::fs::read_dir(disabled_dir(root))
        .map(|it| it.flatten().filter(|e| e.path().is_dir()).count())
        .unwrap_or(0);
    let backups = std::fs::read_dir(backups_dir(root))
        .map(|it| it.flatten().filter(|e| e.path().is_dir()).count())
        .unwrap_or(0);
    lines.push(format!("Disabled: {disabled}  |  Backups kept: {backups}"));
    lines.push(
        "Usage: /evolution show <name> | rollback <name> | disable <name> | enable <name>"
            .to_string(),
    );
    lines.join("\n")
}

/// `show <name>` — full SKILL.md text in the DocViewer (never a summary).
fn show(root: &std::path::Path, name: &str) -> CommandResult {
    if !is_valid_name(name) {
        return CommandResult::Error(format!("invalid skill name: {name:?}"));
    }
    let deployed_path = skills_dir(root).join(format!("user-{name}"));
    let disabled = disabled_path(root, name);
    let (title, skill_md) = if deployed_path.join("SKILL.md").is_file() {
        (
            format!("evolution skill: {name} (deployed)"),
            deployed_path.join("SKILL.md"),
        )
    } else if disabled.join("SKILL.md").is_file() {
        (
            format!("evolution skill: {name} (disabled)"),
            disabled.join("SKILL.md"),
        )
    } else if let Some(latest) = backups_for(root, name).pop() {
        (
            format!("evolution skill: {name} (backup only)"),
            latest.join("SKILL.md"),
        )
    } else {
        return CommandResult::Error(format!(
            "no deployed, disabled, or backed-up skill named {name:?}"
        ));
    };
    let content = match std::fs::read_to_string(&skill_md) {
        Ok(c) => c,
        Err(e) => {
            return CommandResult::Error(format!("read {}: {e}", skill_md.display()));
        }
    };
    let mut text = content;
    // Companion files (top-level + one nested level, 1MiB/file cap) are
    // appended so "show" is the full story.
    if let Some(parent) = skill_md.parent() {
        append_companions(root, parent, &mut text);
    }
    CommandResult::Action(Action::ShowReleaseNotes { title, content: text })
}

/// Append a skill's companion files to `text` for `show`.
///
/// Walks the top level plus one nested directory level (e.g. `scripts/`),
/// skipping `SKILL.md` itself. Files above 1 MiB are not read; a
/// `[skipped: <name> exceeds 1MiB]` marker is inserted instead. Display names
/// are relative to `root` so nested files stay distinguishable.
fn append_companions(root: &std::path::Path, skill_dir: &std::path::Path, text: &mut String) {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(skill_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.file_name().and_then(|n| n.to_str()) == Some("SKILL.md") {
                continue;
            }
            if path.is_file() {
                files.push(path);
            } else if path.is_dir() {
                if let Ok(nested) = std::fs::read_dir(&path) {
                    for child in nested.flatten() {
                        let nested_path = child.path();
                        if nested_path.is_file() {
                            files.push(nested_path);
                        }
                    }
                }
            }
        }
    }
    files.sort();
    for path in files {
        let name = match path.strip_prefix(root) {
            Ok(rel) => rel.display().to_string(),
            Err(_) => path.display().to_string(),
        };
        let too_large = std::fs::metadata(&path)
            .map(|meta| meta.len() > 1024 * 1024)
            .unwrap_or(false);
        if too_large {
            text.push_str(&format!("\n\n[skipped: {name} exceeds 1MiB]"));
            continue;
        }
        if let Ok(extra) = std::fs::read_to_string(&path) {
            text.push_str(&format!("\n\n--- {name} ---\n{extra}"));
        }
    }
}

/// `rollback <name>` — restore the latest backup; the pre-rollback state is
/// itself preserved as `<ts>-<name>-pre-rollback` so the step is reversible.
/// The site name is collision-probed (8s lookahead) so a same-second second
/// rollback cannot collide with an existing site directory.
fn rollback(root: &std::path::Path, name: &str) -> CommandResult {
    if !is_valid_name(name) {
        return CommandResult::Error(format!("invalid skill name: {name:?}"));
    }
    if disabled_path(root, name).is_dir() {
        return CommandResult::Error(format!(
            "{name:?} is disabled — use `/evolution enable {name}` first"
        ));
    }
    let deployed_path = skills_dir(root).join(format!("user-{name}"));
    let backups = backups_for(root, name);
    if backups.is_empty() {
        return CommandResult::Error(format!(
            "no backups found for {name:?}; nothing to roll back"
        ));
    }
    let Some(latest) = backups.last().cloned() else {
        return CommandResult::Error(format!("no backups found for {name:?}"));
    };
    let mut notes = Vec::new();
    if deployed_path.is_dir() {
        // Probe forward up to 8s for a free site name: a same-second second
        // rollback would otherwise rename onto an existing directory.
        let mut now = std::time::SystemTime::now();
        let site = {
            let mut candidate = None;
            for _ in 0..8 {
                let c = backups_dir(root)
                    .join(format!("{}-{name}-pre-rollback", utc_timestamp(now)));
                if !c.exists() {
                    candidate = Some(c);
                    break;
                }
                now += std::time::Duration::from_secs(1);
            }
            match candidate {
                Some(c) => c,
                None => {
                    return CommandResult::Error(
                        "could not allocate a pre-rollback site (clock collision); retry in a second"
                            .to_string(),
                    )
                }
            }
        };
        if let Err(e) = rename_dir(&deployed_path, &site) {
            return CommandResult::Error(e);
        }
        notes.push(format!("previous state preserved at {}", site.display()));
    }
    if let Err(e) = rename_dir(&latest, &deployed_path) {
        return CommandResult::Error(format!(
            "{e}\n(the preserved pre-rollback state is still in backups/)"
        ));
    }
    notes.push(format!("restored from {}", latest.display()));
    CommandResult::Message(format!("[ok] rolled back `{name}`\n{}", notes.join("\n")))
}

/// `disable <name>` — move out of the skills discovery tree (kill switch).
fn disable(root: &std::path::Path, name: &str) -> CommandResult {
    if !is_valid_name(name) {
        return CommandResult::Error(format!("invalid skill name: {name:?}"));
    }
    let deployed_path = skills_dir(root).join(format!("user-{name}"));
    if !deployed_path.is_dir() {
        return CommandResult::Error(format!(
            "no deployed skill {name:?} (nothing to disable)"
        ));
    }
    let target = disabled_path(root, name);
    if target.exists() {
        return CommandResult::Error(format!(
            "disabled copy of {name:?} already exists at {}",
            target.display()
        ));
    }
    let _ = std::fs::create_dir_all(disabled_dir(root));
    if let Err(e) = rename_dir(&deployed_path, &target) {
        return CommandResult::Error(e);
    }
    CommandResult::Message(format!(
        "[ok] disabled `{name}` (moved to {})\nre-enable with `/evolution enable {name}`",
        target.display()
    ))
}

/// `enable <name>` — move a disabled skill back into discovery.
fn enable(root: &std::path::Path, name: &str) -> CommandResult {
    if !is_valid_name(name) {
        return CommandResult::Error(format!("invalid skill name: {name:?}"));
    }
    let disabled = disabled_path(root, name);
    if !disabled.is_dir() {
        return CommandResult::Error(format!("no disabled skill named {name:?}"));
    }
    let deployed_path = skills_dir(root).join(format!("user-{name}"));
    if deployed_path.exists() {
        return CommandResult::Error(format!(
            "target {} already exists",
            deployed_path.display()
        ));
    }
    let _ = std::fs::create_dir_all(skills_dir(root));
    if let Err(e) = rename_dir(&disabled, &deployed_path) {
        return CommandResult::Error(e);
    }
    CommandResult::Message(format!(
        "[ok] enabled `{name}` (back in skills/user-{name})"
    ))
}

/// `/evolution` — post-hoc management surface for evolution-deployed skills.
pub struct EvolutionCommand;

impl SlashCommand for EvolutionCommand {
    fn name(&self) -> &str {
        "evolution"
    }

    fn description(&self) -> &str {
        "Manage evolution-deployed skills (list/show/rollback/disable/enable)"
    }

    fn usage(&self) -> &str {
        "/evolution [list | show <name> | rollback <name> | disable <name> | enable <name>]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn args_required(&self) -> bool {
        false
    }

    fn arg_placeholder(&self) -> Option<&str> {
        Some("[list | show <name> | rollback <name> | disable <name> | enable <name>]")
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let root = match grok_home_root() {
            Ok(r) => r,
            Err(e) => return CommandResult::Error(e),
        };
        let mut parts = args.split_whitespace();
        let sub = parts.next().unwrap_or("list");
        let name = parts.next();
        match (sub, name) {
            ("list", _) => CommandResult::Message(list(&root)),
            ("show", Some(name)) => show(&root, name),
            ("rollback", Some(name)) => rollback(&root, name),
            ("disable", Some(name)) => disable(&root, name),
            ("enable", Some(name)) => enable(&root, name),
            ("show", None) | ("rollback", None) | ("disable", None) | ("enable", None) => {
                CommandResult::Error(format!(
                    "`/{sub}` requires a skill name. {}",
                    self.usage()
                ))
            }
            (other, _) => CommandResult::Error(format!(
                "unknown subcommand {other:?}. {}",
                self.usage()
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempHome(std::path::PathBuf);
    impl TempHome {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "qidi-evolution-cmd-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
            std::fs::create_dir_all(path.join("skills")).unwrap();
            Self(path)
        }
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Fabricate a deployed skill + one backup, as deploy_skill would leave them.
    fn seed_deployed_with_backup(
        root: &std::path::Path,
        name: &str,
        old_body: &str,
        new_body: &str,
    ) {
        let deployed = skills_dir(root).join(format!("user-{name}"));
        std::fs::create_dir_all(&deployed).unwrap();
        std::fs::write(deployed.join("SKILL.md"), new_body).unwrap();
        let backups = backups_dir(root);
        std::fs::create_dir_all(&backups).unwrap();
        let backup = backups.join(format!("20260101-000000-{name}"));
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(backup.join("SKILL.md"), old_body).unwrap();
    }

    #[test]
    fn name_validation_matches_deploy_rules() {
        assert!(is_valid_name("a"));
        assert!(is_valid_name("user-foo"));
        assert!(is_valid_name("0"));
        assert!(!is_valid_name(""));
        assert!(!is_valid_name("Upper"));
        assert!(!is_valid_name("a/b"));
        assert!(!is_valid_name(".."));
        assert!(!is_valid_name(&"a".repeat(65)));
    }

    #[test]
    fn list_reports_deployed_and_counts() {
        let home = TempHome::new("list");
        seed_deployed_with_backup(home.path(), "demo", "old", "new");
        let out = list(home.path());
        assert!(out.contains("demo"), "{out}");
        assert!(out.contains("Backups kept: 1"), "{out}");
    }

    #[test]
    fn rollback_restores_latest_backup_and_preserves_current() {
        let home = TempHome::new("rb");
        seed_deployed_with_backup(home.path(), "demo", "old body", "new body");
        let res = rollback(home.path(), "demo");
        assert!(matches!(res, CommandResult::Message(_)), "{res:?}");
        let restored = std::fs::read_to_string(
            skills_dir(home.path())
                .join("user-demo")
                .join("SKILL.md"),
        )
        .unwrap();
        assert_eq!(restored, "old body");
        let preserved = std::fs::read_dir(backups_dir(home.path()))
            .unwrap()
            .flatten()
            .find(|e| e.file_name().to_string_lossy().contains("-pre-rollback"))
            .expect("pre-rollback site exists");
        assert_eq!(
            std::fs::read_to_string(preserved.path().join("SKILL.md")).unwrap(),
            "new body"
        );
    }

    #[test]
    fn rollback_without_backup_errors() {
        let home = TempHome::new("rbnone");
        let res = rollback(home.path(), "ghost");
        assert!(matches!(res, CommandResult::Error(_)), "{res:?}");
    }

    #[test]
    fn rollback_hints_enable_for_disabled_skill() {
        let home = TempHome::new("rbdis");
        let dis = disabled_dir(home.path()).join("demo");
        std::fs::create_dir_all(&dis).unwrap();
        std::fs::write(dis.join("SKILL.md"), "x").unwrap();
        match rollback(home.path(), "demo") {
            CommandResult::Error(msg) => assert!(msg.contains("enable"), "{msg}"),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    /// A disabled copy must block rollback unconditionally — even when backups
    /// exist — so a rollback can never resurrect a skill while the disabled
    /// copy stays in place (the "double state" deadlock).
    #[test]
    fn rollback_refuses_when_disabled_even_with_backups() {
        let home = TempHome::new("rbdisbk");
        seed_deployed_with_backup(home.path(), "demo", "old", "new");
        let dis = disabled_dir(home.path()).join("demo");
        std::fs::create_dir_all(&dis).unwrap();
        std::fs::write(dis.join("SKILL.md"), "disabled copy").unwrap();

        match rollback(home.path(), "demo") {
            CommandResult::Error(msg) => assert!(msg.contains("disabled"), "{msg}"),
            other => panic!("expected Error, got {other:?}"),
        }
        // The deployed copy must be untouched (no rename happened).
        let deployed = skills_dir(home.path()).join("user-demo");
        assert!(deployed.is_dir(), "deployed dir must not be moved");
        assert_eq!(
            std::fs::read_to_string(deployed.join("SKILL.md")).unwrap(),
            "new"
        );
    }

    /// Rapid successive rollbacks (which may land in the same wall-clock
    /// second) must all succeed: the site name is probed forward so a rename
    /// never targets an existing directory.
    #[test]
    fn rapid_rollbacks_do_not_collide_on_site_name() {
        let home = TempHome::new("rbrapid");
        seed_deployed_with_backup(home.path(), "demo", "old", "new");
        for _ in 0..3 {
            assert!(
                matches!(rollback(home.path(), "demo"), CommandResult::Message(_)),
                "each rollback must succeed"
            );
        }
    }

    #[test]
    fn disable_enable_roundtrip() {
        let home = TempHome::new("disen");
        seed_deployed_with_backup(home.path(), "demo", "old", "new");
        assert!(matches!(
            disable(home.path(), "demo"),
            CommandResult::Message(_)
        ));
        assert!(!skills_dir(home.path()).join("user-demo").exists());
        assert!(disabled_path(home.path(), "demo").is_dir());
        assert!(matches!(
            enable(home.path(), "demo"),
            CommandResult::Message(_)
        ));
        assert!(skills_dir(home.path()).join("user-demo").is_dir());
    }

    #[test]
    fn disable_conflict_errors_when_disabled_copy_exists() {
        let home = TempHome::new("disconf");
        seed_deployed_with_backup(home.path(), "demo", "old", "new");
        let dis = disabled_dir(home.path()).join("demo");
        std::fs::create_dir_all(&dis).unwrap();
        let res = disable(home.path(), "demo");
        assert!(matches!(res, CommandResult::Error(_)), "{res:?}");
    }

    #[test]
    fn show_rejects_invalid_names_before_fs_access() {
        let home = TempHome::new("showbad");
        let res = show(home.path(), "../evil");
        assert!(matches!(res, CommandResult::Error(_)), "{res:?}");
    }

    #[test]
    fn show_returns_full_skill_md_text() {
        let home = TempHome::new("show");
        seed_deployed_with_backup(home.path(), "demo", "old", "# full text\nbody here");
        match show(home.path(), "demo") {
            CommandResult::Action(Action::ShowReleaseNotes { title, content }) => {
                assert!(title.contains("demo"));
                assert!(content.contains("# full text\nbody here"));
            }
            other => panic!("expected ShowReleaseNotes, got {other:?}"),
        }
    }

    /// `append_companions` walks the top level plus one nested level, and
    /// refuses to read any single file above the 1 MiB cap (marker instead).
    #[test]
    fn append_companions_nested_and_oversized_cap() {
        let home = TempHome::new("showcomp");
        let dir = skills_dir(home.path()).join("user-demo");
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        std::fs::write(dir.join("SKILL.md"), "# skill\n").unwrap();
        std::fs::write(dir.join("README.md"), "readme body").unwrap();
        std::fs::write(dir.join("scripts").join("run.py"), "print('hi')").unwrap();
        std::fs::write(
            dir.join("scripts").join("huge.bin"),
            "x".repeat(1024 * 1024 + 1),
        )
        .unwrap();

        let mut text = String::new();
        append_companions(home.path(), &dir, &mut text);
        // Top-level file is included.
        assert!(text.contains("readme body"), "{text}");
        // One level down is included, with a root-relative display name.
        assert!(text.contains("print('hi')"), "{text}");
        assert!(text.contains("scripts"), "{text}");
        // The oversized file is skipped, not read.
        assert!(text.contains("huge.bin"), "{text}");
        assert!(text.contains("exceeds 1MiB"), "{text}");
        assert!(
            !text.contains(&"x".repeat(64)),
            "oversized body must not be read: {text}"
        );
    }

    #[test]
    fn timestamp_shape_is_compact() {
        assert_eq!(utc_timestamp(std::time::UNIX_EPOCH), "19700101-000000");
        // Same civil-calendar vector as deploy.rs: proves both implementations
        // agree byte-for-byte.
        assert_eq!(
            utc_timestamp(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_704_067_200)),
            "20240101-000000"
        );
    }

    /// `demo` must not inherit backups that belong to the sibling skill
    /// `demo-2` (longer name sharing the prefix) — the high-severity isolation
    /// property of `backups_for`, exercised across every tail shape.
    #[test]
    fn backups_for_excludes_sibling_prefix_skill() {
        let home = TempHome::new("sib");
        // demo-2 is deployed (the seed also leaves its bare backup), so
        // `demo-2` is a known sibling and a `-2` tail must NOT be claimed by
        // `demo`.
        seed_deployed_with_backup(home.path(), "demo", "old", "new");
        seed_deployed_with_backup(home.path(), "demo-2", "old2", "new2");
        let bk = backups_dir(home.path());
        // Extra backups across every tail shape.
        for (dir, body) in [
            ("20260102-000000-demo-1", "historical collision copy"),
            ("20260103-000000-demo-pre-rollback", "pre site"),
            ("20260104-000000-demo~1", "evo-1.5 collision copy"),
            ("20260101-000000-demo-2~1", "demo-2 evo-1.5 collision copy"),
        ] {
            let d = bk.join(dir);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("SKILL.md"), body).unwrap();
        }

        let demo = backups_for(home.path(), "demo");
        let demo_names: Vec<&str> = demo
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        // Exactly demo's four backups; demo-2's bare `-2` backup is excluded
        // because that tail names a known sibling.
        assert_eq!(
            demo_names,
            vec![
                "20260101-000000-demo",
                "20260102-000000-demo-1",
                "20260103-000000-demo-pre-rollback",
                "20260104-000000-demo~1",
            ],
            "{demo_names:?}"
        );
        // Ordering is by full path, so the newest timestamp (the ~1 collision)
        // sorts last.
        assert!(demo_names.last().unwrap().ends_with("demo~1"));

        let demo2 = backups_for(home.path(), "demo-2");
        let demo2_names: Vec<&str> = demo2
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        // demo-2 gets only its own backups (bare + ~1), none of demo's.
        assert_eq!(
            demo2_names,
            vec!["20260101-000000-demo-2", "20260101-000000-demo-2~1"],
            "{demo2_names:?}"
        );
    }

    /// The historical `-N` shape is claimed by the prefix skill only when no
    /// sibling literally named `<name>-N` is known (deployed or disabled): the
    /// "rather miss than mis-restore" exception for orphaned backups.
    #[test]
    fn historical_dash_suffix_claims_orphan() {
        let home = TempHome::new("orphan");
        let bk = backups_dir(home.path());
        for (dir, body) in [
            ("20260101-000000-demo", "bare"),
            ("20260101-000000-demo-2", "orphan"),
        ] {
            let d = bk.join(dir);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("SKILL.md"), body).unwrap();
        }
        // demo-2 is neither deployed nor disabled, so nothing blocks the
        // historical `-2` claim.
        let names: Vec<String> = backups_for(home.path(), "demo")
            .into_iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(names.iter().any(|n| n == "20260101-000000-demo-2"), "{names:?}");
    }
}
