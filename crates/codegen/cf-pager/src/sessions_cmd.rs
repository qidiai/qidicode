use anyhow::Result;
use clap::Subcommand;
use cf_shell::agent::config::Config as AgentConfig;
use cf_shell::auth::{AuthManager, try_ensure_fresh_auth};
use cf_shell::session::merge::MergedSession;
use cf_shell::util::grok_home::grok_home;
#[derive(Debug, clap::Args, Clone)]
pub struct SessionsArgs {
    #[command(subcommand)]
    command: SessionsCommand,
}

#[derive(Debug, Subcommand, Clone)]
enum SessionsCommand {
    /// List recent sessions (same as search with no query)
    List {
        /// Maximum number of sessions to show
        #[arg(short = 'n', long, default_value = "20")]
        limit: usize,
    },
    /// Search sessions by keyword
    Search {
        /// Search query (searches summaries and first prompts).
        query: String,
        /// Maximum number of sessions to show
        #[arg(short = 'n', long, default_value = "20")]
        limit: usize,
    },
    /// Permanently delete a session from history
    Delete {
        /// Session id to delete.
        id: String,
    },
    /// Hide task/dispatch sessions from history browsing. Hidden sessions stay
    /// searchable and can still be resumed by id.
    Hide {
        /// Session ids to hide.
        ids: Vec<String>,
        /// Scan the current workspace for sessions created by non-interactive
        /// (headless) runs — i.e. dispatched tasks — and hide them.
        #[arg(long)]
        headless: bool,
        /// With `--headless`, scan every workspace under `~/.qidi/sessions/`
        /// instead of just the current one.
        #[arg(long)]
        all_workspaces: bool,
        /// Print what would be hidden without writing anything.
        #[arg(long)]
        dry_run: bool,
        /// With explicit ids, overwrite a session that already has a
        /// non-subagent kind (e.g. `worktree`) instead of refusing.
        #[arg(long)]
        force: bool,
    },
}

