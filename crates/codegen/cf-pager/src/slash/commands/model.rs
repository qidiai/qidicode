//! `/model` (alias `/m`) — switch model + (optionally) reasoning effort.
//! Chained autocomplete: pick a reasoning-supported model → trailing space
//! re-opens the dropdown into a `low|medium|high|xhigh` sub-menu.

use agent_client_protocol as acp;
use cf_shell::sampling::types::supports_reasoning_effort_meta;
use indexmap::IndexMap;

use crate::acp::model_state::ModelState;
use crate::app::actions::Action;
use crate::slash::command::{AppCtx, ArgItem, CommandExecCtx, CommandResult, SlashCommand};
use crate::slash::commands::effort_levels::build_effort_arg_items;

/// Switch the active model (and optionally its reasoning effort).
pub struct ModelCommand;

impl SlashCommand for ModelCommand {
    fn name(&self) -> &str {
        "model"
    }

    fn aliases(&self) -> &[&str] {
        &["m"]
    }

    fn description(&self) -> &str {
        "Switch the active model"
    }

    fn session_scoped(&self) -> bool {
        true
    }

    fn offered_when_session_less(&self) -> bool {
        // The dashboard offers `/model` to pick the model for the next
        // spawned agent (intercepted in `dispatch_dashboard_dispatch_slash`).
        true
    }

    fn usage(&self) -> &str {
        "/model <name> [effort]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn args_required(&self) -> bool {
        true
    }

    fn arg_placeholder(&self) -> Option<&str> {
        Some("<model> [effort]")
    }

    fn suggest_args(&self, ctx: &AppCtx, args_query: &str) -> Option<Vec<ArgItem>> {
        if ctx.models.is_empty() {
            return None;
        }

        // Effort phase if input is "<reasoning-model> ", else model phase.
        if let Some(model_id) = detect_effort_phase(ctx.models, args_query) {
            return Some(build_effort_items(ctx.models, &model_id));
        }

        // Source-group phase: the query targets a folded group (e.g. after
        // accepting a "<group> N models" row, whose insert_text is
        // "<group> ") - expand into that group's model rows.
        if let Some(items) = detect_group_phase_items(ctx.models, args_query) {
            return Some(items);
        }
        Some(build_model_items(ctx.models))
    }

    fn run(&self, ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let trimmed = args.trim();
        if trimmed.is_empty() {
            return CommandResult::Error("Usage: /model <name> [effort]".into());
        }

        // Prefer an exact full-string catalog match first. Model display names
        // often contain spaces ("Grok 4.5"); if we split on the last token
        // first, a shorter catalog entry ("Grok") would steal the prefix and
        // treat "4.5" as an effort level.
        if let Some(id) = ctx.models.resolve_by_name_or_id(trimmed) {
            return CommandResult::Action(Action::SetDefaultModel(id));
        }

        // Trailing effort token + reasoning model → session-scoped switch
        // (not persisted as default). Resolve via the shared gate so a rejected
        // level (e.g. `none` on grok-4.5) surfaces the effort error with the
        // model's offered ids — not "Unknown model: … none".
        if let Some((prefix, token)) = split_trailing_token(trimmed)
            && let Some(id) = resolve_model(ctx.models, prefix)
            && ctx
                .models
                .available
                .get(&id)
                .map(supports_reasoning_effort)
                .unwrap_or(false)
        {
            return match ctx.models.resolve_effort_for_model(&id, token) {
                Ok(effort) => CommandResult::Action(Action::SwitchModel {
                    model_id: id,
                    effort: Some(effort),
                }),
                Err(err) => CommandResult::Error(err.message()),
            };
        }

        // A source-group label typed directly isn't a model - nudge into
        // the picker's group expansion instead of a confusing Unknown error.
        if is_group_phase(ctx.models, trimmed) {
            return CommandResult::Error(format!(
                "'{trimmed}' is a model source group - expand it in the picker and pick a model"
            ));
        }

        CommandResult::Error(format!("Unknown model: {trimmed}"))
    }
}

