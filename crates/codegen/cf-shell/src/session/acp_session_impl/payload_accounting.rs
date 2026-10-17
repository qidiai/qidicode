//! 载荷单价 tap（payload per-unit tap）—— 纯观测层（推进方案 v2.3 放行版）。
//!
//! # 设计规格（v2.3 定案，实现勿改）
//!
//! - **choke point 唯一**：组装 messages+tools 提交 sampling 的那一处，内存结构上、
//!   单次序列化之前计数——绝不二次序列化/clone section。主回路与子代理机械上
//!   同 choke point 覆盖（P1-4），compaction 走 [`observe_compaction_request`]。
//! - **纯观测件**：默认关（`QIDI_PAYLOAD_TAP` 未设置 = 0 开销 0 载荷）；
//!   `QIDI_PAYLOAD_TAP=sample` 启用 1% 确定性采样（按 session_id 哈希，整会话
//!   同臂）；`QIDI_PAYLOAD_TAP=full`（dev flag）全量。永不成为限额/计费第二真源。
//! - **开销线**：字节计数 O(prompt) 亚毫秒（复用热路径零件
//!   `cf_chat_state::actor::request_builder::{serialized_json_bytes,
//!   conversation_body_bytes}`，request_builder.rs:468 / :531，经可见性提升
//!   复用，零新建）；真 tokenizer 禁进热路径——token 估算复用生产口径
//!   `cf_chat_state::actor::state::{estimate_item_tokens,
//!   estimate_tool_spec_tokens}`（内部即 cf-token-estimation 的
//!   `estimate_tokens_aware`，lib.rs:85，已核实生产消费在位）。
//!
//! # Section 分类学（8 类，全量分账）
//!
//! | # | 事件字段 | 口径 |
//! |---|---|---|
//! | 1 | `systemPromptStatic` / `systemPromptDynamicMemory` | 系统提示静态（跨请求稳定）与动态（`<memory-context>` 注入块）分开计 |
//! | 2 | `builtinToolDefs` | `request.tools` 净载荷（plan-mode 过滤后的 effective_tools，含 StructuredOutput 合成工具）；hosted_tools 单列子行 |
//! | 3 | `lenses.mcpServers.servers[]` | 每 MCP server 一行（server 级粒度，估算行） |
//! | 4 | `lenses.skillListing` | SkillListingSnapshot 实际载荷 |
//! | 5 | `conversationHistory` | 会话历史合计（items 逐条线上字节）——**Phase C 灰度槽位的归因基础（P1-1）** |
//! | 6 | `lenses.recent5ToolResults` | 近 5 条 tool results 合计（Phase C 成果压缩的对口杠杆；history 的子集透镜） |
//! | 7 | `lenses.attachments` | 内联图片（User content + ToolResult.images；history 的子集透镜） |
//! | 8 | `unattributedResidual` | items 线上总字节 − Σ分账字节（满分类学下 ≈ JSON 数组框架字节；增长即漂移/漏 section） |
//!
//! **分账完整性**：section 1/2/5/8 是**分区行**（Σ+残差 = 载荷全量，无重叠无遗漏）；
//! 3/4/6/7 是**透镜行**（分区行的子集视图，提供 server 级/清单级粒度，不进 Σ——
//! 防双算破对账，P1-3）。对账恒等式：`Σ(分区行 est_tokens) ≈ provider prompt_tokens`，
//! 告警对象 = 对账差额，阈值 15%（P1-3 校准：残差只监控增长率，不挂"残差大"告警）。
//!
//! # 前缀哈希与稳定性铁律（P1-2 执法，不悬空）
//!
//! 跨请求应稳定段（系统提示静态 + tool defs + 技能清单 [+ AGENTS.md]）逐段 sha256；
//! 跨轮对比（session-id 键控注册表）输出 `prefixChangedVsPrevTurn` = 缓存击穿点定位。
//! 不变量执法（tap 运行时顺带执法 + 单测钉死检测器）：
//! 1. 禁时间戳——稳定段出现 ISO 日期时间（`yyyy-mm-dd` + `T/space` + `hh:mm`）即违例；
//! 2. 禁 nonce——稳定段出现 ≥16 位十六进制连串即违例（uuid/请求 id 形态）；
//! 3. defs 顺序稳定——名字集合不变而 wire 顺序变 = 顺序抖动违例；
//! 4. defs 集合稳定——跨轮 sorted names 集合变化输出为 changed（合法解锁/plan 切换）。
//! 检测器为启发式（1/2 误报可能：会话级常量 id 也命中 2——记录违例不阻断，
//! 跨轮哈希对比才是缓存击穿的权威判定）。
//!
//! # 记忆行落位规则（防双算，step5 复核裁定落规格）
//!
//! 记忆字节 = System item 中 `<memory-context>...</memory-context>` 块（注入后
//! 权威形态，注入器 request_builder.rs:642-651）；`systemPromptStatic` 已扣除同块。
//! 二选一裁定：**按注入后块提取计**，入口参数不再参与计数——杜绝双算破对账。
//!
//! # 覆盖面留白声明（v2.1：未覆盖路径必须留白，不能默认被代表）
//!
//! - 主回路（main）/ 子代理（subagent，含 spawn 组装首轮）——turn.rs choke point；
//! - compaction（摘要请求三后端臂 + 重宣由主回路轮次的前缀哈希兜底）——session_compact；
//! - replay harness——golden 单测覆盖（非运行时计数）；
//! - **留白**：recap / memory_dream / laziness / reasoning_summarizer /
//!   trace_classifier 的辅助 sampling 请求未挂 tap（非四路覆盖目标）。
//!
//! # /context 域行（P2-6）
//!
//! 主/子代理最近一次观测存入注册表；[`context_rows`] 输出 AGENTS.md /
//! 近 5 条 tool results / attachments / 残差四行，经既有
//! TokenUsageCategory→ContextInfo.usage_categories→LegendRow 管道直达 /context
//! （tap 未跑过则零行 = 零行为变更）。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use cf_chat_state::actor::request_builder::{conversation_body_bytes, serialized_json_bytes};
use cf_chat_state::actor::state::{estimate_item_tokens, estimate_tool_spec_tokens};
use cf_chat_state::types::{MEMORY_CONTEXT_CLOSE_TAG, MEMORY_CONTEXT_OPEN_TAG};
use cf_sampling_types::conversation::{ConversationItem, ContentPart, ConversationRequest, HostedTool, ToolSpec};
use cf_sampling_types::conversation::TokenUsage;
use cf_token_estimation::{estimate_image_tokens, estimate_tokens_aware, BYTES_PER_TOKEN};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::prompt_build::is_project_instructions;
use crate::session::acp_types::TokenUsageCategory;

/// dev flag / 采样模式（环境变量 `QIDI_PAYLOAD_TAP`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TapMode {
    /// 默认：完全关闭（0 开销 0 载荷，纯观测件承诺）。
    Off,
    /// 1% 确定性采样（按 session_id 哈希，整会话同臂）。
    Sampled1Pct,
    /// dev flag：全量。
    Full,
}