pub async fn run(args: SessionsArgs, agent_config: &AgentConfig) -> Result<()> {
    // Best-effort only. Do not force an interactive public login for enterprise
    // deployments that only configure a deployment_key + custom cf_api_base_url.
    // If the user has previously run the interactive `grok` TUI (which succeeds
    // for these setups), any cached credential will be used. Otherwise we still
    // proceed so the SessionRegistryClient can use the deployment_key when
    // talking to the custom proxy.
    let auth = try_ensure_fresh_auth(&agent_config.grok_com_config).await;

    let auth_manager = std::sync::Arc::new(AuthManager::new(
        &grok_home(),
        agent_config.grok_com_config.clone(),
    ));

    let client = cf_shell::agent::session_registry_client::SessionRegistryClient::new(
        agent_config.endpoints.proxy_url(),
        String::new(),
    )
    .with_deployment_key(agent_config.endpoints.deployment_key.clone())
    .with_alpha_test_key(agent_config.endpoints.alpha_test_key.clone())
    .with_auth(auth_manager.clone());

    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());

    match args.command {
        SessionsCommand::List { limit } => {
            let sessions = cf_shell::session::merge::fetch_merged(
                Some(&client),
                cwd.to_str(),
                None,
                limit,
            )
            .await;
            print_sessions_grouped(&sessions);
        }
        SessionsCommand::Search { query, limit } => {
            use std::collections::HashSet;
            use cf_shell::session::merge::REMOTE_TIMEOUT;
            use cf_shell::session::storage::search::{SessionSearchRequest, execute_search};

            let req = SessionSearchRequest {
                query,
                cwd: Some(cwd.to_string_lossy().to_string()),
                limit,
                offset: 0,
                include_content: true,
            };
            let root = grok_home();

            let remote_limit = (limit * 3).max(100) as i64;
            let (local_resp, remote_results) = tokio::join!(execute_search(&root, &req), async {
                tokio::time::timeout(
                    REMOTE_TIMEOUT,
                    client.search(Some(&req.query), remote_limit),
                )
                .await
                .unwrap_or_else(|_| {
                    eprintln!(
                        "warning: remote session search timed out, showing local results only"
                    );
                    Ok(Vec::new())
                })
                .unwrap_or_else(|e| {
                    eprintln!("warning: remote session search failed: {e}");
                    Vec::new()
                })
            });

            let resp = local_resp?;
            let local_ids: HashSet<&str> =
                resp.results.iter().map(|r| r.session_id.as_str()).collect();

            for hit in &resp.results {
                let title = if hit.title.is_empty() {
                    "(untitled)"
                } else {
                    &hit.title
                };
                let time = chrono::DateTime::from_timestamp(hit.updated_at_unix, 0)
                    .map(|dt| {
                        dt.with_timezone(&chrono::Local)
                            .format("%b %d, %l:%M%P")
                            .to_string()
                    })
                    .unwrap_or_default();
                println!(
                    "{} (score: {:.2})  {}\n  {}\n  {}",
                    hit.session_id,
                    hit.score,
                    time,
                    title,
                    hit.snippet.as_deref().unwrap_or("")
                );
            }

            let remaining = limit.saturating_sub(resp.results.len());
            let mut remote_shown = 0usize;
            for r in &remote_results {
                if remote_shown >= remaining {
                    break;
                }
                if local_ids.contains(r.session_id.as_str()) {
                    continue;
                }
                let title = if r.summary.is_empty() {
                    "(untitled)"
                } else {
                    &r.summary
                };
                let time = chrono::DateTime::parse_from_rfc3339(&r.updated_at)
                    .map(|dt| {
                        dt.with_timezone(&chrono::Local)
                            .format("%b %d, %l:%M%P")
                            .to_string()
                    })
                    .unwrap_or_default();
                let snippet: String = r
                    .first_prompt
                    .as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(80)
                    .collect();
                println!(
                    "{} (remote)  {}\n  {}\n  {}",
                    r.session_id, time, title, snippet
                );
                remote_shown += 1;
            }

            println!("\nTotal: {}", resp.results.len() + remote_shown);
        }
        SessionsCommand::Delete { id } => {
            // Always attempt the remote delete when authenticated and not
            // ZDR — `list` / `search` likewise query remote unconditionally
            // rather than gating on storage mode (which the CLI cannot
            // resolve here: it builds config without remote settings). The
            // backend delete is idempotent (a `404` is treated as success),
            // so this is safe for local-only sessions with no remote copy.
            // ZDR teams never upload, so there is nothing remote to delete.
            let needs_remote = auth.as_ref().is_some_and(|a| !a.is_zdr_team());

            // Pass `cwd = None` so the session is found by id regardless of
            // which workspace it was created in; the local delete still uses
            // the resolved per-session cwd.
            let deletion = cf_shell::session::persistence::delete_session_history(
                &id,
                None,
                needs_remote,
                auth_manager.clone(),
            )
            .await?;

            if deletion.any_removed() {
                println!("Deleted session {id}");
            } else {
                println!("No session found with id {id}.");
            }
        }
        SessionsCommand::Hide {
            ids,
            headless,
            all_workspaces,
            dry_run,
            force,
        } => {
            if !headless && ids.is_empty() {
                anyhow::bail!(
                    "pass at least one session id, or --headless to scan this workspace"
                );
            }

            let mut targets: Vec<(String, String)> = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for id in &ids {
                match cf_shell::session::persistence::find_summary_by_session_id(id) {
                    Some(summary) => {
                        // Explicit ids must not silently clobber a meaningful
                        // existing kind (e.g. worktree/fork). `--force` opts in.
                        if !may_overwrite_kind(summary.session_kind.as_deref(), force) {
                            eprintln!(
                                "warning: session {id} has kind {:?}; refusing to overwrite \
                                 (pass --force to hide anyway)",
                                summary.session_kind
                            );
                            continue;
                        }
                        if seen.insert(id.clone()) {
                            targets.push((id.clone(), summary.display_title().to_string()));
                        }
                    }
                    None => eprintln!("warning: no local session with id {id}"),
                }
            }
            if headless {
                let cwd_str = cwd.to_string_lossy().to_string();
                for dir in session_scan_dirs(&cwd_str, all_workspaces) {
                    for (id, title) in headless_sessions_in(&dir) {
                        if seen.insert(id.clone()) {
                            targets.push((id, title));
                        }
                    }
                }
            }

            if targets.is_empty() {
                println!("Nothing to hide.");
                return Ok(());
            }
            if dry_run {
                for (id, title) in &targets {
                    println!("would hide {id}  {title}");
                }
                println!("Would hide {} session(s).", targets.len());
                return Ok(());
            }

            let mut hidden = 0usize;
            for (id, _) in &targets {
                match cf_shell::session::persistence::set_session_kind(
                    id,
                    HIDDEN_TASK_SESSION_KIND,
                ) {
                    Ok(_) => hidden += 1,
                    Err(e) => eprintln!("warning: could not hide {id}: {e}"),
                }
            }
            println!(
                "Hidden {hidden} session(s) as {HIDDEN_TASK_SESSION_KIND} \
                 (removed from history browsing; still searchable and resumable by id)."
            );
        }
    }

    Ok(())
}