/// Look up a model by case-insensitive display name OR model id match.
fn resolve_model(models: &ModelState, name: &str) -> Option<acp::ModelId> {
    models.resolve_by_name_or_id(name)
}

fn supports_reasoning_effort(info: &acp::ModelInfo) -> bool {
    supports_reasoning_effort_meta(info.meta.as_ref())
}

/// Split `args` into `(prefix, last_token)` on the final whitespace run.
/// Returns `None` when there is no interior whitespace to split on. The token is
/// resolved to an effort against the picked model's options by the caller.
fn split_trailing_token(args: &str) -> Option<(&str, &str)> {
    let (prefix, last) = args.rsplit_once(char::is_whitespace)?;
    let prefix = prefix.trim_end();
    if prefix.is_empty() || last.is_empty() {
        return None;
    }
    Some((prefix, last))
}

/// Returns the matched model id when `args_query` is `"<reasoning-model> ..."`.
/// Longest-name-first to disambiguate names that share a prefix.
fn detect_effort_phase(models: &ModelState, args_query: &str) -> Option<acp::ModelId> {
    let mut candidates: Vec<(&acp::ModelId, &str)> = models
        .available
        .iter()
        .filter(|(_, info)| supports_reasoning_effort(info))
        .map(|(id, info)| (id, info.name.as_str()))
        .collect();
    candidates.sort_by_key(|(_, name)| std::cmp::Reverse(name.len()));

    for (id, name) in candidates {
        if args_query.len() > name.len()
            && args_query.is_char_boundary(name.len())
            && args_query[..name.len()].eq_ignore_ascii_case(name)
            && args_query[name.len()..].starts_with(char::is_whitespace)
        {
            return Some(id.clone());
        }
    }
    None
}

/// Read the source-group tag from a model's ACP metadata. The tag is set
/// from `group = "..."` in a `[model.<id>]` config entry (relayed through
/// the `group` key in the model's ACP meta) and folds the model into a
/// single expandable group row in the `/model` picker.
fn group_of(info: &acp::ModelInfo) -> Option<String> {
    info.meta
        .as_ref()?
        .get("group")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// One selectable model row (shared by the flat list and expanded groups).
/// Reasoning models get a trailing space in `insert_text` so the prompt
/// widget chains into the effort sub-menu.
fn model_row(
    id: &acp::ModelId,
    info: &acp::ModelInfo,
    current_id: Option<&acp::ModelId>,
) -> ArgItem {
    let is_current = current_id == Some(id);
    let supports = supports_reasoning_effort(info);

    let display = if is_current {
        format!("{} (current)", info.name)
    } else {
        info.name.clone()
    };

    // Trailing space on reasoning models: signals "more input
    // expected" to the prompt widget so Enter advances to effort
    // phase instead of submitting.
    let insert_text = if supports {
        format!("{} ", info.name)
    } else {
        info.name.clone()
    };

    ArgItem {
        display,
        match_text: info.name.clone(),
        insert_text,
        description: info.description.clone().unwrap_or_default(),
    }
}

/// One row per logical model, with source-grouped models folded into
/// collapsed group rows. Ungrouped models keep catalog order first; group
/// rows follow in first-appearance order, so a source contributing many
/// models (e.g. a 30-channel relay) tucks away behind one row. A group
/// row's `insert_text` ends with a space so every picker surface (prompt
/// dropdown, ArgPicker modal, dashboard dispatch) chains into the group's
/// expanded model list.
fn build_model_items(models: &ModelState) -> Vec<ArgItem> {
    let current_id = models.current.as_ref();
    let mut items: Vec<ArgItem> = Vec::with_capacity(models.available.len());
    // Label -> (member display names, any-member-is-current), first-appearance order.
    let mut groups: IndexMap<String, (Vec<String>, bool)> = IndexMap::new();
    for (id, info) in &models.available {
        if let Some(label) = group_of(info) {
            let entry = groups.entry(label).or_insert_with(|| (Vec::new(), false));
            entry.0.push(info.name.clone());
            entry.1 |= current_id == Some(id);
            continue;
        }
        items.push(model_row(id, info, current_id));
    }
    for (label, (names, has_current)) in groups {
        // match_text carries the label plus every member name so the group
        // row still surfaces when the user types any member fragment while
        // the group is collapsed.
        let mut match_text = format!("{label} group");
        for name in &names {
            match_text.push(' ');
            match_text.push_str(name);
        }
        let count = if names.len() == 1 {
            "1 model".to_string()
        } else {
            format!("{} models", names.len())
        };
        items.push(ArgItem {
            display: format!(
                "{label} \u{25B8} {count}{}",
                if has_current { " (current)" } else { "" },
            ),
            match_text,
            // Trailing space chains the picker into the group's model list.
            insert_text: format!("{label} "),
            description: "Source group - Enter to expand".to_string(),
        });
    }
    items
}

/// Expanded group rows: one plain row per member of `label`. Member
/// `match_text` gets a `<sort-key> <label> ` prefix so the controller's
/// fuzzy rank matches the full `<label> <fragment>` query and the
/// alphabetical tiebreak preserves catalog order.
fn group_member_items(models: &ModelState, label: &str) -> Vec<ArgItem> {
    let current_id = models.current.as_ref();
    let lower_label = label.to_ascii_lowercase();
    let mut items: Vec<ArgItem> = Vec::new();
    for (id, info) in models.available.iter() {
        let in_group = group_of(info)
            .map(|g| g.eq_ignore_ascii_case(&lower_label))
            .unwrap_or(false);
        if !in_group {
            continue;
        }
        let mut item = model_row(id, info, current_id);
        let sort_prefix = char::from(b'a' + items.len().min(25) as u8);
        item.match_text = format!("{sort_prefix} {label} {}", item.match_text);
        items.push(item);
    }
    items
}

/// Distinct group labels in first-appearance order, longest first (so a
/// label that prefixes another wins prefix disambiguation).
fn group_labels(models: &ModelState) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    for info in models.available.values() {
        if let Some(label) = group_of(info)
            && !labels.iter().any(|l| l.eq_ignore_ascii_case(&label))
        {
            labels.push(label);
        }
    }
    labels.sort_by_key(|l| std::cmp::Reverse(l.len()));
    labels
}

