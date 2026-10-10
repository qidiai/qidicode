//! Resident-set classification for the dynamic tool pipeline
//! (P0-2 第二步 design doc, Q1 穷尽表 v3.3).
//!
//! The pipeline splits the per-turn tool definitions (`defs`) into a
//! **resident segment** (unconditionally shipped at the defs head,
//! byte-stable across turns) and an **imported segment** (hidden until
//! unlocked via `search_tool`, append-only). This module is the
//! **criterion skeleton**: every `ToolKind` variant states where it
//! belongs. The `match` below is deliberately exhaustive — **no `_`
//! arm** — so a new `ToolKind` variant fails to compile until it is
//! triaged here (same forcing function as `ALL_TOOL_KINDS` in
//! `cf-workspace/src/capability.rs` and the exhaustive
//! `presentation_name` / `is_read_only` matches in
//! `tool_taxonomy.rs`).
//!
//! Design-doc anchors (all `docs/token-optimization/P0-2第二步-动态工具管线设计稿.md`):
//! - Q1 穷尽表 (36 variants): the per-variant classification below.
//! - Q1 规则 3: MCP-namespace tools are **never** resident — the
//!   namespace is per-tool (not per-kind), so that rule is applied at
//!   projection time by the caller, not in this per-kind table.
//! - Q1 规则 4: tool packs (`register_tool_pack`) are pre-injected and
//!   excluded from resident-set judgment.
//! - 回退方案: the resident membership is **config data**
//!   ([`RESIDENT_SEGMENT_KINDS`]) carried separately from the match
//!   skeleton — retuning the resident set (driven by the ratio metric
//!   + unlock-miss telemetry) edits the table, not the code path.
//!
//! Phase A scope note: segmentation is **not active** yet
//! (`dynamic_tools` defaults to false; `true` is a documented no-op
//! until Phase B). This module ships the skeleton + tests only; the
//! projection code that consumes it lands in Phase B.

use crate::types::tool::ToolKind;

/// Where a tool lands in the `defs` projection under the dynamic
/// tool pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSegment {
    /// 常驻段 — shipped unconditionally at the defs head every turn,
    /// in a fixed order, never added/removed/reordered within a
    /// session (KV-cache prefix stability).
    Resident,
    /// 常驻段尾部固定槽 — resident, but pinned to the tail of the
    /// resident segment (the plan-mode pair, Q2-plan): the plan gate
    /// must never trim them, and any future plan-gated tool may only
    /// land here or in the imported segment.
    ResidentTailSlot,
    /// 恒驻 meta 层 — the pipeline's own channels (`search_tool` /
    /// `use_tool`). Not registered into `defs` in this build
    /// (Phase B registers them); until then they behave as
    /// "registered-if-present, never trimmed" (C-4 conditional form).
    Meta,
    /// 按需段 — hidden from `defs` until unlocked via `search_tool`
    /// (Phase B). Until then (Phase A) everything ships full — this
    /// classification is the declaration of intent, not an active
    /// filter.
    Imported,
}

