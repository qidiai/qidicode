//! P0-2 第二步 Phase A 验收测试（设计稿
//! `docs/token-optimization/P0-2第二步-动态工具管线设计稿.md`
//! §6 Phase A 验收标准 A-1~A-4 + 契约测试 C-4/C-7）。
//!
//! Phase A 语义锚点（设计稿 A-3 + Sonnet 2.12）：
//! `dynamic_tools` 总开关骨架已就位，**默认 false**；
//! Phase A 阶段 `true` = **no-op + warn**（切分实现属
//! Phase B）——组装点 `prepare_tool_definitions_inner`
//! 走单一代码路径，两种极性的 defs 输出字节级一致。
//!
//! 这些测试钉住三件事：
//! 1. **空操作定调（A-3）**：flag 两极性 defs 字节级一致，
//!    且等于组装点直出（bridge 全量 builtins + plan 恒等
//!    过滤）——即现状基线本身；
//! 2. **旗标纪律（A-4）**：flag 在会话启动时一次性读取并
//!    快照，中途配置翻转不影响当次会话 defs；
//! 3. **契约（C-4 条件版 / C-7）**：meta 二元（注册后）
//!    在任何裁剪配置下不被移除、plan 对恒在（无条件
//!    钉槽是 Phase B 投影行为，Q2-plan）、plan 进出
//!    defs 字节级不变。

use super::support::*;
use super::*;

/// defs 的字节级快照（serde JSON 串）——A-3/C-7 的
/// “字节级一致”断言载体。`ToolDefinition` 派生
/// `Serialize`，序列化串即 wire 形态。
fn defs_fingerprint(defs: &[cf_tools::types::definition::ToolDefinition]) -> String {
    serde_json::to_string(defs).expect("ToolDefinition serializes")
}

/// 已组装 defs 的工具名集合（C-4 矩阵断言载体）。
/// 独立 async fn 而非闭包：返回的 future 借用
/// `agent`——闭包形无法表达参数生命周期
/// （E0594 生命周期错误），命名 async fn 经
/// 生命周期省略规则自然成立。
async fn tool_names(agent: &cf_agent::Agent) -> Vec<String> {
    let defs = agent
        .tool_bridge()
        .tool_definitions_builtins_only()
        .await;
    defs.iter()
        .map(|td| td.function.name.clone())
        .collect::<Vec<_>>()
}

/// A-3（空操作定调，设计稿 §6 Phase A 验收 3）：
/// `dynamic_tools` 两极性下组装链输出**字节级一致**。
///
/// Phase A 实现为单代码路径（设计稿 v3.1 K3 顺手 3 +
/// v3.2 极性修正）：flag=false（缺省）短路直通现状全量
/// defs；flag=true 视同 false 走同一条路径（切分分支
/// Phase B 才实现），仅多一条 warn 日志——日志不进
/// defs，故两极性 defs 字节级一致。同时锚定“与现状
/// 一致”：组装点输出 == bridge 直出全量 builtins
/// （plan 过滤器在现状 build 中是恒等 pass-through，
/// 设计稿 §3.2.4）。
#[tokio::test(flavor = "current_thread")]
async fn a3_flag_polarities_defs_byte_identical() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<cf_acp_lib::AcpClientMessage>();
            let (persistence_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();

            // flag=false（缺省极性）。
            let actor_off = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx)
                .await;
            // 带真实工具的夹具（update_goal 注册件）——
            // test_agent_default 是空工具集，defs 为空
            // 会使字节级恒等断言失去意义。
            actor_off.agent.replace(test_agent_with_goal_tool().await);
            assert!(
                !actor_off.dynamic_tools,
                "test fixture must default the Phase A flag to false"
            );
            let defs_off = actor_off.prepare_tool_definitions_inner().await;

            // flag=true（Phase A：no-op + warn，切分未实现）。
            // 独立通道对：create_test_actor 接管 sender 所有权。
            let (gateway_tx_on, _) =
                tokio::sync::mpsc::unbounded_channel::<cf_acp_lib::AcpClientMessage>();
            let (persistence_tx_on, _) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let mut actor_on =
                create_test_actor(0, 256_000, 85, gateway_tx_on, persistence_tx_on).await;
            actor_on.agent.replace(test_agent_with_goal_tool().await);
            actor_on.dynamic_tools = true;
            let defs_on = actor_on.prepare_tool_definitions_inner().await;

            // 两极性字节级一致（纯空操作）。
            assert_eq!(
                defs_fingerprint(&defs_off),
                defs_fingerprint(&defs_on),
                "A-3: flag=true must be a byte-level no-op over flag=false in Phase A"
            );

            // 与现状基线一致：组装点输出 == bridge 直出
            // 全量 builtins（plan 恒等过滤 + 无 plan 激活
            // 时不改变任何字节）。
            let bridge = actor_off.agent.borrow().tool_bridge().clone();
            let raw_builtins = bridge.tool_definitions_builtins_only().await;
            assert_eq!(
                defs_fingerprint(&defs_off),
                defs_fingerprint(&raw_builtins),
                "A-3: assembly point output must equal the raw full built-in defs \
                 (the Phase A single path is the current behavior)"
            );
            assert!(
                !defs_off.is_empty(),
                "fixture must expose a non-empty toolset for the identity to be meaningful"
            );
        })
        .await;
}