/// The group label an args query targets: the query equals the label or
/// starts with `<label> ` (case-insensitive). `None` = not in a group phase.
fn group_phase_label(models: &ModelState, args_query: &str) -> Option<String> {
    let trimmed = args_query.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    group_labels(models).into_iter().find(|label| {
        let ll = label.to_ascii_lowercase();
        lower == ll || lower.starts_with(&format!("{ll} "))
    })
}

/// Whether the args query targets a folded source group. Exported for the
/// ArgPicker chain guard (a group phase always keeps the picker open) and
/// `/model` run (a bare label isn't a model).
pub(crate) fn is_group_phase(models: &ModelState, args_query: &str) -> bool {
    group_phase_label(models, args_query).is_some()
}

/// Expanded member rows for the current group phase, or `None` when the
/// query doesn't target a group (or the group has no members).
fn detect_group_phase_items(models: &ModelState, args_query: &str) -> Option<Vec<ArgItem>> {
    let label = group_phase_label(models, args_query)?;
    let items = group_member_items(models, &label);
    if items.is_empty() { None } else { Some(items) }
}

/// Title for the ArgPicker modal at the current `/model` phase.
pub(crate) fn picker_phase_title(models: &ModelState, args_query: &str) -> String {
    if detect_effort_phase(models, args_query).is_some() {
        "Pick reasoning effort".to_string()
    } else if let Some(label) = group_phase_label(models, args_query) {
        format!("Pick {label} model")
    } else {
        "Pick model".to_string()
    }
}