impl ToolKind {
    /// Classify this kind into its `defs` segment. Exhaustive — no
    /// wildcard arm — so the compiler forces a triage decision for
    /// every new `ToolKind` variant (design doc Q1 穷尽表, Sonnet
    /// 必改 6).
    ///
    /// Classification per the design-doc table (variant order =
    /// `ToolKind` declaration order, `cf-tools/src/types/tool.rs`):
    ///
    /// | # | Kind | Segment | Rationale (design doc) |
    /// |---|------|---------|------------------------|
    /// | 1 | Read | Resident | 万能前摇；read-only |
    /// | 2 | Edit | Resident | 写入循环主体 |
    /// | 3 | Delete | Imported | 低频；调表即可 |
    /// | 4 | ListDir | Resident | 探索链；kind 与实例分离 |
    /// | 5 | Write | Resident | 造文件主路径 |
    /// | 6 | Move | Imported | 低频（重命名） |
    /// | 7 | Search | Resident | 探索链正文 |
    /// | 8 | Lsp | Imported | 重度低频 |
    /// | 9 | Execute | Resident | 环境操作主轴 |
    /// | 10 | Plan | Imported | 穷尽表态防静默 |
    /// | 11 | WebSearch | Imported | backend-search 剔除点作用于全 defs，进常驻中部会挖穿前缀 |
    /// | 12 | WebFetch | Imported | 低频 |
    /// | 13 | BackgroundTaskAction | Imported | 后台族 |
    /// | 14 | WaitTasksAction | Imported | 后台族 |
    /// | 15 | KillTaskAction | Imported | 后台族、低频 |
    /// | 16 | List | Resident | 轻探索；一 kind 两实例（list_dir + glob）全取 |
    /// | 17 | Skill | Imported | 低频 |
    /// | 18 | MemorySearch | Imported | 启动普遍、持续低频 |
    /// | 19 | MemoryGet | Imported | 同上 |
    /// | 20 | Task | Imported | 一次任务 spawn 数次 |
    /// | 21 | EnterPlan | ResidentTailSlot | 进出门槛；plan 约束见 Q2-plan |
    /// | 22 | ExitPlan | ResidentTailSlot | 同上 |
    /// | 23 | AskUser | Resident | 提问是防卡死红线 |
    /// | 24 | ImageGen | Imported | 生成系低频+重 |
    /// | 25 | VideoGen | Imported | 同上 |
    /// | 26 | ImageToVideo | Imported | 同上 |
    /// | 27 | ReferenceToVideo | Imported | 同上 |
    /// | 28 | DeployApp | Imported | 低频 |
    /// | 29 | SearchTool | Meta | 管线自通道（找回入口） |
    /// | 30 | UseTool | Meta | 执行通道 |
    /// | 31 | Monitor | Imported | 编排件；低频 |
    /// | 32 | GoalUpdate | Imported | 编排件 |
    /// | 33 | Think | Imported | 编排件 |
    /// | 34 | BrowserRead | Imported | browser 族 |
    /// | 35 | BrowserAct | Imported | 同上 |
    /// | 36 | Other | Imported | MCP 动态注册兜底 |
    pub fn pipeline_segment(self) -> ToolSegment {
        match self {
            ToolKind::Read => ToolSegment::Resident,
            ToolKind::Edit => ToolSegment::Resident,
            ToolKind::Delete => ToolSegment::Imported,
            ToolKind::ListDir => ToolSegment::Resident,
            ToolKind::Write => ToolSegment::Resident,
            ToolKind::Move => ToolSegment::Imported,
            ToolKind::Search => ToolSegment::Resident,
            ToolKind::Lsp => ToolSegment::Imported,
            ToolKind::Execute => ToolSegment::Resident,
            ToolKind::Plan => ToolSegment::Imported,
            ToolKind::WebSearch => ToolSegment::Imported,
            ToolKind::WebFetch => ToolSegment::Imported,
            ToolKind::BackgroundTaskAction => ToolSegment::Imported,
            ToolKind::WaitTasksAction => ToolSegment::Imported,
            ToolKind::KillTaskAction => ToolSegment::Imported,
            ToolKind::List => ToolSegment::Resident,
            ToolKind::Skill => ToolSegment::Imported,
            ToolKind::MemorySearch => ToolSegment::Imported,
            ToolKind::MemoryGet => ToolSegment::Imported,
            ToolKind::Task => ToolSegment::Imported,
            ToolKind::EnterPlan => ToolSegment::ResidentTailSlot,
            ToolKind::ExitPlan => ToolSegment::ResidentTailSlot,
            ToolKind::AskUser => ToolSegment::Resident,
            ToolKind::ImageGen => ToolSegment::Imported,
            ToolKind::VideoGen => ToolSegment::Imported,
            ToolKind::ImageToVideo => ToolSegment::Imported,
            ToolKind::ReferenceToVideo => ToolSegment::Imported,
            ToolKind::DeployApp => ToolSegment::Imported,
            ToolKind::SearchTool => ToolSegment::Meta,
            ToolKind::UseTool => ToolSegment::Meta,
            ToolKind::Monitor => ToolSegment::Imported,
            ToolKind::GoalUpdate => ToolSegment::Imported,
            ToolKind::Think => ToolSegment::Imported,
            ToolKind::BrowserRead => ToolSegment::Imported,
            ToolKind::BrowserAct => ToolSegment::Imported,
            ToolKind::Other => ToolSegment::Imported,
        }
    }
}

/// The resident-set membership table (config data — the design doc's
/// 回退方案: the `match` above is the criterion skeleton, this table
/// is the tunable membership). Retuning which kinds the resident set
/// actually carries edits this list, not the code path; the test
/// below keeps the table in sync with the exhaustive match.
///
/// Membership = every kind whose [`ToolSegment`] is [`Resident`] or
/// [`ResidentTailSlot`]. The meta pair ([`ToolKind::SearchTool`] /
/// [`ToolKind::UseTool`]) is deliberately **not** listed here: it is
/// the pipeline's own channel (Phase B registers it into `defs`;
/// today it is not projected at all), tracked separately via
/// [`META_SEGMENT_KINDS`].
pub const RESIDENT_SEGMENT_KINDS: &[ToolKind] = &[
    ToolKind::Read,
    ToolKind::Edit,
    ToolKind::ListDir,
    ToolKind::Write,
    ToolKind::Search,
    ToolKind::Execute,
    ToolKind::List,
    ToolKind::AskUser,
    ToolKind::EnterPlan,
    ToolKind::ExitPlan,
];

