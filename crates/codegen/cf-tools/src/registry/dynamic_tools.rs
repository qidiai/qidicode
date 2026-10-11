//! Phase B projection: the three-segment `defs` split
//! (P0-2 第二步 design doc §6 Phase B, Q1/Q2.5).
//!
//! The pipeline splits each turn's `defs` into two projected segments:
//!
//! - **常驻段 (resident)** — every tool whose [`ToolKind`] classifies to
//!   [`ToolSegment::Resident`] / [`ToolSegment::ResidentTailSlot`], plus the
//!  恒驻 meta pair (`search_tool` / `use_tool`, [`ToolSegment::Meta`]). Ships
//!   at the head of every turn, byte-stable (KV-cache prefix).
//! - **按需段 (imported)** — the built-in tools whose kind classifies to
//!   [`ToolSegment::Imported`]. **Hidden** until the model unlocks them via
//!   `search_tool`; unlocked tools are appended to the tail of the resident
//!   segment, in unlock order, **append-only** (Q2.5 分段三律 #2).
//!
//! This module is pure data: it takes the already-resolved built-in `defs`
//! (with a kind lookup) and produces the partition + the projected defs. The
//! cf-shell assembly point (`prepare_tool_definitions_inner`) is the only
//! caller that turns it on — and only when `dynamic_tools=true` (Phase A's
//! pass-through is preserved for the default).
//!
//! Ordering rationale (design doc Q1): within the resident segment the order
//! is `(resident kinds in registration order) ++ (plan tail-slot pair) ++
//! (meta pair)`. The tail-slot bucket pins the plan pair to a fixed tail
//! position (Q2-plan). Preserving the toolset's registration order inside
//! each bucket keeps the resident segment a **deterministic function of the
//! full defs list** — which is exactly the C-10 prefix-stability contract
//! (flag=false's resident subset == flag=true's resident segment).

use crate::registry::resident_set::ToolSegment;
use crate::types::definition::ToolDefinition;
use crate::types::tool::ToolKind;

/// One built-in tool's segmentation input: wire name, kind, and definition.
#[derive(Debug, Clone, Copy)]
pub struct SegEntry<'a> {
    /// Client-facing wire name (`defs[].function.name`).
    pub client_name: &'a str,
    /// The tool's **declared** kind (registration-time trust source, Q1 规则 2).
    pub kind: ToolKind,
    /// The assembled wire definition.
    pub definition: &'a ToolDefinition,
}

/// The split of a turn's built-in `defs` into the two projected segments.
#[derive(Debug, Clone, Default)]
pub struct DefsPartition {
    /// 常驻段 — shipped every turn at the defs head (resident + tail-slot +
    /// meta), in the deterministic order described above.
    pub resident: Vec<ToolDefinition>,
    /// import candidates — `(client_name, definition)` in registration order.
    /// Only the tools named by the unlock list are projected, appended in
    /// unlock order.
    pub imported_pool: Vec<(String, ToolDefinition)>,
}

impl DefsPartition {
    /// Project the defs for a turn given the current unlock list.
    ///
    /// **Append-only by construction**: the resident head is fixed and every
    /// unlocked tool is appended in `unlocked` order — never inserted, never
    /// reordered. Unknown names (not in this toolset's import pool) are
    /// skipped, and repeats are de-duplicated, so the output is a monotone
    /// prefix-growth of the resident segment as `unlocked` grows.
    pub fn assemble(&self, unlocked: &[String]) -> Vec<ToolDefinition> {
        let mut out = self.resident.clone();
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for name in unlocked {
            if !seen.insert(name.as_str()) {
                continue;
            }
            if let Some((_, def)) = self.imported_pool.iter().find(|(n, _)| n == name) {
                out.push(def.clone());
            }
        }
        out
    }

    /// The resident segment alone (C-10 anchor: the byte-stable prefix).
    pub fn resident(&self) -> &[ToolDefinition] {
        &self.resident
    }
}

/// Partition built-in `defs` given per-tool segmentation entries.
///
/// Bucket order for the resident segment: `Resident` (registration order) ++
/// `ResidentTailSlot` (registration order) ++ `Meta` (registration order).
/// Everything else lands in [`DefsPartition::imported_pool`] in registration
/// order.
pub fn partition_builtin_defs<'a, I>(entries: I) -> DefsPartition
where
    I: IntoIterator<Item = SegEntry<'a>>,
{
    let mut head: Vec<ToolDefinition> = Vec::new();
    let mut tail: Vec<ToolDefinition> = Vec::new();
    let mut meta: Vec<ToolDefinition> = Vec::new();
    let mut imported: Vec<(String, ToolDefinition)> = Vec::new();
    for entry in entries {
        match entry.kind.pipeline_segment() {
            ToolSegment::Resident => head.push(entry.definition.clone()),
            ToolSegment::ResidentTailSlot => tail.push(entry.definition.clone()),
            ToolSegment::Meta => meta.push(entry.definition.clone()),
            ToolSegment::Imported => {
                imported.push((entry.client_name.to_owned(), entry.definition.clone()));
            }
        }
    }
    head.extend(tail);
    head.extend(meta);
    DefsPartition {
        resident: head,
        imported_pool: imported,
    }
}