/// 事件名（搭现有 unified_log per-request 事件流便车）。
const TURN_TAP_EVENT: &str = "shell.turn.payload_tap";
const COMPACT_TAP_EVENT: &str = "shell.compact.payload_tap";
/// 对账告警阈值（P1-3：告警对象 = 对账差额 Σ+残差−provider_total）。
const RECONCILIATION_WARN_PCT: f64 = 15.0;
const RECONCILIATION_INTERPRETATION: &str = "drift = est_total(sections+residual) - provider prompt_tokens; negative = provider chat-template/serialization overhead; |drift|>15% = drift or missed section";
const CALIBRATION_NOTE: &str = "estimator=estimate_tokens_aware (shared production caliber via estimate_item_tokens/estimate_tool_spec_tokens); chars->tokens offline calibrator: pending (预留接口)";

/// 注册表容量上限（session 级，条目极小；超限逐出任意旧键）。
const TAP_MEMORY_CAP: usize = 512;

fn parse_tap_mode(raw: Option<&str>) -> TapMode {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        Some("sample") | Some("1") | Some("1%") => TapMode::Sampled1Pct,
        Some("full") | Some("all") | Some("on") | Some("true") => TapMode::Full,
        _ => TapMode::Off,
    }
}

fn tap_mode() -> TapMode {
    static CELL: OnceLock<TapMode> = OnceLock::new();
    *CELL.get_or_init(|| {
        parse_tap_mode(std::env::var("QIDI_PAYLOAD_TAP").ok().as_deref())
    })
}

fn fnv1a64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 采样判定：默认关；sample 模式按 session_id 哈希 1%（整会话同臂，会话内
/// 前缀哈希跨轮对比因此总是可用）；full 模式全量。
fn should_observe(session_id: &str) -> bool {
    match tap_mode() {
        TapMode::Off => false,
        TapMode::Full => true,
        TapMode::Sampled1Pct => fnv1a64(session_id) % 100 == 0,
    }
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// 会话级 tap 记忆：跨轮前缀哈希（P1-2 执法 + 缓存击穿点定位）+
/// 最近一次观测（/context 域行，P2-6）。
#[derive(Default)]
struct SessionTapMemory {
    prefix: Option<PrefixHashes>,
    last: Option<TapObservation>,
}

fn tap_memory_lock() -> &'static Mutex<HashMap<String, SessionTapMemory>> {
    static CELL: OnceLock<Mutex<HashMap<String, SessionTapMemory>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashMap::new()))
}

fn with_session_memory<T>(session_id: &str, f: impl FnOnce(&mut SessionTapMemory) -> T) -> T {
    let mut guard = tap_memory_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !guard.contains_key(session_id) && guard.len() >= TAP_MEMORY_CAP {
        if let Some(oldest) = guard.keys().next().cloned() {
            guard.remove(&oldest);
        }
    }
    f(guard.entry(session_id.to_string()).or_default())
}

/// 把 System 文本里的 `<memory-context>...</memory-context>` 块切出来。
/// 返回（静态余部, 记忆块拼接）。记忆字节 = 块原文长度（防双算规则，
/// 见模块文档"记忆行落位规则"）。
fn split_memory_block(system_text: &str) -> (String, String) {
    let mut static_out = String::with_capacity(system_text.len());
    let mut memory_out = String::new();
    let mut rest = system_text;
    while let Some(start) = rest.find(MEMORY_CONTEXT_OPEN_TAG) {
        static_out.push_str(&rest[..start]);
        let after_open = &rest[start + MEMORY_CONTEXT_OPEN_TAG.len()..];
        match after_open.find(MEMORY_CONTEXT_CLOSE_TAG) {
            Some(end) => {
                let close_end = end + MEMORY_CONTEXT_CLOSE_TAG.len();
                memory_out.push_str(MEMORY_CONTEXT_OPEN_TAG);
                memory_out.push_str(&after_open[..close_end]);
                rest = &after_open[close_end..];
            }
            // 未闭合块：保守归静态——把块起点起的余部一次性并入静态，
            // rest 清空防止循环后的收尾 push 重复拼接。
            None => {
                static_out.push_str(&rest[start..]);
                rest = "";
                break;
            }
        }
    }
    static_out.push_str(rest);
    (static_out, memory_out)
}

/// 不变量 1：稳定段禁时间戳（ISO 日期时间形态）。
fn contains_volatile_timestamp(text: &str) -> bool {
    let b = text.as_bytes();
    let n = b.len();
    if n < 16 {
        return false;
    }
    let is_digits = |s: &[u8]| s.iter().all(u8::is_ascii_digit);
    for i in 0..=(n - 16) {
        if is_digits(&b[i..i + 4])
            && b[i + 4] == b'-'
            && is_digits(&b[i + 5..i + 7])
            && b[i + 7] == b'-'
            && is_digits(&b[i + 8..i + 10])
        {
            let j = i + 10;
            if j + 6 <= n
                && (b[j] == b'T' || b[j] == b' ')
                && is_digits(&b[j + 1..j + 3])
                && b[j + 3] == b':'
                && is_digits(&b[j + 4..j + 6])
            {
                return true;
            }
        }
    }
    false
}