/// The meta pair — the pipeline's own channels
/// (`search_tool` / `use_tool`). Never trimmed by any projection
/// config (C-4), registered into `defs` from Phase B on.
pub const META_SEGMENT_KINDS: &[ToolKind] =
    &[ToolKind::SearchTool, ToolKind::UseTool];

#[cfg(test)]
mod tests {
    use super::*;
    use strum::IntoEnumIterator;

    /// The exhaustive table (design doc Q1): every one of the 36
    /// `ToolKind` variants must classify to exactly the segment the
    /// design table assigns it. Iterating the enum (rather than
    /// hand-listing) makes this test fail for a *new* variant even
    /// before it lands in `pipeline_segment` — at which point the
    /// compiler has already failed the build (no `_` arm), so this
    /// table doubles as the per-variant expectation record.
    #[test]
    fn pipeline_segment_classifies_all_36_variants_per_design_table() {
        let mut seen = 0;
        for kind in ToolKind::iter() {
            seen += 1;
            let expected = match kind {
                ToolKind::Read
                | ToolKind::Edit
                | ToolKind::ListDir
                | ToolKind::Write
                | ToolKind::Search
                | ToolKind::Execute
                | ToolKind::List
                | ToolKind::AskUser => ToolSegment::Resident,
                ToolKind::EnterPlan | ToolKind::ExitPlan => {
                    ToolSegment::ResidentTailSlot
                }
                ToolKind::SearchTool | ToolKind::UseTool => ToolSegment::Meta,
                ToolKind::Delete
                | ToolKind::Move
                | ToolKind::Lsp
                | ToolKind::Plan
                | ToolKind::WebSearch
                | ToolKind::WebFetch
                | ToolKind::BackgroundTaskAction
                | ToolKind::WaitTasksAction
                | ToolKind::KillTaskAction
                | ToolKind::Skill
                | ToolKind::MemorySearch
                | ToolKind::MemoryGet
                | ToolKind::Task
                | ToolKind::ImageGen
                | ToolKind::VideoGen
                | ToolKind::ImageToVideo
                | ToolKind::ReferenceToVideo
                | ToolKind::DeployApp
                | ToolKind::Monitor
                | ToolKind::GoalUpdate
                | ToolKind::Think
                | ToolKind::BrowserRead
                | ToolKind::BrowserAct
                | ToolKind::Other => ToolSegment::Imported,
            };
            assert_eq!(
                kind.pipeline_segment(),
                expected,
                "kind {kind:?} must classify per the design-doc Q1 table"
            );
        }
        assert_eq!(
            seen,
            ToolKind::VARIANT_COUNT,
            "iteration must cover every ToolKind variant (36 at design time)"
        );
        // Design-doc invariant: 12 resident-side kind slots
        // (8 resident + 2 tail-slot + 2 meta).
        let resident_side = ToolKind::iter()
            .filter(|k| {
                matches!(
                    k.pipeline_segment(),
                    ToolSegment::Resident | ToolSegment::ResidentTailSlot | ToolSegment::Meta
                )
            })
            .count();
        assert_eq!(resident_side, 12, "design doc: 常驻段合计 kind 槽 12");
    }

    /// The membership table (config data) must agree with the match
    /// (criterion skeleton) for every variant — the two drift apart
    /// only if someone edits one and not the other.
    #[test]
    fn resident_table_agrees_with_exhaustive_match() {
        for kind in ToolKind::iter() {
            let in_table = RESIDENT_SEGMENT_KINDS.contains(&kind);
            let match_resident = matches!(
                kind.pipeline_segment(),
                ToolSegment::Resident | ToolSegment::ResidentTailSlot
            );
            assert_eq!(
                in_table,
                match_resident,
                "RESIDENT_SEGMENT_KINDS and pipeline_segment disagree on {kind:?}"
            );
            let in_meta = META_SEGMENT_KINDS.contains(&kind);
            assert_eq!(
                in_meta,
                kind.pipeline_segment() == ToolSegment::Meta,
                "META_SEGMENT_KINDS and pipeline_segment disagree on {kind:?}"
            );
        }
    }

    /// glob is classified under `List` (Opus 勘误： glob 归 List 非
    /// ListDir) — the `List` row is what makes the kind→Vec query
    /// return both `list_dir` and `glob` (design doc Q1 #16).
    #[test]
    fn list_kind_is_resident_covering_both_instances() {
        assert_eq!(ToolKind::List.pipeline_segment(), ToolSegment::Resident);
        assert_eq!(
            ToolKind::ListDir.pipeline_segment(),
            ToolSegment::Resident,
            "ListDir stays resident-if-registered (table-driven)"
        );
    }
}