/// Kind stamped on dispatched/headless task sessions. It starts with
/// `subagent`, so [`cf_shell::session::persistence::Summary::is_hidden`] keeps
/// these sessions out of history browsing while the picker's search still
/// finds them.
const HIDDEN_TASK_SESSION_KIND: &str = "subagent_headless";

/// Marker only the non-interactive (headless) system prompt carries. Used to
/// recognise task sessions created before headless runs were auto-classified.
const HEADLESS_PROMPT_MARKER: &str =
    "an autonomous agent that completes software engineering tasks";

fn is_headless_prompt(system_prompt: &str) -> bool {
    system_prompt.contains(HEADLESS_PROMPT_MARKER)
}

/// Whether an explicit-id hide may overwrite the session's current kind.
/// `subagent*` (already hidden) and kindless sessions are safe; anything else
/// (`worktree`, `fork`, …) needs `--force` so a task-hiding command cannot
/// silently erase a meaningful classification.
fn may_overwrite_kind(existing: Option<&str>, force: bool) -> bool {
    force
        || existing
            .map(|kind| kind.starts_with("subagent"))
            .unwrap_or(true)
}

/// Workspace session directories to scan for pre-classification task sessions.
fn session_scan_dirs(cwd: &str, all_workspaces: bool) -> Vec<std::path::PathBuf> {
    if !all_workspaces {
        return vec![cf_shell::util::grok_home::sessions_cwd_dir(cwd)];
    }
    let root = cf_shell::util::grok_home::grok_home().join("sessions");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            eprintln!("warning: cannot scan {}: {e}", root.display());
            return Vec::new();
        }
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect()
}

/// Session ids (and titles) under `cwd_sessions_dir` that look like unclassified
/// headless/dispatch task sessions: no `session_kind`, not explicitly hidden,
/// and carrying the non-interactive system prompt.
fn headless_sessions_in(cwd_sessions_dir: &std::path::Path) -> Vec<(String, String)> {
    let entries = match std::fs::read_dir(cwd_sessions_dir) {
        Ok(entries) => entries,
        // A workspace with no sessions is normal, not an error.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            eprintln!(
                "warning: cannot scan {}: {e}",
                cwd_sessions_dir.display()
            );
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let Ok(bytes) = std::fs::read(dir.join("summary.json")) else {
            continue;
        };
        let Ok(summary) = serde_json::from_slice::<cf_shell::session::persistence::Summary>(&bytes)
        else {
            continue;
        };
        // Already classified (subagent/fork/worktree) or explicitly hidden.
        if summary.session_kind.is_some() || summary.is_hidden() {
            continue;
        }
        let Ok(prompt) = std::fs::read_to_string(dir.join("system_prompt.txt")) else {
            continue;
        };
        if !is_headless_prompt(&prompt) {
            continue;
        }
        out.push((
            summary.info.id.0.to_string(),
            summary.display_title().to_string(),
        ));
    }
    out.sort();
    out.dedup();
    out
}