/// Partition `defs` using a name → [`ToolKind`] lookup.
///
/// A name the lookup cannot resolve is treated as **resident** (conservative:
/// never hide a tool we cannot classify — design doc Q1 代价模型 偏保守).
/// This keeps the resident segment a superset of the flag=false view for any
/// tool whose kind is momentarily unknown, so projection can never *drop* a
/// tool the model could otherwise see.
pub fn partition_defs_with<F>(defs: &[ToolDefinition], kind_of: F) -> DefsPartition
where
    F: Fn(&str) -> Option<ToolKind>,
{
    partition_builtin_defs(defs.iter().map(|def| {
        let name = def.function.name.as_str();
        // An unresolved name falls back to `Read` (a `Resident` kind): never
        // hide a tool we cannot classify (design doc Q1 偏保守).
        let kind = kind_of(name).unwrap_or(ToolKind::Read);
        SegEntry {
            client_name: name,
            kind,
            definition: def,
        }
    }))
}

/// Build the built-in search surface from a partition's import pool
/// (design doc B-0/B-1): every hidden built-in tool, with the exact wire name
/// the model must relay and its full input schema. The resident segment is
/// deliberately excluded — the design mandates that `search_tool` never
/// returns tools already present in `defs` (Q2.1 tool_result 语义).
pub fn builtin_search_surface(
    partition: &DefsPartition,
) -> Vec<crate::types::resources::BuiltinToolEntry> {
    partition
        .imported_pool
        .iter()
        .map(|(client_name, def)| crate::types::resources::BuiltinToolEntry {
            client_name: client_name.clone(),
            description: def.function.description.clone().unwrap_or_default(),
            input_schema: def.function.parameters.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::definition::ToolDefinition;

    fn def(name: &str) -> ToolDefinition {
        ToolDefinition::function(name, Some(format!("{name} desc")), serde_json::json!({}))
    }

    /// Kind lookup for the fixture: names encode their kind.
    fn kind_of(name: &str) -> Option<ToolKind> {
        Some(match name {
            "read_file" => ToolKind::Read,
            "run_terminal_cmd" => ToolKind::Execute,
            "enter_plan_mode" => ToolKind::EnterPlan,
            "exit_plan_mode" => ToolKind::ExitPlan,
            "search_tool" => ToolKind::SearchTool,
            "use_tool" => ToolKind::UseTool,
            "think" => ToolKind::Think,
            "video_gen" => ToolKind::VideoGen,
            "monitor" => ToolKind::Monitor,
            _ => return None,
        })
    }

    /// C-10 core: the resident segment is a deterministic function of the
    /// full defs list — registration order preserved within each bucket,
    /// plan pair pinned to the tail slot, meta pair last.
    #[test]
    fn resident_segment_bucket_order_is_deterministic() {
        // Registration order deliberately interleaves plan/meta between
        // resident + imported tools.
        let defs = vec![
            def("read_file"),
            def("think"),
            def("enter_plan_mode"),
            def("run_terminal_cmd"),
            def("search_tool"),
            def("video_gen"),
            def("exit_plan_mode"),
            def("use_tool"),
            def("monitor"),
        ];
        let p = partition_defs_with(&defs, kind_of);
        let resident: Vec<&str> = p.resident().iter().map(|d| d.function.name.as_str()).collect();
        assert_eq!(
            resident,
            vec![
                "read_file",       // Resident bucket (registration order)
                "run_terminal_cmd",// Resident bucket
                "enter_plan_mode", // tail slot
                "exit_plan_mode",  // tail slot
                "search_tool",     // meta
                "use_tool",        // meta
            ],
            "resident = (resident kinds) ++ (plan tail slot) ++ (meta)"
        );
        let pool: Vec<&str> = p.imported_pool.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(pool, vec!["think", "video_gen", "monitor"]);
    }

    /// C-11 core: `assemble` is append-only — resident head fixed, unlocks
    /// appended in order, unknown names skipped, repeats de-duplicated.
    #[test]
    fn assemble_is_append_only_and_dedups() {
        let defs = vec![
            def("read_file"),
            def("search_tool"),
            def("use_tool"),
            def("think"),
            def("monitor"),
        ];
        let p = partition_defs_with(&defs, kind_of);
        let base: Vec<String> = p.resident().iter().map(|d| d.function.name.clone()).collect();
        assert_eq!(base, vec!["read_file", "search_tool", "use_tool"]);

        // No unlocks → resident only.
        assert_eq!(
            p.assemble(&[]).iter().map(|d| d.function.name.clone()).collect::<Vec<_>>(),
            base
        );
        // Unlock one → appended at the tail.
        let one = p.assemble(&["think".into()]);
        assert_eq!(
            one.iter().map(|d| d.function.name.clone()).collect::<Vec<_>>(),
            vec!["read_file", "search_tool", "use_tool", "think"]
        );
        // The resident (prefix) bytes are unchanged.
        assert_eq!(
            serde_json::to_string(p.resident()).unwrap(),
            serde_json::to_string(&one[..base.len()]).unwrap()
        );
        // Unlock order preserved; repeats + unknown skipped.
        let two = p.assemble(&["think".into(), "think".into(), "ghost".into(), "monitor".into()]);
        assert_eq!(
            two.iter().map(|d| d.function.name.clone()).collect::<Vec<_>>(),
            vec!["read_file", "search_tool", "use_tool", "think", "monitor"]
        );
    }

    /// Unresolved names are conservative (resident): a tool we cannot
    /// classify is never hidden.
    #[test]
    fn unresolved_kind_is_resident_conservative() {
        let defs = vec![def("mystery_tool"), def("think")];
        let p = partition_defs_with(&defs, kind_of);
        let resident: Vec<&str> = p.resident().iter().map(|d| d.function.name.as_str()).collect();
        assert_eq!(resident, vec!["mystery_tool"]);
        assert_eq!(p.imported_pool.len(), 1);
    }
}