/// A-1（口径一致，设计稿 §6 Phase A 验收 1）：
/// 三口（turn 工具列表 / `/context` 计数 / compaction
/// 请求）共用同一组装点，口径一致——沿用
/// `context_tool_definition_consistency`（即
/// `tool_definition_caliber_tests`）测试模式，并扩到
/// `dynamic_tools` 两极性（Phase A 下两口性同一）。
#[tokio::test(flavor = "current_thread")]
async fn a1_three_mouths_share_one_assembly_caliber() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            for dynamic_tools in [false, true] {
                let (gateway_tx, _) = tokio::sync::mpsc::unbounded_channel::<
                    cf_acp_lib::AcpClientMessage,
                >();
                let (persistence_tx, _) =
                    tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
                let mut actor =
                    create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
                actor.agent.replace(test_agent_with_goal_tool().await);
                actor.dynamic_tools = dynamic_tools;

                // 组装点单源。
                let defs = actor.prepare_tool_definitions_inner().await;

                // 口 1：turn 工具列表。
                let specs = actor.turn_base_tool_specs(&defs);

                // 口 2：/context 计数与 token 估算。
                let info = actor.build_session_info().await;
                assert_eq!(
                    info.context.tool_definitions_count as usize,
                    specs.len(),
                    "A-1: /context count vs turn specs (dynamic_tools={dynamic_tools})"
                );
                let expected_tokens =
                    cf_chat_state::estimate_tool_definitions_tokens(&defs);
                assert_eq!(
                    info.context.tool_definitions_tokens, expected_tokens,
                    "A-1: /context tokens vs turn defs tokens \
                     (dynamic_tools={dynamic_tools})"
                );

                // 口 3：compaction 请求经同一组装链
                // （prepare_tool_definitions →
                // turn_base_tool_specs，见 compaction.rs
                // two_pass_sample / 单遍路径）—— timed
                // 包装与 inner 同源，规格映射同一函数。
                let compaction_defs = actor.prepare_tool_definitions().await;
                assert_eq!(
                    defs_fingerprint(&compaction_defs),
                    defs_fingerprint(&defs),
                    "A-1: compaction-request defs must come from the same \
                     assembly point (dynamic_tools={dynamic_tools})"
                );
                let compaction_specs = actor.turn_base_tool_specs(&compaction_defs);
                assert_eq!(
                    specs.len(),
                    compaction_specs.len(),
                    "A-1: compaction specs vs turn specs \
                     (dynamic_tools={dynamic_tools})"
                );
            }
        })
        .await;
}

/// A-4（旗标纪律，设计稿 §6 Phase A 验收 4）：
/// flag **会话启动一次性读取**——快照后中途配置翻转
/// 不影响当次会话 defs。
#[tokio::test(flavor = "current_thread")]
async fn a4_flag_snapshot_survives_mid_session_config_flip() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            // 会话启动时的配置读取（spawn 路径：
            // effective_config.resolve_dynamic_tools()
            // 一次见 spawn.rs）。
            let mut startup_config = crate::agent::config::Config::default();
            startup_config.features.dynamic_tools = Some(true);
            let startup_flag = startup_config.resolve_dynamic_tools();
            assert!(startup_flag, "fixture: startup config enables the flag");

            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<cf_acp_lib::AcpClientMessage>();
            let (persistence_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let mut actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx)
                .await;
            actor.agent.replace(test_agent_with_goal_tool().await);
            // spawn 落点：启动读取结果快照进 actor。
            actor.dynamic_tools = startup_flag;
            let defs_at_startup = actor.prepare_tool_definitions_inner().await;

            // 中途配置翻转（config.toml 改回 false）——
            // 新读取能看到翻转……
            let mut flipped_config = crate::agent::config::Config::default();
            flipped_config.features.dynamic_tools = Some(false);
            assert!(
                !flipped_config.resolve_dynamic_tools(),
                "a fresh read observes the mid-session flip"
            );
            // ……但当次会话的启动快照不受影响，defs
            // 字节级不变。
            assert!(
                actor.dynamic_tools,
                "A-4: the session's startup snapshot must not track the config flip"
            );
            let defs_after_flip = actor.prepare_tool_definitions_inner().await;
            assert_eq!(
                defs_fingerprint(&defs_at_startup),
                defs_fingerprint(&defs_after_flip),
                "A-4: mid-session config change must not alter the live session's defs"
            );
        })
        .await;
}