/// One row per effort level for the `/model` chained effort phase.
/// `insert_text` is `"ModelName high"` so selecting a row completes both tokens.
fn build_effort_items(models: &ModelState, model_id: &acp::ModelId) -> Vec<ArgItem> {
    let info = match models.available.get(model_id) {
        Some(info) => info,
        None => return Vec::new(),
    };
    let model_name = info.name.clone();
    let is_current_model = models.current.as_ref() == Some(model_id);
    let options = models.reasoning_effort_options_for(model_id);
    build_effort_arg_items(
        &options,
        models.reasoning_effort,
        is_current_model,
        |option| format!("{model_name} {}", option.id),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use cf_shell::sampling::types::ReasoningEffort;

    fn model_with_reasoning(id: &str, name: &str) -> (acp::ModelId, acp::ModelInfo) {
        let id = acp::ModelId::new(Arc::from(id));
        let mut meta = serde_json::Map::new();
        meta.insert(
            "supportsReasoningEffort".into(),
            serde_json::Value::Bool(true),
        );
        let info = acp::ModelInfo::new(id.clone(), name.to_string())
            .meta(serde_json::Value::Object(meta).as_object().cloned());
        (id, info)
    }

    fn plain_model(id: &str, name: &str) -> (acp::ModelId, acp::ModelInfo) {
        let id = acp::ModelId::new(Arc::from(id));
        let info = acp::ModelInfo::new(id.clone(), name.to_string());
        (id, info)
    }

    static EMPTY_BUNDLE: crate::app::bundle::BundleState = crate::app::bundle::BundleState {
        has_cache: false,
        version: String::new(),
        personas: Vec::new(),
        roles: Vec::new(),
        agents: Vec::new(),
        skills: Vec::new(),
        persona_details: Vec::new(),
        role_details: Vec::new(),
    };

    fn dummy_exec_ctx(models: &ModelState) -> CommandExecCtx<'_> {
        CommandExecCtx {
            models,
            session_id: None,
            bundle_state: &EMPTY_BUNDLE,
            screen_mode: crate::app::ScreenMode::Inline,
            pager_state: crate::settings::PagerLocalSnapshot {
                multiline_mode: false,
                yolo_mode: false,
                ..crate::settings::PagerLocalSnapshot::default()
            },
        }
    }

    #[test]
    fn split_trailing_token_splits_on_final_whitespace() {
        assert_eq!(
            split_trailing_token("Reasoning X high"),
            Some(("Reasoning X", "high"))
        );
        assert_eq!(
            split_trailing_token("reasoning-x  xhigh"),
            Some(("reasoning-x", "xhigh"))
        );
        // No interior whitespace → nothing to split off.
        assert!(split_trailing_token("reasoning-x-pro").is_none());
    }

    #[test]
    fn empty_query_returns_one_row_per_logical_model() {
        let mut state = ModelState::default();
        let (rid, rinfo) = model_with_reasoning("reasoning-x", "Reasoning X");
        let (pid, pinfo) = plain_model("grok-4.5", "Grok 4.5");
        state.available.insert(rid, rinfo);
        state.available.insert(pid, pinfo);

        let cmd = ModelCommand;
        let ctx = AppCtx {
            models: &state,
            cwd: std::path::Path::new("."),
            has_session_announcements: false,
            screen_mode: crate::app::ScreenMode::Fullscreen,
        };
        let items = cmd.suggest_args(&ctx, "").unwrap();
        assert_eq!(items.len(), 2, "model phase: one row per logical model");

        // Reasoning model has trailing space in insert_text -- this is the
        // signal the prompt widget reads to keep the dropdown open after
        // Enter so the effort sub-menu can render.
        let reasoning = items
            .iter()
            .find(|i| i.match_text == "Reasoning X")
            .unwrap();
        assert_eq!(reasoning.insert_text, "Reasoning X ");

        // Plain model has no trailing space -- Enter commits immediately.
        let plain = items.iter().find(|i| i.match_text == "Grok 4.5").unwrap();
        assert_eq!(plain.insert_text, "Grok 4.5");
    }

    #[test]
    fn trailing_space_after_reasoning_model_enters_effort_phase() {
        let mut state = ModelState::default();
        let (id, info) = model_with_reasoning("reasoning-x", "Reasoning X");
        state.available.insert(id, info);

        let cmd = ModelCommand;
        let ctx = AppCtx {
            models: &state,
            cwd: std::path::Path::new("."),
            has_session_announcements: false,
            screen_mode: crate::app::ScreenMode::Fullscreen,
        };
        // Args query has a trailing space -> effort phase. Items come out
        // ordered xhigh -> low (strongest first) per EFFORT_LEVELS.
        let items = cmd.suggest_args(&ctx, "Reasoning X ").unwrap();
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].insert_text, "Reasoning X xhigh");
        assert_eq!(items[1].insert_text, "Reasoning X high");
        assert_eq!(items[2].insert_text, "Reasoning X medium");
        assert_eq!(items[3].insert_text, "Reasoning X low");
        // Display is just the level so the user sees a clean column.
        assert_eq!(items[0].display, "xhigh");
        // match_text carries the sort-key prefix that forces the matcher's
        // alphabetical tiebreak to render rows in EFFORT_LEVELS order.
        assert!(items[0].match_text.starts_with("a "));
        assert!(items[3].match_text.starts_with("d "));
    }

    #[test]
    fn partial_effort_query_still_in_effort_phase() {
        let mut state = ModelState::default();
        let (id, info) = model_with_reasoning("reasoning-x", "Reasoning X");
        state.available.insert(id, info);

        let cmd = ModelCommand;
        let ctx = AppCtx {
            models: &state,
            cwd: std::path::Path::new("."),
            has_session_announcements: false,
            screen_mode: crate::app::ScreenMode::Fullscreen,
        };
        // Still in effort phase; matcher upstream narrows to high / xhigh.
        let items = cmd.suggest_args(&ctx, "Reasoning X h").unwrap();
        assert_eq!(items.len(), 4);
    }

    #[test]
    fn partial_model_query_stays_in_model_phase() {
        let mut state = ModelState::default();
        let (id, info) = model_with_reasoning("reasoning-x", "Reasoning X");
        state.available.insert(id, info);

        let cmd = ModelCommand;
        let ctx = AppCtx {
            models: &state,
            cwd: std::path::Path::new("."),
            has_session_announcements: false,
            screen_mode: crate::app::ScreenMode::Fullscreen,
        };
        // No trailing space, user is still typing the model name.
        let items = cmd.suggest_args(&ctx, "Reason").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].insert_text, "Reasoning X ");
    }

    #[test]
    fn run_parses_model_plus_effort_when_supported() {
        let mut state = ModelState::default();
        let (id, info) = model_with_reasoning("reasoning-x", "Reasoning X");
        state.available.insert(id, info);
        let mut ctx = dummy_exec_ctx(&state);
        let result = ModelCommand.run(&mut ctx, "Reasoning X xhigh");
        match result {
            CommandResult::Action(Action::SwitchModel { model_id, effort }) => {
                assert_eq!(model_id.0.as_ref(), "reasoning-x");
                assert_eq!(effort, Some(ReasoningEffort::Xhigh));
            }
            other => panic!("expected SwitchModel with effort, got {other:?}"),
        }
    }

    #[test]
    fn run_rejects_unoffered_effort_with_effort_error_not_unknown_model() {
        // Regression: previously `resolve_effort_token_for` returned None and
        // the handler fell through to `Unknown model: Reasoning X none`.
        let mut state = ModelState::default();
        let (id, info) = model_with_reasoning("reasoning-x", "Reasoning X");
        state.available.insert(id, info);
        let mut ctx = dummy_exec_ctx(&state);
        let result = ModelCommand.run(&mut ctx, "Reasoning X none");
        match result {
            CommandResult::Error(msg) => {
                assert!(
                    msg.contains("unknown effort level 'none'"),
                    "expected effort error, got {msg}"
                );
                assert!(
                    msg.contains("use one of:"),
                    "expected offered levels in message, got {msg}"
                );
                assert!(
                    !msg.to_lowercase().contains("unknown model"),
                    "must not misreport as unknown model: {msg}"
                );
                let offered = msg.split_once("; ").map(|(_, r)| r).unwrap_or("");
                assert!(
                    !offered.contains("none"),
                    "must not list none as offered: {msg}"
                );
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn run_prefers_full_multi_word_model_name_over_prefix_plus_effort() {
        // Catalog has both "Grok" (reasoning) and "Grok 4.5". `/model Grok 4.5`
        // must select the full name, not treat "4.5" as an effort on "Grok".
        let mut state = ModelState::default();
        let (short_id, short_info) = model_with_reasoning("grok", "Grok");
        let (long_id, long_info) = model_with_reasoning("grok-4.5", "Grok 4.5");
        state.available.insert(short_id, short_info);
        state.available.insert(long_id.clone(), long_info);
        let mut ctx = dummy_exec_ctx(&state);
        let result = ModelCommand.run(&mut ctx, "Grok 4.5");
        match result {
            CommandResult::Action(Action::SetDefaultModel(resolved_id)) => {
                assert_eq!(resolved_id, long_id);
            }
            other => panic!("expected SetDefaultModel(Grok 4.5), got {other:?}"),
        }
    }

    #[test]
    fn run_rejects_effort_for_non_reasoning_model() {
        let mut state = ModelState::default();
        let (id, info) = plain_model("grok-4.5", "Grok 4.5");
        state.available.insert(id, info);
        let mut ctx = dummy_exec_ctx(&state);
        let result = ModelCommand.run(&mut ctx, "Grok 4.5 high");
        // Falls through to "is the whole string a model name?" — which
        // it isn't, so we get an Unknown error.
        assert!(matches!(result, CommandResult::Error(_)));
    }

    /// The bare `/model <name>` form dispatches
    /// `Action::SetDefaultModel(<ModelId>)` instead of the legacy
    /// `Action::SwitchModel { effort: None }`. The dispatcher routes
    /// the typed setter through both `Effect::SwitchModel`
    /// (session-level mutation) AND `Effect::PersistSetting`
    /// (next-session default).
    ///
    /// The payload is the typed `acp::ModelId` (resolved at the slash
    /// boundary), not a String.
    #[test]
    fn run_bare_model_name_dispatches_set_default_model() {
        let mut state = ModelState::default();
        let (id, info) = plain_model("grok-4.5", "Grok 4.5");
        state.available.insert(id.clone(), info);
        let mut ctx = dummy_exec_ctx(&state);
        let result = ModelCommand.run(&mut ctx, "Grok 4.5");
        match result {
            CommandResult::Action(Action::SetDefaultModel(resolved_id)) => {
                assert_eq!(resolved_id, id);
            }
            other => panic!("expected Action::SetDefaultModel(<id>), got {other:?}"),
        }
    }

    /// Case-insensitive matching against the catalog: `/model grok 4.5`
    /// resolves to the same `ModelId` as `/model Grok 4.5`.
    #[test]
    fn run_set_default_model_resolves_case_insensitively() {
        let mut state = ModelState::default();
        let (id, info) = plain_model("grok-4.5", "Grok 4.5");
        state.available.insert(id.clone(), info);
        let mut ctx = dummy_exec_ctx(&state);
        let result = ModelCommand.run(&mut ctx, "grok 4.5");
        match result {
            CommandResult::Action(Action::SetDefaultModel(resolved_id)) => {
                assert_eq!(resolved_id, id);
            }
            other => panic!("expected Action::SetDefaultModel(<id>), got {other:?}"),
        }
    }

    fn model_ctx(models: &ModelState) -> AppCtx<'_> {
        AppCtx {
            models,
            cwd: std::path::Path::new("."),
            has_session_announcements: false,
            screen_mode: crate::app::ScreenMode::Fullscreen,
        }
    }

    fn grouped_model(id: &str, name: &str, group: &str) -> (acp::ModelId, acp::ModelInfo) {
        let id = acp::ModelId::new(Arc::from(id));
        let mut meta = serde_json::Map::new();
        meta.insert(
            "group".to_string(),
            serde_json::Value::String(group.to_string()),
        );
        (
            id.clone(),
            acp::ModelInfo::new(id, name.to_string()).meta(Some(meta)),
        )
    }

    #[test]
    fn grouped_models_fold_into_one_row() {
        let mut state = ModelState::default();
        let (pid, pinfo) = plain_model("grok-4.5", "Grok 4.5");
        let (w1, w1i) = grouped_model("wb-hy3", "wb-hy3", "workbuddy");
        let (w2, w2i) = grouped_model("wb-glm-5.3", "wb-glm-5.3", "workbuddy");
        state.available.insert(pid, pinfo);
        state.available.insert(w1, w1i);
        state.available.insert(w2, w2i);

        let cmd = ModelCommand;
        let items = cmd.suggest_args(&model_ctx(&state), "").unwrap();
        // Ungrouped models keep catalog order first; group rows follow.
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].display, "Grok 4.5");
        let group = &items[1];
        assert!(
            group.display.contains("workbuddy"),
            "got: {}",
            group.display
        );
        assert!(group.display.contains("2 models"), "got: {}", group.display);
        assert_eq!(group.insert_text, "workbuddy ");
        assert!(group.match_text.contains("wb-hy3"));
        assert!(group.match_text.contains("wb-glm-5.3"));
    }

    #[test]
    fn group_query_expands_members() {
        let mut state = ModelState::default();
        let (pid, pinfo) = plain_model("grok-4.5", "Grok 4.5");
        let (w1, w1i) = grouped_model("wb-hy3", "wb-hy3", "workbuddy");
        let (w2, w2i) = grouped_model("wb-kimi-k2.7", "wb-kimi-k2.7", "workbuddy");
        state.available.insert(pid, pinfo);
        state.available.insert(w1, w1i);
        state.available.insert(w2, w2i);

        let cmd = ModelCommand;
        for q in ["workbuddy", "workbuddy ", "workbuddy glm", "WorkBuddy"] {
            let items = cmd
                .suggest_args(&model_ctx(&state), q)
                .unwrap_or_else(|| panic!("query {q:?} should expand the group"));
            assert_eq!(items.len(), 2, "query {q:?}");
            assert!(items.iter().all(|i| i.display.starts_with("wb-")));
            // Prefixed match_text keeps controller ranking working against
            // the full "<label> <fragment>" query.
            assert!(items[0].match_text.starts_with("a workbuddy "));
        }
    }

    #[test]
    fn unrelated_fragment_keeps_group_collapsed() {
        let mut state = ModelState::default();
        let (pid, pinfo) = plain_model("grok-4.5", "Grok 4.5");
        let (wid, winfo) = grouped_model("wb-kimi-k2.7", "wb-kimi-k2.7", "workbuddy");
        state.available.insert(pid, pinfo);
        state.available.insert(wid, winfo);

        // "kimi" is not a group label: still the collapsed top list (the
        // caller's matcher surfaces the group row via its match_text).
        let cmd = ModelCommand;
        let items = cmd.suggest_args(&model_ctx(&state), "kimi").unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].display, "Grok 4.5");
        assert!(items[1].display.contains("workbuddy"));
        assert!(items[1].match_text.contains("wb-kimi-k2.7"));
    }

    #[test]
    fn group_row_marks_current_and_run_rejects_bare_label() {
        let mut state = ModelState::default();
        let (pid, pinfo) = plain_model("grok-4.5", "Grok 4.5");
        let (wid, winfo) = grouped_model("wb-hy3", "wb-hy3", "workbuddy");
        state.available.insert(pid, pinfo);
        state.available.insert(wid.clone(), winfo);
        state.current = Some(wid);

        let cmd = ModelCommand;
        let items = cmd.suggest_args(&model_ctx(&state), "").unwrap();
        let group = items.last().unwrap();
        assert!(
            group.display.contains("(current)"),
            "got: {}",
            group.display
        );

        let mut ctx = dummy_exec_ctx(&state);
        let result = ModelCommand.run(&mut ctx, "workbuddy");
        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("source group"), "got: {msg}");
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }
}