/// Print sessions grouped by worktree label, preserving the original table
/// format with a `Label: <label>` header before each group.
fn print_sessions_grouped(sessions: &[MergedSession]) {
    if sessions.is_empty() {
        println!("No sessions found.");
        return;
    }

    // Group by worktree_label, sort alphabetically, None last.
    let mut groups: std::collections::BTreeMap<Option<&str>, Vec<&MergedSession>> =
        std::collections::BTreeMap::new();
    for s in sessions {
        groups
            .entry(s.worktree_label.as_deref())
            .or_default()
            .push(s);
    }

    let header = format!(
        "{:<36}  {:<10}  {:<10}  {:<10}  {}",
        "SESSION ID", "CREATED", "UPDATED", "STATUS", "SUMMARY"
    );

    // Labeled groups first (alphabetical), then unlabeled last.
    let none_group = groups.remove(&None);
    let print_group = |label_line: &str, members: &[&MergedSession]| {
        println!("\n{label_line}");
        println!("{header}");
        for s in members {
            let first_line;
            let summary: &str = if !s.summary.is_empty() {
                &s.summary
            } else if let Some(ref fp) = s.first_prompt
                && let Some(line) = fp.lines().find(|l| !l.trim().is_empty())
            {
                first_line = line.trim().to_string();
                &first_line
            } else {
                "(no summary)"
            };
            let truncated: String = summary.chars().take(50).collect();
            let created = &s.created_at[..s.created_at.len().min(10)];
            let updated = &s.updated_at[..s.updated_at.len().min(10)];
            println!(
                "{}  {}  {}  {}  {}",
                s.session_id, created, updated, s.source, truncated
            );
        }
    };

    for (label, members) in &groups {
        let line = format!("Label: {}", label.unwrap_or(""));
        print_group(&line, members);
    }
    if let Some(members) = &none_group {
        print_group("(no label)", members);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_session(
        root: &std::path::Path,
        id: &str,
        kind: Option<&str>,
        hidden: Option<bool>,
        prompt: &str,
    ) {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        let mut summary = serde_json::json!({
            "info": { "id": id, "cwd": "/test" },
            "session_summary": id,
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "num_messages": 0,
            "current_model_id": "test-model"
        });
        if let Some(kind) = kind {
            summary["session_kind"] = serde_json::Value::String(kind.to_string());
        }
        if let Some(hidden) = hidden {
            summary["hidden"] = serde_json::Value::Bool(hidden);
        }
        std::fs::write(dir.join("summary.json"), summary.to_string()).unwrap();
        std::fs::write(dir.join("system_prompt.txt"), prompt).unwrap();
    }

    #[test]
    fn recognizes_only_headless_system_prompt() {
        assert!(is_headless_prompt(
            "You are QIDI Code. You are an autonomous agent that completes software engineering tasks."
        ));
        assert!(!is_headless_prompt(
            "You are QIDI Code. You are an interactive CLI tool that helps users with software engineering tasks."
        ));
    }

    #[test]
    fn explicit_hide_refuses_to_clobber_non_subagent_kinds() {
        assert!(may_overwrite_kind(None, false), "kindless is safe");
        assert!(
            may_overwrite_kind(Some("subagent"), false),
            "already hidden is safe"
        );
        assert!(
            may_overwrite_kind(Some("subagent_headless"), false),
            "already a task kind is safe"
        );
        assert!(
            !may_overwrite_kind(Some("worktree"), false),
            "worktree must need --force"
        );
        assert!(
            !may_overwrite_kind(Some("fork"), false),
            "fork must need --force"
        );
        assert!(
            may_overwrite_kind(Some("worktree"), true),
            "--force allows overwrite"
        );
    }

    /// The retroactive scan must only touch task sessions created before
    /// classification existed: user sessions, already-classified sessions and
    /// explicitly hidden sessions are left alone.
    #[test]
    fn scan_selects_only_unclassified_headless_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let headless_prompt =
            "You are QIDI Code. You are an autonomous agent that completes software engineering tasks.";
        let interactive_prompt =
            "You are QIDI Code. You are an interactive CLI tool that helps users.";

        write_session(root, "task", None, None, headless_prompt);
        write_session(root, "chat", None, None, interactive_prompt);
        write_session(root, "already-kind", Some("subagent"), None, headless_prompt);
        write_session(root, "already-hidden", None, Some(true), headless_prompt);

        let ids: Vec<String> = headless_sessions_in(root)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(ids, vec!["task"]);
    }
}