/// C-7（plan 恒等回归保护，设计稿 §Q4 + §3.2.4）：
/// plan 模式进出前后 defs **字节级不变**——现状
/// `filter_cursor_tools_by_plan_mode` 是恒等 pass-through
/// twin；若未来真身激活而未经 Q2-plan 重审，此测试变红，
/// 强制设计者回到设计稿 Q2-plan 重审（而非静默破坏
/// KV 前缀）。
#[tokio::test(flavor = "current_thread")]
async fn c7_plan_mode_enter_exit_defs_byte_identical() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<cf_acp_lib::AcpClientMessage>();
            let (persistence_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx)
                .await;
            // plan 对注册夹具：plan 进出恒等断言才有
            // 观察面（若未来 plan 过滤真身激活而未经
            // Q2-plan 重审，defs 在此变红）。
            actor.agent.replace(test_agent_with_plan_tools().await);

            let defs_before = actor.prepare_tool_definitions_inner().await;

            // plan 模式进入（Pending → Active）。
            let entered = actor.plan_mode.lock().enter_pending();
            assert!(entered, "fixture: plan mode must enter from inactive");
            actor.plan_mode.lock().activate();
            assert!(
                actor.plan_mode.lock().is_active(),
                "fixture: plan mode must report active"
            );
            let defs_plan_active = actor.prepare_tool_definitions_inner().await;

            // plan 模式退出。
            actor.plan_mode.lock().user_exit(false);
            assert!(
                !actor.plan_mode.lock().is_active(),
                "fixture: plan mode must report inactive after exit"
            );
            let defs_after_exit = actor.prepare_tool_definitions_inner().await;

            let before = defs_fingerprint(&defs_before);
            assert_eq!(
                before,
                defs_fingerprint(&defs_plan_active),
                "C-7: plan-enter must not change defs (byte-level)"
            );
            assert_eq!(
                before,
                defs_fingerprint(&defs_after_exit),
                "C-7: plan-exit must not change defs (byte-level)"
            );
        })
        .await;
}