/// 不变量 2：稳定段禁 nonce（≥16 位十六进制连串）。
fn contains_hex_nonce(text: &str) -> bool {
    let mut run = 0usize;
    for c in text.chars() {
        if c.is_ascii_hexdigit() {
            run += 1;
            if run >= 16 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

// ============================================================================
// 会话种类（P1-4：事件必须带 session-kind 标签，否则子代理全量 defs 污染主回路归因）
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionKind {
    Main,
    Subagent,
    Compaction,
}

impl SessionKind {
    fn as_str(self) -> &'static str {
        match self {
            SessionKind::Main => "main",
            SessionKind::Subagent => "subagent",
            SessionKind::Compaction => "compaction",
        }
    }
}

// ============================================================================
// 观测结构（事件 JSON 形态，camelCase；全量克隆入注册表供 /context 复用）
// ============================================================================

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ByteTokens {
    pub bytes: u64,
    pub est_tokens: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolDefsSection {
    pub bytes: u64,
    pub est_tokens: u64,
    pub tool_count: usize,
    /// hosted_tools（backend-search 启用时上线的 hosted defs）子行。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hosted_tools: Option<ByteTokens>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HistorySection {
    pub bytes: u64,
    pub est_tokens: u64,
    pub item_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Sections {
    pub system_prompt_static: ByteTokens,
    pub system_prompt_dynamic_memory: ByteTokens,
    pub agents_md: ByteTokens,
    pub builtin_tool_defs: ToolDefsSection,
    pub conversation_history: HistorySection,
    pub unattributed_residual: ByteTokens,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecentToolResultsLens {
    pub bytes: u64,
    pub est_tokens: u64,
    /// 计入透镜的条数（= min(5, 总数)）。
    pub tool_result_count: usize,
    pub total_tool_results: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttachmentsLens {
    pub bytes: u64,
    pub est_tokens: u64,
    pub image_count: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillListingLens {
    pub bytes: u64,
    pub est_tokens: u64,
    pub skill_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpServerRow {
    pub name: String,
    pub tool_count: usize,
    pub bytes: u64,
    pub est_tokens: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpServersLens {
    pub total_bytes: u64,
    pub total_est_tokens: u64,
    pub servers: Vec<McpServerRow>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Lenses {
    pub recent5_tool_results: RecentToolResultsLens,
    pub attachments: AttachmentsLens,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_listing: Option<SkillListingLens>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<McpServersLens>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrefixHashOut {
    pub system: String,
    pub tool_defs: String,
    pub agents_md: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_listing: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrefixChangedOut {
    pub system: bool,
    pub tool_defs: bool,
    pub agents_md: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_listing: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InvariantsOut {
    pub no_timestamp_in_stable_prefix: bool,
    pub no_nonce_in_stable_prefix: bool,
    /// 名字集合不变时 wire 顺序是否不变（跨轮；首轮 None）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_order_stable_vs_prev_turn: Option<bool>,
    /// sorted names 集合是否与上一轮一致（首轮 None）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_set_stable_vs_prev_turn: Option<bool>,
    pub violations: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TapObservation {
    pub session_kind: &'static str,
    pub session_id: String,
    pub req_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub sections: Sections,
    pub lenses: Lenses,
    pub prefix_hash: PrefixHashOut,
    /// 跨轮前缀对比（P1-2 执法 + 缓存击穿点定位）。compaction 轮不参与
    /// 主/子代理前缀序列，恒 None。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix_changed_vs_prev_turn: Option<PrefixChangedOut>,
    pub invariants: InvariantsOut,
    pub calibration: &'static str,
}

impl TapObservation {
    /// Σ(分区行 est_tokens)——对账分子（透镜行不进 Σ，防双算）。
    pub fn est_total_tokens(&self) -> u64 {
        let s = &self.sections;
        s.system_prompt_static.est_tokens
            + s.system_prompt_dynamic_memory.est_tokens
            + s.agents_md.est_tokens
            + s.builtin_tool_defs.est_tokens
            + s.builtin_tool_defs.hosted_tools.as_ref().map_or(0, |h| h.est_tokens)
            + s.conversation_history.est_tokens
            + s.unattributed_residual.est_tokens
    }
}

/// 注册表内部的前缀哈希（含跨轮对比所需的辅助序列）。
#[derive(Debug, Clone)]
struct PrefixHashes {
    system: String,
    tool_defs: String,
    agents_md: String,
    skill_listing: Option<String>,
    /// defs wire 顺序名序列（顺序抖动检测）。
    wire_names: Vec<String>,
    /// defs sorted 名序列（集合稳定检测）。
    sorted_names: Vec<String>,
}

/// 一次观测的输入（结构体打包，避开过长参数列表）。
pub(crate) struct ObserveInputs<'a> {
    pub kind: SessionKind,
    pub session_id: &'a str,
    pub req_id: &'a str,
    pub model: Option<&'a str>,
    pub items: &'a [ConversationItem],
    pub tools: &'a [ToolSpec],
    pub hosted_tools: &'a [HostedTool],
    /// (清单正文, 技能数)——SkillListingSnapshot 的实际载荷。
    pub skill_listing: Option<(&'a str, usize)>,
    /// MCP 全量公告文本（build_server_reminder + hint；无连接时 None）。
    pub mcp_announcement: Option<&'a str>,
    /// 每 server 一行的估算输入（server 级粒度）。
    pub mcp_servers: &'a [McpServerRow],
}

/// tap 核心：对一次组装完毕的请求做 8 类分账 + 前缀哈希 + 不变量执法。
/// 纯函数（注册表交互仅限 Main/Subagent 的跨轮哈希对比），同步、O(prompt)。
pub(crate) fn observe_payload(input: ObserveInputs<'_>) -> TapObservation {
    // ── 透镜预置：近 5 条 ToolResult 的位置集合 ──
    let tr_positions: Vec<usize> = input
        .items
        .iter()
        .enumerate()
        .filter(|(_, item)| matches!(item, ConversationItem::ToolResult(_)))
        .map(|(idx, _)| idx)
        .collect();
    let total_tool_results = tr_positions.len();
    let recent5: std::collections::HashSet<usize> = tr_positions
        .iter()
        .rev()
        .take(5)
        .copied()
        .collect();

    // ── items 分区：每 item 恰落入一个分区行（bytes=线上字节，tokens=生产估算） ──
    let mut system_static_bytes = 0u64;
    let mut system_static_tokens = 0u64;
    let mut memory_bytes = 0u64;
    let mut memory_tokens = 0u64;
    let mut agents_bytes = 0u64;
    let mut agents_tokens = 0u64;
    let mut history_bytes = 0u64;
    let mut history_tokens = 0u64;
    let mut history_items = 0usize;
    let mut attributed_item_bytes = 0u64;
    let mut recent5_bytes = 0u64;
    let mut recent5_tokens = 0u64;
    let mut recent5_count = 0usize;
    let mut attach_bytes = 0u64;
    let mut attach_images = 0u64;
    let mut system_static_text = String::new();
    let mut agents_text = String::new();

    for (idx, item) in input.items.iter().enumerate() {
        let wire = serialized_json_bytes(item) as u64;
        attributed_item_bytes += wire;
        match item {
            ConversationItem::System(s) => {
                let (static_text, mem_block) = split_memory_block(&s.content);
                system_static_text.push_str(static_text.trim_end());
                system_static_tokens += estimate_tokens_aware(&static_text);
                system_static_bytes += wire.saturating_sub(mem_block.len() as u64);
                if !mem_block.is_empty() {
                    memory_bytes += mem_block.len() as u64;
                    memory_tokens += estimate_tokens_aware(&mem_block);
                }
            }
            ConversationItem::User(u) => {
                if is_project_instructions(item) {
                    agents_bytes += wire;
                    agents_tokens += estimate_item_tokens(item);
                    if let Some(ContentPart::Text { text }) = u.content.first() {
                        agents_text.push_str(text.as_ref());
                    }
                } else {
                    history_bytes += wire;
                    history_tokens += estimate_item_tokens(item);
                    history_items += 1;
                }
                for part in &u.content {
                    if let ContentPart::Image { url } = part {
                        attach_bytes += url.len() as u64;
                        attach_images += 1;
                    }
                }
            }
            ConversationItem::ToolResult(tr) => {
                history_bytes += wire;
                // TR 内联图（read_file 读图/PDF）确凿上 provider 线
                // （conversation.rs:1100-1104 直嵌 tool result message），
                // 对账分子必须含其图片 token——漏计 = drift 系统性 −765×N
                // 假阳性（R10-B2 修复；与 estimate_item_tokens(User) 的
                // 图片处理对称）。
                history_tokens += estimate_item_tokens(item)
                    + estimate_image_tokens(tr.images.len() as u64);
                history_items += 1;
                if recent5.contains(&idx) {
                    recent5_bytes += wire;
                    recent5_tokens += estimate_item_tokens(item);
                    recent5_count += 1;
                }
                for part in &tr.images {
                    if let ContentPart::Image { url } = part {
                        attach_bytes += url.len() as u64;
                        attach_images += 1;
                    }
                }
            }
            // Assistant / BackendToolCall / Reasoning / 其余合成 user 项
            _ => {
                history_bytes += wire;
                history_tokens += estimate_item_tokens(item);
                history_items += 1;
            }
        }
    }

    // ── tools：字节=逐工具线上字节；tokens=生产共享口径 estimate_tool_spec_tokens ──
    let mut tools_bytes = 0u64;
    let mut tools_tokens = 0u64;
    let mut defs_fingerprint = String::with_capacity(1024);
    let mut wire_names: Vec<String> = Vec::with_capacity(input.tools.len());
    for t in input.tools {
        tools_bytes += serialized_json_bytes(t) as u64;
        tools_tokens += estimate_tool_spec_tokens(t);
        wire_names.push(t.name.clone());
        defs_fingerprint.push_str(&t.name);
        defs_fingerprint.push('\n');
        defs_fingerprint.push_str(t.description.as_deref().unwrap_or(""));
        defs_fingerprint.push('\n');
        defs_fingerprint.push_str(&t.parameters.to_string());
        defs_fingerprint.push('\n');
    }
    let hosted_bytes = input
        .hosted_tools
        .iter()
        .map(|h| {
            // HostedTool 无 serde Serialize（provider 客户端层手工注入 JSON），
            // 字节按 wire_name + JSON 信封近似；WebSearch 的域名单另计。
            let mut b = (h.wire_name().len() + 16) as u64;
            if let HostedTool::WebSearch { allowed_domains: Some(domains) } = h {
                b += domains.iter().map(|d| d.len() as u64 + 3).sum::<u64>();
            }
            b
        })
        .sum();
    // hosted defs 为纯 JSON（ASCII 为主），token 估算按 bytes/4——与
    // estimate_tokens_aware 的 ASCII 恒等（同口径，离线校准器后续接）。
    let hosted_tokens = hosted_bytes / BYTES_PER_TOKEN;

    // ── 残差：items 线上总量 − Σ分账字节（满分类学下 ≈ 数组框架字节） ──
    let items_wire_total = conversation_body_bytes(input.items) as u64;
    let residual_bytes = items_wire_total.saturating_sub(attributed_item_bytes);
    let residual_tokens = residual_bytes / BYTES_PER_TOKEN;

    // ── 透镜 3/4/6/7 ──
    let skill_lens = input.skill_listing.map(|(text, count)| SkillListingLens {
        bytes: text.len() as u64,
        est_tokens: estimate_tokens_aware(text),
        skill_count: count,
    });
    let mcp_lens = input.mcp_announcement.map(|text| McpServersLens {
        total_bytes: text.len() as u64,
        total_est_tokens: estimate_tokens_aware(text),
        servers: input.mcp_servers.to_vec(),
    });

    // ── 前缀哈希 ──
    let system_hash = sha256_hex(system_static_text.as_bytes());
    let defs_hash = sha256_hex(defs_fingerprint.as_bytes());
    let agents_hash = sha256_hex(agents_text.as_bytes());
    let skills_hash = input.skill_listing.map(|(text, _)| sha256_hex(text.as_bytes()));
    let sorted_names = {
        let mut v = wire_names.clone();
        v.sort();
        v
    };

    // ── 不变量 1/2（自包含，全 kind 通用；扫描域 = 稳定段） ──
    let stable_prefix_text = format!(
        "{}{}",
        system_static_text,
        input.skill_listing.map(|(t, _)| t).unwrap_or("")
    );
    let has_timestamp = contains_volatile_timestamp(&stable_prefix_text);
    let has_nonce = contains_hex_nonce(&stable_prefix_text);
    let mut violations: Vec<String> = Vec::new();
    if has_timestamp {
        violations.push("timestamp_in_stable_prefix".to_string());
    }
    if has_nonce {
        violations.push("nonce_in_stable_prefix".to_string());
    }

    // ── 跨轮对比 + 不变量 3/4（仅 Main/Subagent 参与，防 compaction 重建投毒） ──
    let tracks_prefix = matches!(input.kind, SessionKind::Main | SessionKind::Subagent);
    let (prefix_changed, order_stable, set_stable) = if tracks_prefix {
        with_session_memory(input.session_id, |mem| {
            let cur = PrefixHashes {
                system: system_hash.clone(),
                tool_defs: defs_hash.clone(),
                agents_md: agents_hash.clone(),
                skill_listing: skills_hash.clone(),
                wire_names: wire_names.clone(),
                sorted_names: sorted_names.clone(),
            };
            let prev = mem.prefix.replace(cur);
            match prev {
                Some(p) => {
                    let set_stable = p.sorted_names == sorted_names;
                    let order_stable = p.wire_names == wire_names;
                    if set_stable && !order_stable {
                        violations.push("tool_order_jitter_with_unchanged_set".to_string());
                    }
                    (
                        Some(PrefixChangedOut {
                            system: p.system != system_hash,
                            tool_defs: p.tool_defs != defs_hash,
                            agents_md: p.agents_md != agents_hash,
                            skill_listing: Some(p.skill_listing != skills_hash),
                        }),
                        Some(order_stable),
                        Some(set_stable),
                    )
                }
                None => (None, None, None),
            }
        })
    } else {
        (None, None, None)
    };

    TapObservation {
        session_kind: input.kind.as_str(),
        session_id: input.session_id.to_string(),
        req_id: input.req_id.to_string(),
        model: input.model.map(str::to_string),
        sections: Sections {
            system_prompt_static: ByteTokens { bytes: system_static_bytes, est_tokens: system_static_tokens },
            system_prompt_dynamic_memory: ByteTokens { bytes: memory_bytes, est_tokens: memory_tokens },
            agents_md: ByteTokens { bytes: agents_bytes, est_tokens: agents_tokens },
            builtin_tool_defs: ToolDefsSection {
                bytes: tools_bytes,
                est_tokens: tools_tokens,
                tool_count: input.tools.len(),
                hosted_tools: (!input.hosted_tools.is_empty())
                    .then(|| ByteTokens { bytes: hosted_bytes, est_tokens: hosted_tokens }),
            },
            conversation_history: HistorySection {
                bytes: history_bytes,
                est_tokens: history_tokens,
                item_count: history_items,
            },
            unattributed_residual: ByteTokens { bytes: residual_bytes, est_tokens: residual_tokens },
        },
        lenses: Lenses {
            recent5_tool_results: RecentToolResultsLens {
                bytes: recent5_bytes,
                est_tokens: recent5_tokens,
                tool_result_count: recent5_count,
                total_tool_results,
            },
            attachments: AttachmentsLens {
                bytes: attach_bytes,
                est_tokens: estimate_image_tokens(attach_images),
                image_count: attach_images,
            },
            skill_listing: skill_lens,
            mcp_servers: mcp_lens,
        },
        prefix_hash: PrefixHashOut {
            system: system_hash,
            tool_defs: defs_hash,
            agents_md: agents_hash,
            skill_listing: skills_hash,
        },
        prefix_changed_vs_prev_turn: prefix_changed,
        invariants: InvariantsOut {
            no_timestamp_in_stable_prefix: !has_timestamp,
            no_nonce_in_stable_prefix: !has_nonce,
            tool_order_stable_vs_prev_turn: order_stable,
            tool_set_stable_vs_prev_turn: set_stable,
            violations,
        },
        calibration: CALIBRATION_NOTE,
    }
}

// ============================================================================
// 事件落盘：并入现有 per-request usage/header 事件流（搭便车，采样门内）
// ============================================================================

/// 请求侧产物——同一轮循环内交由 usage 侧汇合（局部变量携带，零结构体改动）。
pub(crate) struct PendingTap {
    observation: TapObservation,
}

fn observation_to_json(observation: &TapObservation) -> serde_json::Value {
    serde_json::to_value(observation).unwrap_or(serde_json::Value::Null)
}

fn reconciliation_json(observation: &TapObservation, usage: &TokenUsage) -> serde_json::Value {
    let est_total = observation.est_total_tokens();
    let drift = i64::from(est_total.min(i64::MAX as u64) as i64) - i64::from(usage.prompt_tokens);
    let provider = f64::from(usage.prompt_tokens);
    let pct = if provider > 0.0 {
        (drift.unsigned_abs() as f64) / provider * 100.0
    } else {
        0.0
    };
    serde_json::json!({
        "estTotalTokens": est_total,
        "providerPromptTokens": usage.prompt_tokens,
        "cachedPromptTokens": usage.cached_prompt_tokens,
        "driftTokens": drift,
        "driftPercent": (pct * 10.0).round() / 10.0,
        "warn": pct > RECONCILIATION_WARN_PCT,
        "interpretation": RECONCILIATION_INTERPRETATION,
    })
}

/// usage 侧汇合点（turn.rs 响应段调用）：并入 provider usage、计算对账
/// 差额（P1-3：告警对象 = Σ+残差−provider_total）、落盘事件、注册表存
/// 最近观测（/context 域行数据源）。
pub(crate) fn complete_turn_with_usage(pending: PendingTap, usage: Option<&TokenUsage>) {
    let session_id = pending.observation.session_id.clone();
    // 最近观测先入注册表（/context 域行数据源）——warn 早退也不丢。
    with_session_memory(&session_id, |mem| mem.last = Some(pending.observation.clone()));
    let mut event = observation_to_json(&pending.observation);
    if let Some(obj) = event.as_object_mut() {
        match usage {
            Some(u) => {
                let recon = reconciliation_json(&pending.observation, u);
                let warn = recon.get("warn").and_then(serde_json::Value::as_bool).unwrap_or(false);
                obj.insert("reconciliation".to_string(), recon);
                if warn {
                    cf_telemetry::unified_log::warn(
                        TURN_TAP_EVENT,
                        Some(&session_id),
                        Some(event),
                    );
                    return;
                }
            }
            None => {
                obj.insert(
                    "reconciliation".to_string(),
                    serde_json::json!({
                        "note": "provider usage absent; sections+prefix-hash only",
                        "estTotalTokens": pending.observation.est_total_tokens(),
                    }),
                );
            }
        }
    }
    cf_telemetry::unified_log::info(TURN_TAP_EVENT, Some(&session_id), Some(event));
}

// ============================================================================
// 主回路 + 子代理 choke point（turn.rs；P1-4：机械上同 choke point 覆盖）
// ============================================================================

use super::SessionActor;

impl SessionActor {
    /// 请求侧观测（在请求全量装配完毕、提交 sampling 之前调用）。
    /// 关闭/未采样时返回 None 且零副作用（0 开销 0 载荷承诺）。
    pub(super) async fn payload_tap_observe_turn_request(
        &self,
        request: &ConversationRequest,
        req_id: &str,
    ) -> Option<PendingTap> {
        let session_id = self.session_info.id.0.as_ref();
        if !should_observe(session_id) {
            return None;
        }
        // 透镜快照仅在 tap 激活时拉取（SkillListingSnapshot / MCP 公告——
        // 结构化源，无字节嗅探；MCP 主回路生产零注入，此处为 /context 同源估算）。
        let skill_listing = self
            .tool_bridge_handle()
            .skill_listing_snapshot()
            .await
            .map(|s| (s.text, s.skill_count));
        let mcp_announcement = self
            .mcp_announcement_snapshot()
            .await
            .map(|a| a.text);
        let mcp_servers: Vec<McpServerRow> = self
            .connected_server_summaries()
            .iter()
            .map(|s| {
                let line = format!(
                    "{} ({} tools): {}",
                    s.name,
                    s.tool_count,
                    s.tool_names.join(", ")
                );
                McpServerRow {
                    name: s.name.clone(),
                    tool_count: s.tool_count,
                    bytes: line.len() as u64,
                    est_tokens: estimate_tokens_aware(&line),
                }
            })
            .collect();
        let kind = if self.startup_hints.is_subagent {
            SessionKind::Subagent
        } else {
            SessionKind::Main
        };
        Some(PendingTap {
            observation: observe_payload(ObserveInputs {
                kind,
                session_id,
                req_id,
                model: request.model.as_deref(),
                items: &request.items,
                tools: &request.tools,
                hosted_tools: &request.hosted_tools,
                skill_listing: skill_listing.as_ref().map(|(t, c)| (t.as_str(), *c)),
                mcp_announcement: mcp_announcement.as_deref(),
                mcp_servers: &mcp_servers,
            }),
        })
    }
}

// ============================================================================
// compaction 路径（session_compact.rs；重宣字节一致性由前缀哈希兜底——
// P2-10：compact_system_prompt 是静态串 cf-agent/src/agent.rs:117-119，
// 分类学静态行叙事断点在此）
// ============================================================================

/// compaction 摘要请求观测（generate_session_compact 入口一处覆盖三后端臂）。
/// 立即落盘（raw stream 路径无 usage 汇合）；不参与主/子代理前缀序列，
/// 不写注册表（防 compaction 重建投毒主回路归因）。
pub(crate) fn observe_compaction_request(
    chat_history: &[ConversationItem],
    tools: &[ToolSpec],
    hosted_tools: &[HostedTool],
    session_id: &str,
    model: Option<&str>,
) {
    if !should_observe(session_id) {
        return;
    }
    let observation = observe_payload(ObserveInputs {
        kind: SessionKind::Compaction,
        session_id,
        req_id: "compaction",
        model,
        items: chat_history,
        tools,
        hosted_tools,
        skill_listing: None,
        mcp_announcement: None,
        mcp_servers: &[],
    });
    let mut event = observation_to_json(&observation);
    if let Some(obj) = event.as_object_mut() {
        obj.insert(
            "reconciliation".to_string(),
            serde_json::json!({
                "estTotalTokens": observation.est_total_tokens(),
                "providerPromptTokens": serde_json::Value::Null,
                "note": "compaction request: provider usage not joined (raw stream path); re-announce byte-stability is watched via main-loop prefix hash",
            }),
        );
    }
    cf_telemetry::unified_log::info(COMPACT_TAP_EVENT, Some(session_id), Some(event));
}

// ============================================================================
// /context 域行（P2-6：TokenUsageCategory→ContextInfo.usage_categories→LegendRow
// 管道全通；tap 未跑过 = 零行 = 零行为变更）
// ============================================================================

/// 从注册表最近一次主/子代理观测生成 /context 域行（cf-pager 泛化渲染，
/// cf-pager 零改动）。
pub(crate) fn context_rows(session_id: &str) -> Vec<TokenUsageCategory> {
    let last = with_session_memory(session_id, |mem| mem.last.clone());
    let Some(obs) = last else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    if obs.sections.agents_md.bytes > 0 {
        rows.push(TokenUsageCategory {
            label: "Project instructions (AGENTS.md)".to_string(),
            tokens: obs.sections.agents_md.est_tokens,
            detail: Some(format!("~{} bytes", obs.sections.agents_md.bytes)),
        });
    }
    rows.push(TokenUsageCategory {
        label: "Recent tool results (last 5)".to_string(),
        tokens: obs.lenses.recent5_tool_results.est_tokens,
        detail: Some(format!(
            "{} of {} results",
            obs.lenses.recent5_tool_results.tool_result_count,
            obs.lenses.recent5_tool_results.total_tool_results
        )),
    });
    if obs.lenses.attachments.image_count > 0 {
        rows.push(TokenUsageCategory {
            label: "Attachments (inline images)".to_string(),
            tokens: obs.lenses.attachments.est_tokens,
            detail: Some(format!("{} images", obs.lenses.attachments.image_count)),
        });
    }
    rows.push(TokenUsageCategory {
        label: "Payload residual".to_string(),
        tokens: obs.sections.unattributed_residual.est_tokens,
        detail: Some(format!("~{} bytes unattributed", obs.sections.unattributed_residual.bytes)),
    });
    rows
}

// ============================================================================
// golden 钉死测试（固定组装输入 → 期望 section 字节数；断言带 section 名失败消息）
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use cf_sampling_types::conversation::{AssistantItem, ToolResultItem};
    use serde_json::json;
    use std::sync::Arc;

    fn sys(text: &str) -> ConversationItem {
        ConversationItem::system(text.to_string())
    }
    fn user(text: &str) -> ConversationItem {
        ConversationItem::user(text.to_string())
    }
    fn agents_item(text: &str) -> ConversationItem {
        ConversationItem::project_instructions(text.to_string())
    }
    fn tool_result(id: &str, content: &str) -> ConversationItem {
        ConversationItem::ToolResult(ToolResultItem {
            tool_call_id: id.to_string(),
            content: Arc::<str>::from(content),
            images: vec![],
        })
    }
    fn tool_result_with_image(id: &str, content: &str, url: &str) -> ConversationItem {
        ConversationItem::ToolResult(ToolResultItem {
            tool_call_id: id.to_string(),
            content: Arc::<str>::from(content),
            images: vec![ContentPart::Image { url: Arc::<str>::from(url) }],
        })
    }
    fn assistant(text: &str) -> ConversationItem {
        ConversationItem::Assistant(AssistantItem {
            content: Arc::<str>::from(text),
            tool_calls: vec![],
            model_id: None,
            model_fingerprint: None,
            reasoning_effort: None,
        })
    }
    fn user_with_image(text: &str, url: &str) -> ConversationItem {
        ConversationItem::User(cf_sampling_types::conversation::UserItem {
            content: vec![
                ContentPart::Text { text: Arc::<str>::from(text) },
                ContentPart::Image { url: Arc::<str>::from(url) },
            ],
            synthetic_reason: None,
            prior_turn_interrupt: None,
            prompt_index: None,
        })
    }
    fn spec(name: &str, desc: &str, params: serde_json::Value) -> ToolSpec {
        ToolSpec {
            name: name.to_string(),
            description: Some(desc.to_string()),
            parameters: params,
        }
    }
    fn observe<'a>(
        kind: SessionKind,
        session_id: &'a str,
        items: &'a [ConversationItem],
        tools: &'a [ToolSpec],
    ) -> TapObservation {
        observe_payload(ObserveInputs {
            kind,
            session_id,
            req_id: "req-test",
            model: Some("test-model"),
            items,
            tools,
            hosted_tools: &[],
            skill_listing: None,
            mcp_announcement: None,
            mcp_servers: &[],
        })
    }

    #[test]
    fn parse_tap_mode_modes() {
        assert_eq!(parse_tap_mode(None), TapMode::Off);
        assert_eq!(parse_tap_mode(Some("")), TapMode::Off);
        assert_eq!(parse_tap_mode(Some("off")), TapMode::Off);
        assert_eq!(parse_tap_mode(Some("sample")), TapMode::Sampled1Pct);
        assert_eq!(parse_tap_mode(Some("1")), TapMode::Sampled1Pct);
        assert_eq!(parse_tap_mode(Some("full")), TapMode::Full);
        assert_eq!(parse_tap_mode(Some("all")), TapMode::Full);
        assert_eq!(parse_tap_mode(Some(" on ")), TapMode::Full);
    }
    #[test]
    fn split_memory_block_extracts_and_keeps_static() {
        let (static_text, mem) = split_memory_block("base<memory-context>hello</memory-context>tail");
        assert_eq!(static_text, "basetail");
        assert_eq!(mem, "<memory-context>hello</memory-context>");
        let (s2, m2) = split_memory_block("no block here");
        assert_eq!(s2, "no block here");
        assert!(m2.is_empty());
        // 未闭合块保守归静态。
        let (s3, m3) = split_memory_block("x<memory-context>open");
        assert_eq!(s3, "x<memory-context>open");
        assert!(m3.is_empty());
    }

    #[test]
    fn invariant_detector_timestamp() {
        assert!(contains_volatile_timestamp("logged 2026-10-17T10:30 ok"));
        assert!(contains_volatile_timestamp("at 2026-10-17 10:30:15Z"));
        // 纯日期（无时间成分）不触发——会话级常量日期不算每轮抖动。
        assert!(!contains_volatile_timestamp("date 2026-10-17 only"));
        assert!(!contains_volatile_timestamp("no digits here"));
    }

    #[test]
    fn invariant_detector_hex_nonce() {
        assert!(contains_hex_nonce("id abcdef0123456789 end"));
        assert!(contains_hex_nonce("ABCDEF0123456789"));
        assert!(!contains_hex_nonce("short abcdef01234567"));
        assert!(!contains_hex_nonce("plain text with 1234"));
    }

    /// 黄金钉死：固定 ASCII 组装 → 逐 section 精确字节/估算（断言带 section 名）。
    /// 常量为 serde 实测钉死（2026-10-18 校准轮，全部机器实证）：
    /// items: [System(41B), AGENTS(106B), User(55B), ToolResult(59B), Assistant(35B)]
    /// tools: ToolSpec{"t1","abcd",{"a":1}} → 55B / 2 tok（serde 实测）
    /// 残差 = 数组框架（2 括号 + 4 逗号 = 6B）——证明分账无重叠无遗漏。
    #[test]
    fn golden_full_partition_exact_bytes_and_tokens() {
        let items = vec![
            sys("hello world"),
            agents_item("AGENTS body"),
            user("hi"),
            tool_result("c1", "abcd"),
            assistant("ok"),
        ];
        let tools = vec![spec("t1", "abcd", json!({"a": 1}))];
        let obs = observe(SessionKind::Main, "tap-golden", &items, &tools);
        let s = &obs.sections;
        assert_eq!(s.system_prompt_static.bytes, 41, "systemPromptStatic bytes");
        assert_eq!(s.system_prompt_static.est_tokens, 2, "systemPromptStatic estTokens (11 ascii bytes / 4)");
        assert_eq!(s.system_prompt_dynamic_memory.bytes, 0, "systemPromptDynamicMemory bytes");
        assert_eq!(s.agents_md.bytes, 106, "agentsMd bytes (serde 实测)");
        assert_eq!(s.agents_md.est_tokens, 2, "agentsMd estTokens");
        assert_eq!(s.builtin_tool_defs.bytes, 55, "builtinToolDefs bytes (serde 实测)");
        assert_eq!(s.builtin_tool_defs.est_tokens, 2, "builtinToolDefs estTokens (shared estimate_tool_spec_tokens caliber, serde 实测)");
        assert_eq!(s.builtin_tool_defs.tool_count, 1, "builtinToolDefs toolCount");
        assert!(s.builtin_tool_defs.hosted_tools.is_none(), "hostedTools absent when empty");
        assert_eq!(s.conversation_history.bytes, 149, "conversationHistory bytes (55+59+35, serde 实测)");
        assert_eq!(s.conversation_history.item_count, 3, "conversationHistory itemCount");
        assert_eq!(s.unattributed_residual.bytes, 6, "unattributedResidual bytes = JSON array framing only");
        assert_eq!(s.unattributed_residual.est_tokens, 1, "unattributedResidual estTokens (6/4)");
        // 透镜：近 5 条 TR（本例 1 条）。
        assert_eq!(obs.lenses.recent5_tool_results.bytes, 59, "recent5ToolResults bytes");
        assert_eq!(obs.lenses.recent5_tool_results.tool_result_count, 1, "recent5ToolResults count");
        assert_eq!(obs.lenses.recent5_tool_results.total_tool_results, 1, "totalToolResults");
        assert_eq!(obs.lenses.attachments.image_count, 0, "attachments images");
        // Σ（分区行含 hosted/residual）= est_total。
        assert_eq!(obs.est_total_tokens(), 8, "estTotalTokens sum (2+0+2+2+1+1)");
        // session-kind 标签（P1-4）。
        assert_eq!(obs.session_kind, "main", "sessionKind tag");
    }
    /// 记忆行落位（防双算）：`<memory-context>` 块计 memory 行，
    /// systemPromptStatic 扣除同块（无换行 → 无转义差，字节数精确）。
    #[test]
    fn golden_memory_block_split_no_double_count() {
        let items = vec![sys("base<memory-context>mem</memory-context>")];
        let obs = observe(SessionKind::Main, "tap-mem", &items, &[]);
        let s = &obs.sections;
        // wire = {"type":"system","content":"base<memory-context>mem</memory-context>"} = 70B（serde 实测）
        assert_eq!(s.system_prompt_static.bytes, 34, "systemPromptStatic bytes = wire(70) - block(36)");
        assert_eq!(s.system_prompt_static.est_tokens, 1, "systemPromptStatic estTokens (4 ascii bytes / 4)");
        assert_eq!(s.system_prompt_dynamic_memory.bytes, 36, "systemPromptDynamicMemory bytes = block text");
        assert_eq!(s.system_prompt_dynamic_memory.est_tokens, 9, "systemPromptDynamicMemory estTokens (36/4)");
        // 分账无遗漏：static + memory + 框架 = wire 总量。
        assert_eq!(
            s.system_prompt_static.bytes + s.system_prompt_dynamic_memory.bytes
                + s.unattributed_residual.bytes,
            70 + 2,
            "partition covers wire + framing exactly"
        );
    }

    #[test]
    fn attachments_lens_counts_user_and_tool_result_images() {
        let items = vec![
            user_with_image("look", "AAAA"),
            tool_result_with_image("c1", "r", "BBBB"),
            sys("s"),
        ];
        let obs = observe(SessionKind::Main, "tap-att", &items, &[]);
        assert_eq!(obs.lenses.attachments.image_count, 2, "attachments images (user + tool result)");
        assert_eq!(obs.lenses.attachments.bytes, 8, "attachments bytes = raw url lens");
        assert_eq!(
            obs.lenses.attachments.est_tokens,
            2 * cf_token_estimation::IMAGE_TOKEN_ESTIMATE,
            "attachments estTokens = per-image constant"
        );
    }

    /// R10-B2 回归：带图 ToolResult 的 history 分区行必须含图片 token
    /// （对账分子修复）。TR 内联图确凿上 provider 线，漏计时 est_total
    /// 对 provider prompt_tokens 系统性偏低 −765×N → 15% 阈假阳性 warn。
    #[test]
    fn tool_result_images_count_into_history_partition_tokens() {
        let items = vec![
            user("q"),
            tool_result_with_image("c1", "result body", "BBBB"),
            tool_result("c2", "result body"),
        ];
        let obs = observe(SessionKind::Main, "tap-b2", &items, &[]);
        let s = &obs.sections;
        // history est_tokens = Σ content-only 估算 + 1 张 TR 图的 per-image 常量。
        let content_only: u64 = items
            .iter()
            .map(|i| match i {
                ConversationItem::ToolResult(tr) => estimate_tokens_aware(tr.content.as_ref()),
                _ => estimate_item_tokens(i),
            })
            .sum();
        assert_eq!(
            s.conversation_history.est_tokens,
            content_only + cf_token_estimation::IMAGE_TOKEN_ESTIMATE,
            "history partition must include tool-result image tokens (B2)"
        );
        // attachments 透镜（per-image 常量）与 history 分区行恢复子集语义。
        assert_eq!(
            obs.lenses.attachments.est_tokens,
            cf_token_estimation::IMAGE_TOKEN_ESTIMATE,
            "attachments lens tokens must mirror the same image"
        );
        // 对账恒等式：Σ(分区行，现含 TR 图) = est_total（透镜行不进 Σ）。
        assert_eq!(
            obs.est_total_tokens(),
            s.system_prompt_static.est_tokens
                + s.system_prompt_dynamic_memory.est_tokens
                + s.agents_md.est_tokens
                + s.builtin_tool_defs.est_tokens
                + s.conversation_history.est_tokens
                + s.unattributed_residual.est_tokens,
            "reconciliation numerator now includes tool-result images"
        );
    }

    #[test]
    fn recent5_lens_takes_only_last_five_tool_results() {
        let items: Vec<ConversationItem> = (0..7)
            .map(|i| tool_result(&format!("c{i}"), "x"))
            .collect();
        let obs = observe(SessionKind::Main, "tap-r5", &items, &[]);
        let l = &obs.lenses.recent5_tool_results;
        assert_eq!(l.total_tool_results, 7, "totalToolResults");
        assert_eq!(l.tool_result_count, 5, "recent5 count capped at 5");
        // 每条 ToolResult {"type":"tool_result","tool_call_id":"cN","content":"x"} = 56B（serde 实测）。
        assert_eq!(l.bytes, 5 * 56, "recent5 bytes = last five wire sizes");
        assert!(obs.sections.conversation_history.bytes > l.bytes, "recent5 is a subset lens of history");
    }

    /// 前缀稳定性（P1-2 执法）：同输入跨轮哈希不变；内容变 → tool_defs changed
    /// 但顺序稳定不误报；顺序抖动（集合不变顺序变）→ 违例。
    #[test]
    fn prefix_hash_stability_and_invariant_enforcement() {
        let sid = "tap-prefix";
        let items = vec![sys("stable"), user("u")];
        let tools_a = vec![spec("t1", "d1", json!({"x": 1})), spec("t2", "d2", json!({"y": 2}))];
        let obs1 = observe(SessionKind::Main, sid, &items, &tools_a);
        assert!(obs1.prefix_changed_vs_prev_turn.is_none(), "first turn: no comparison baseline");
        let obs2 = observe(SessionKind::Main, sid, &items, &tools_a);
        let ch = obs2.prefix_changed_vs_prev_turn.expect("second turn must compare");
        assert!(!ch.system && !ch.tool_defs && !ch.agents_md, "identical input: prefix unchanged");
        assert_eq!(obs2.invariants.tool_order_stable_vs_prev_turn, Some(true), "order stable");
        assert_eq!(obs2.invariants.tool_set_stable_vs_prev_turn, Some(true), "set stable");
        assert!(obs2.invariants.violations.is_empty(), "no violations on steady state");

        // 内容变化（集合/顺序不变）：tool_defs changed，但不得误报顺序抖动。
        let tools_b = vec![spec("t1", "d1-EDITED", json!({"x": 1})), spec("t2", "d2", json!({"y": 2}))];
        let obs3 = observe(SessionKind::Main, sid, &items, &tools_b);
        let ch3 = obs3.prefix_changed_vs_prev_turn.expect("third turn");
        assert!(ch3.tool_defs, "defs content change detected");
        assert_eq!(obs3.invariants.tool_order_stable_vs_prev_turn, Some(true), "content-only change is NOT order jitter");
        assert!(obs3.invariants.violations.is_empty(), "no violation for legit defs edit");

        // 顺序抖动（集合不变、wire 顺序变）→ 违例。
        let tools_c = vec![spec("t2", "d2", json!({"y": 2})), spec("t1", "d1-EDITED", json!({"x": 1}))];
        let obs4 = observe(SessionKind::Main, sid, &items, &tools_c);
        assert_eq!(obs4.invariants.tool_set_stable_vs_prev_turn, Some(true), "set unchanged");
        assert_eq!(obs4.invariants.tool_order_stable_vs_prev_turn, Some(false), "order jittered");
        assert!(
            obs4.invariants.violations.iter().any(|v| v == "tool_order_jitter_with_unchanged_set"),
            "order jitter must be enforced as violation"
        );
    }

    /// compaction 不参与主/子代理前缀序列（防重建投毒主回路归因）。
    #[test]
    fn compaction_does_not_poison_main_loop_prefix() {
        let sid = "tap-poison";
        let main_items = vec![sys("main-prompt"), user("q")];
        let obs1 = observe(SessionKind::Main, sid, &main_items, &[]);
        assert!(obs1.prefix_changed_vs_prev_turn.is_none(), "first main turn baseline");
        let compact_items = vec![sys("totally-different-compact-history")];
        let comp = observe(SessionKind::Compaction, sid, compact_items.as_slice(), &[]);
        assert!(comp.prefix_changed_vs_prev_turn.is_none(), "compaction never compares");
        let obs2 = observe(SessionKind::Main, sid, &main_items, &[]);
        let ch = obs2.prefix_changed_vs_prev_turn.expect("main resume comparison");
        assert!(!ch.system, "compaction must not poison the main-loop prefix state");
        assert_eq!(comp.session_kind, "compaction", "sessionKind tag");
    }

    /// 稳定段内时间戳/nonce → 违例（P1-2：执法不悬空）。
    #[test]
    fn stable_prefix_invariants_flag_volatile_content() {
        let sid_t = "tap-inv-ts";
        let items_t = vec![sys("hello 2026-10-17T10:00 world")];
        let o1 = observe(SessionKind::Main, sid_t, &items_t, &[]);
        assert!(!o1.invariants.no_timestamp_in_stable_prefix, "timestamp must be flagged");
        assert!(o1.invariants.violations.iter().any(|v| v == "timestamp_in_stable_prefix"));

        let sid_n = "tap-inv-nonce";
        let items_n = vec![sys("key=abcdef0123456789aa end")];
        let o2 = observe(SessionKind::Main, sid_n, &items_n, &[]);
        assert!(!o2.invariants.no_nonce_in_stable_prefix, "nonce must be flagged");
        assert!(o2.invariants.violations.iter().any(|v| v == "nonce_in_stable_prefix"));
    }

    /// 对账数学（P1-3：告警对象 = Σ+残差−provider_total，阈值 15%）。
    #[test]
    fn reconciliation_math_threshold() {
        let obs = TapObservation {
            session_kind: "main",
            session_id: "s".into(),
            req_id: "r".into(),
            model: None,
            sections: Sections {
                system_prompt_static: ByteTokens { bytes: 0, est_tokens: 100 },
                system_prompt_dynamic_memory: ByteTokens { bytes: 0, est_tokens: 0 },
                agents_md: ByteTokens { bytes: 0, est_tokens: 0 },
                builtin_tool_defs: ToolDefsSection {
                    bytes: 0, est_tokens: 200, tool_count: 1, hosted_tools: None,
                },
                conversation_history: HistorySection { bytes: 0, est_tokens: 700, item_count: 3 },
                unattributed_residual: ByteTokens { bytes: 0, est_tokens: 10 },
            },
            lenses: Lenses {
                recent5_tool_results: RecentToolResultsLens {
                    bytes: 0, est_tokens: 0, tool_result_count: 0, total_tool_results: 0,
                },
                attachments: AttachmentsLens::default(),
                skill_listing: None,
                mcp_servers: None,
            },
            prefix_hash: PrefixHashOut {
                system: String::new(), tool_defs: String::new(), agents_md: String::new(),
                skill_listing: None,
            },
            prefix_changed_vs_prev_turn: None,
            invariants: InvariantsOut {
                no_timestamp_in_stable_prefix: true,
                no_nonce_in_stable_prefix: true,
                tool_order_stable_vs_prev_turn: None,
                tool_set_stable_vs_prev_turn: None,
                violations: vec![],
            },
            calibration: CALIBRATION_NOTE,
        };
        assert_eq!(obs.est_total_tokens(), 1010, "estTotal = Σ partition rows (lenses excluded)");
        let usage_ok = TokenUsage {
            prompt_tokens: 1000,
            completion_tokens: 0,
            total_tokens: 1000,
            reasoning_tokens: 0,
            cached_prompt_tokens: 0,
        };
        let recon = reconciliation_json(&obs, &usage_ok);
        assert_eq!(recon["driftTokens"], json!(10), "drift = 1010 - 1000");
        assert_eq!(recon["warn"], json!(false), "1% drift < 15% threshold");
        let usage_bad = TokenUsage { prompt_tokens: 800, ..usage_ok.clone() };
        let recon_bad = reconciliation_json(&obs, &usage_bad);
        assert_eq!(recon_bad["driftTokens"], json!(210), "drift = 1010 - 800");
        assert_eq!(recon_bad["warn"], json!(true), "26% drift > 15% threshold");
    }

    /// /context 域行（P2-6）：观测存入注册表后输出域行；未观测 = 零行。
    #[test]
    fn context_rows_from_last_observation() {
        assert!(context_rows("tap-ctx-absent").is_empty(), "no observation -> zero rows");
        let sid = "tap-ctx";
        let items = vec![
            sys("prompt"),
            agents_item("AGENTS body"),
            user("hi"),
            tool_result("c1", "abcd"),
        ];
        let obs = observe(SessionKind::Main, sid, &items, &[]);
        with_session_memory(sid, |mem| mem.last = Some(obs));
        let rows = context_rows(sid);
        assert!(rows.iter().any(|r| r.label.contains("AGENTS.md")), "AGENTS.md domain row present");
        assert!(rows.iter().any(|r| r.label.contains("Recent tool results")), "recent-5 domain row present");
        assert!(rows.iter().any(|r| r.label == "Payload residual"), "residual domain row present");
        assert!(!rows.iter().any(|r| r.label.contains("Attachments")), "attachments row suppressed when zero images");
    }
}