/// C-4（条件断言版，设计稿 §Q4）：凡
/// `ToolKind ∈ {SearchTool, UseTool}` 的**已注册**工具，
/// 在任何裁剪配置（--tools 白名单 / --disallowed-tools）
/// 下不被移除；plan 对在无白名单裁剪的 配置下恒在。
///
/// 条件式说明（设计稿 C-4 注）：
/// - 现状 meta 二元默认未注册进 defs（设计稿 Q1
///   #29/#30）——本夹具显式注册它们，使“若注册则恒在”
///   非空；
/// - 白名单模式下 meta 二元经 `cf-agent/src/builder.rs`
///   的 `SearchTool | UseTool` 恒保留条款存活（:936-940
///   行锚）；
/// - **plan 对的无条件钉槽（常驻段尾部固定槽）是
///   Phase B 投影行为（Q2-plan）**：现状窄白名单
///   （如 `--tools read_file`）的 retain 会按既有规则
///   裁掉 plan 对——故 plan 对恒在此处断言在“无白名单
///   裁剪”与“denylist 不针对”配置下成立；Phase B 随
///   投影钉槽落地，本契约升级为无条件形式。
///
/// 同时覆盖 C-1 的 builder 侧半：空白名单（未设置）
/// ≡ 不裁剪 → defs 计数全量（等于注册全集）。
#[tokio::test(flavor = "current_thread")]
async fn c4_meta_pair_survives_every_trim_config() {
    use cf_agent::AgentBuilder;
    use cf_tools::implementations::qidi_build::{
        EnterPlanModeTool, ExitPlanModeTool, ReadFileTool,
    };
    use cf_tools::implementations::search_tool::SearchTool;
    use cf_tools::implementations::use_tool::UseTool;
    use cf_tools::registry::types::ToolConfig;
    use cf_tools::types::tool::ToolKind;

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            // 夹具定义：显式注册 meta 二元 + read_file +
            // grep；plan 对由 builder 的
            // `ensure_plan_mode_tools` 注入（无条件补齐）。
            let mut definition = cf_agent::AgentDefinition::default_qidi_build();
            definition.discover_skills = false; // 无 FS 扫描
            definition.tool_config.tools = vec![
                ToolConfig::for_tool::<ReadFileTool>(),
                ToolConfig::for_tool::<cf_tools::implementations::qidi_build::GrepTool>(),
                ToolConfig::for_tool::<SearchTool>(),
                ToolConfig::for_tool::<UseTool>(),
            ];

            let build_with = |tools: Vec<String>,
                              disallowed: Vec<String>| async {
                let mut def = definition.clone();
                def.tools = tools;
                def.disallowed_tools = disallowed;
                AgentBuilder::new(
                    std::path::PathBuf::from("/tmp"),
                    std::sync::Arc::new(
                        cf_tools::computer::local::LocalTerminalBackend::new(),
                    ),
                    cf_tools::notification::ToolNotificationHandle::noop(),
                )
                .from_definition(def)
                .build()
                .await
                .expect("AgentBuilder::build must succeed for the C-4 matrix")
            };

            // ── 情形 1：未设置白名单（C-1：空名单 ≡
            //    未设置 ≡ 全量 defs）→ meta 二元 + plan
            //    对恒在，defs 计数 = 注册全集（含
            //    inject_default_tools 注入件）。
            let agent = build_with(vec![], vec![]).await;
            let full = tool_names(&agent).await;
            assert!(
                full.contains(&"search_tool".to_string()),
                "C-4: meta search_tool must ship when registered (no trim)"
            );
            assert!(
                full.contains(&"use_tool".to_string()),
                "C-4: meta use_tool must ship when registered (no trim)"
            );
            assert!(
                full.contains(&"enter_plan_mode".to_string())
                    && full.contains(&"exit_plan_mode".to_string()),
                "C-4: plan pair must ship when no allowlist trim applies"
            );

            // ── 情形 2：白名单未点名 meta 二元 →
            //    meta 二元经恒保留条款存活（C-4 核心
            //    断言，builder.rs:936-940）。
            let agent = build_with(vec!["read_file".into()], vec![]).await;
            let trimmed = tool_names(&agent).await;
            assert!(
                trimmed.contains(&"search_tool".to_string()),
                "C-4: registered search_tool must survive an allowlist that \
                 does not name it"
            );
            assert!(
                trimmed.contains(&"use_tool".to_string()),
                "C-4: registered use_tool must survive an allowlist that \
                 does not name it"
            );
            assert!(
                trimmed.contains(&"read_file".to_string()),
                "allowlist-named read_file must ship"
            );

            // ── 情形 3：白名单点名 meta 二元 → 显式
            //    点名同样保留。
            let agent = build_with(
                vec![
                    "read_file".into(),
                    "search_tool".into(),
                    "use_tool".into(),
                ],
                vec![],
            )
            .await;
            let named = tool_names(&agent).await;
            assert!(
                named.contains(&"search_tool".to_string())
                    && named.contains(&"use_tool".to_string()),
                "C-4: explicitly named meta pair must ship"
            );

            // ── 情形 4：denylist 不针对 meta/plan →
            //    两者恒在。
            let agent = build_with(vec![], vec!["think".into()]).await;
            let denylisted = tool_names(&agent).await;
            assert!(
                !denylisted.contains(&"think".to_string()),
                "denylisted think must be cut"
            );
            assert!(
                denylisted.contains(&"search_tool".to_string())
                    && denylisted.contains(&"use_tool".to_string()),
                "C-4: meta pair must survive a denylist that does not target it"
            );
            assert!(
                denylisted.contains(&"enter_plan_mode".to_string())
                    && denylisted.contains(&"exit_plan_mode".to_string()),
                "C-4: plan pair must survive a denylist that does not target it"
            );

            // meta 二元的 kind 分类（穷尽表 Q1 #29/#30）
            // 与设计稿一致——恒驻 meta 层。
            assert_eq!(
                ToolKind::SearchTool.pipeline_segment(),
                cf_tools::registry::resident_set::ToolSegment::Meta
            );
            assert_eq!(
                ToolKind::UseTool.pipeline_segment(),
                cf_tools::registry::resident_set::ToolSegment::Meta
            );
        })
        .await;
}
