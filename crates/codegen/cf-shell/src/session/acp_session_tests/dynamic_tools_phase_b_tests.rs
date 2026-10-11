//! P0-2 第二步 Phase B 验收测试（设计稿
//! `docs/token-optimization/P0-2第二步-动态工具管线设计稿.md`
//! §6 Phase B 节 + §Q2/§Q2.4/§Q2.5）。
//!
//! Phase B 语义锚点：`dynamic_tools=true` 时组装点
//! （`prepare_tool_definitions_inner`）从 no-op 升级为**真实切分**：
//! built-in `defs` 按 kind 分流为常驻段（常驻 + plan 尾部槽 + meta
//! 二元）与 import 池；import 池按 import 解锁列表 **append-only**
//! 追加到常驻段尾部（Q2.5 分段三律 #2）。同时每轮发布
//! `DefsSnapshot`（B-12 门控 + C-8a）与 `BuiltinToolSearchSurface`
//! （B-0/B-1 搜索扩面）。
//!
//! 本文件钉住 Phase B 契约：
//! - **C-10**：flag=true 常驻段 == flag=false 视图的确定性常驻子集
//!   （append-only 前缀稳定性）；
//! - **C-11**：解锁只增不减（imported append-only），投影单调前缀增长；
//! - **C-12**：use_tool 中转真到达实际工具（端到端）；
//! - **金标准**：search_tool 查询 → 解锁 → use_tool 中转全链跑通
//!   （hermetic，设计稿 B-1 hermetic 主验收）；
//! - **imported 生命周期四场景**（resume/fork/subagent/compact，
//!   设计稿 Q2.4 + /clear）。

use super::support::*;
use super::*;

/// defs 的字节级快照（serde JSON 串）。
fn defs_fingerprint(defs: &[cf_tools::types::definition::ToolDefinition]) -> String {
    serde_json::to_string(defs).expect("ToolDefinition serializes")
}

/// Client-facing wire names in order.
fn defs_names(defs: &[cf_tools::types::definition::ToolDefinition]) -> Vec<String> {
    defs.iter().map(|d| d.function.name.clone()).collect()
}

/// Fixture: `search_tool` + `use_tool` (both meta → resident) + `think`
/// (kind `Think` → imported/hidden). The only registered builtin that the
/// resident set does **not** carry is `think`, so it is the import candidate.
async fn test_agent_dynamic_tools_fixture() -> cf_agent::Agent {
    use cf_tools::implementations::search_tool::SearchTool;
    use cf_tools::implementations::think::ThinkImpl;
    use cf_tools::implementations::use_tool::UseTool;
    use cf_tools::registry::types::ToolConfig;
    test_agent_with_tools(vec![
        ToolConfig::for_tool::<SearchTool>(),
        ToolConfig::for_tool::<UseTool>(),
        ToolConfig::for_tool::<ThinkImpl>(),
    ])
    .await
}

/// Deterministic resident segment of a full defs list (the C-10 anchor:
/// the same function the assembly point uses).
fn resident_segment(
    bridge: &cf_tools::bridge::ToolBridge,
    defs: &[cf_tools::types::definition::ToolDefinition],
) -> Vec<cf_tools::types::definition::ToolDefinition> {
    let partitioned = cf_tools::registry::dynamic_tools::partition_builtin_defs(
        defs.iter().map(|def| cf_tools::registry::dynamic_tools::SegEntry {
            client_name: def.function.name.as_str(),
            kind: bridge
                .declared_kind_of(&def.function.name)
                .unwrap_or(cf_tools::types::tool::ToolKind::Read),
            definition: def,
        }),
    );
    partitioned.resident().to_vec()
}

/// C-10（append-only 前缀稳定性）：flag=true 的常驻段与 flag=false 视
/// 图的**确定性常驻子集**字节级相等——常驻段是 flag=false defs 的一
/// 个纯函数，切分只重排（plan 尾部槽）不改变常驻集合的内容与字节。
#[tokio::test(flavor = "current_thread")]
async fn c10_resident_segment_is_deterministic_subset_of_full_defs() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<cf_acp_lib::AcpClientMessage>();
            let (persistence_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();

            // 真实夹具：search_tool + use_tool + think。
            let actor_off =
                create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            actor_off.agent.replace(test_agent_dynamic_tools_fixture().await);
            let defs_off = actor_off.prepare_tool_definitions_inner().await;
            let bridge = actor_off.agent.borrow().tool_bridge().clone();

            let (gateway_tx_on, _) =
                tokio::sync::mpsc::unbounded_channel::<cf_acp_lib::AcpClientMessage>();
            let (persistence_tx_on, _) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let mut actor_on =
                create_test_actor(0, 256_000, 85, gateway_tx_on, persistence_tx_on).await;
            actor_on.agent.replace(test_agent_dynamic_tools_fixture().await);
            actor_on.dynamic_tools = true;
            let defs_on = actor_on.prepare_tool_definitions_inner().await;

            assert_eq!(
                defs_fingerprint(&defs_on),
                defs_fingerprint(&resident_segment(&bridge, &defs_off)),
                "C-10: flag=true resident segment must equal the deterministic \
                 resident subset of the flag=false defs"
            );
            // 常驻段只含 meta 二元（本夹具无其它常驻 kind）；think 被藏。
            let names_on = defs_names(&defs_on);
            assert!(names_on.contains(&"search_tool".to_string()));
            assert!(names_on.contains(&"use_tool".to_string()));
            assert!(
                !names_on.contains(&"think".to_string()),
                "C-10: hidden imported tool must not ship in the resident segment"
            );
            assert!(
                defs_names(&defs_off).contains(&"think".to_string()),
                "flag=false (baseline) still ships the full defs (think included)"
            );
        })
        .await;
}

/// C-11（解锁只增不减，imported append-only）：解锁后常驻段不动、投影
/// = 常驻段 ++ 按解锁时序追加；重复解锁幂等；投影是常驻段的单调前缀增长。
#[tokio::test(flavor = "current_thread")]
async fn c11_unlock_append_only_and_projection_prefix_grows() {
    use cf_tools::types::resources::DynamicToolState;
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<cf_acp_lib::AcpClientMessage>();
            let (persistence_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let mut actor =
                create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            actor.agent.replace(test_agent_dynamic_tools_fixture().await);
            actor.dynamic_tools = true;
            let bridge = actor.agent.borrow().tool_bridge().clone();

            let defs0 = actor.prepare_tool_definitions_inner().await;
            let names0 = defs_names(&defs0);
            assert!(!names0.contains(&"think".to_string()));

            // 解锁 think（模拟 search_tool 记录解锁）。
            let state = bridge
                .read_resource::<DynamicToolState>()
                .await
                .expect("assembly must publish DynamicToolState under flag=true");
            state.record_unlock("think");
            // 幂等：重复解锁不追加。
            state.record_unlock("think");

            let defs1 = actor.prepare_tool_definitions_inner().await;
            let names1 = defs_names(&defs1);

            // 常驻段字节级不改（前缀稳定）。
            assert!(
                defs_fingerprint(&defs0)
                    .len()
                    <= defs_fingerprint(&defs1).len(),
                "sanity"
            );
            assert_eq!(
                &names1[..names0.len()],
                names0.as_slice(),
                "C-11: resident segment is unchanged — projection is prefix-growth"
            );
            assert_eq!(
                names1,
                [names0.clone(), vec!["think".to_string()]].concat(),
                "C-11: the unlocked tool is appended at the tail (append-only)"
            );
            assert_eq!(
                state.unlocked(),
                vec!["think".to_string()],
                "C-11: unlock list is append-only and de-duplicated"
            );

            // 第三轮：状态稳定（无新解锁 → 投影不变）。
            let defs2 = actor.prepare_tool_definitions_inner().await;
            assert_eq!(defs_fingerprint(&defs1), defs_fingerprint(&defs2));
        })
        .await;
}

/// S-1（registry_ids 域契约断言，不删字段）：B-12 快照的 `registry_ids`
/// 域 == 该轮投影 def 的 registry_id 集合（写入侧
/// `sampler_turn::prepare_tool_definitions_inner` 的 `registry_id_of` 链）
/// ——让这一域有断言钉住，不再是无读方、无断言的死数据。消费侧 use_tool
/// 门控按 `client_names` 直判（relay 域），两域对本轮 built-in 面结构性同源
/// （C-5 双域裁决）。任何投影 def 若解析不出 registry_id（filter_map 静默
/// 丢）即红。
#[tokio::test(flavor = "current_thread")]
async fn s1_defs_snapshot_registry_ids_match_projection() {
    use cf_tools::types::resources::DefsSnapshot;
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<cf_acp_lib::AcpClientMessage>();
            let (persistence_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let mut actor =
                create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            actor.agent.replace(test_agent_dynamic_tools_fixture().await);
            actor.dynamic_tools = true;
            let bridge = actor.agent.borrow().tool_bridge().clone();

            let projected = actor.prepare_tool_definitions_inner().await;

            let snap = bridge
                .read_resource::<DefsSnapshot>()
                .await
                .expect("assembly must publish DefsSnapshot under flag=true");

            // 写入侧：每个投影 def 都必须能解析出 registry_id（无一被静默丢）。
            let expected_registry_ids: std::collections::HashSet<String> = projected
                .iter()
                .map(|d| {
                    bridge.registry_id_of(&d.function.name).unwrap_or_else(|| {
                        panic!(
                            "S-1: projected def `{}` has no registry_id — the snapshot's \
                             registry_ids domain would silently drop it",
                            d.function.name
                        )
                    })
                })
                .collect();
            assert!(
                !expected_registry_ids.is_empty(),
                "S-1: the fixture projection must be non-empty for this assertion to bite"
            );
            assert_eq!(
                snap.registry_ids, expected_registry_ids,
                "S-1: DefsSnapshot.registry_ids must equal the projection's registry_id set \
                 (write side sampler_turn vs consumer side use_tool gate)"
            );

            // 消费侧对齐：client_names 域 == 投影名集合（门控直判域）。
            let expected_client_names: std::collections::HashSet<String> =
                projected.iter().map(|d| d.function.name.clone()).collect();
            assert_eq!(
                snap.client_names, expected_client_names,
                "S-1: DefsSnapshot.client_names must equal the projection's wire-name set"
            );
        })
        .await;
}

/// C-12 + 金标准（search → 解锁 → 中转全链，hermetic）：`search_tool`
/// 查询命中被藏内建工具 → 返回其完整 schema 与逐字节 callable_name
/// （C-5 中转域）并记录解锁 → 同一轮 `use_tool` 按该名中转**真实到达**
/// 目标工具执行 → 下一轮投影把该工具 append 到常驻段尾部 → 工具一旦
/// 进入 defs 快照，再经 use_tool 中转即回 corrective（B-12 三态）。
#[tokio::test(flavor = "current_thread")]
async fn c12_golden_search_unlock_relay_full_chain() {
    use cf_tools::types::resources::DynamicToolState;
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<cf_acp_lib::AcpClientMessage>();
            let (persistence_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let mut actor =
                create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            actor.agent.replace(test_agent_dynamic_tools_fixture().await);
            actor.dynamic_tools = true;
            let bridge = actor.agent.borrow().tool_bridge().clone();

            // 轮 1：组装 → 常驻段（search_tool + use_tool），think 被藏。
            let defs_turn1 = actor.prepare_tool_definitions_inner().await;
            assert!(!defs_names(&defs_turn1).contains(&"think".to_string()));

            // search_tool("think") → schema + callable_name（C-5）。
            let search = bridge
                .call(
                    "search_tool",
                    serde_json::json!({ "query": "think", "limit": 5 }),
                    "call-search-1",
                )
                .await
                .expect("search_tool must run");
            let content = match search.output {
                cf_tools::types::output::ToolOutput::SearchTool(out) => out.content,
                other => panic!("expected SearchTool output, got {other:?}"),
            };
            let json: serde_json::Value = serde_json::from_str(&content).unwrap();
            let tools = json["results"][0]["tools"].as_array().unwrap();
            let think = tools
                .iter()
                .find(|t| t["callable_name"] == "think")
                .expect("search_tool must surface the hidden `think` tool");
            assert!(
                think["input_schema"]["properties"]["thought"].is_object(),
                "golden: search result must carry the full input schema"
            );
            let callable = think["callable_name"].as_str().unwrap().to_string();
            assert_eq!(callable, "think", "C-5: callable_name == relay domain");

            // 解锁被记录（同一轮，投影尚未刷新 → defs 快照仍不含 think）。
            let state = bridge
                .read_resource::<DynamicToolState>()
                .await
                .expect("DynamicToolState published");
            assert_eq!(state.unlocked(), vec!["think".to_string()]);

            // use_tool 中转到真实 `think`（快照不含 → 放行中转 → 真执行）。
            let relay = bridge
                .call(
                    "use_tool",
                    serde_json::json!({
                        "tool_name": callable,
                        "tool_input": { "thought": "ping" }
                    }),
                    "call-use-1",
                )
                .await
                .expect("C-12: hidden native tool must relay to the real tool");
            match relay.output {
                cf_tools::types::output::ToolOutput::Text(t) => {
                    assert!(
                        t.text.contains("Thought recorded"),
                        "C-12: relay reached the real think tool, got {:?}",
                        t.text
                    );
                }
                other => panic!("expected relayed Text output, got {other:?}"),
            }

            // 轮 2：投影把已解锁的 think append 到尾部（append-only）。
            let defs_turn2 = actor.prepare_tool_definitions_inner().await;
            let names2 = defs_names(&defs_turn2);
            assert!(
                names2.contains(&"think".to_string()),
                "golden: next-turn projection appends the unlocked tool"
            );
            assert_eq!(
                names2.last().map(String::as_str),
                Some("think"),
                "golden: appended at the tail (append-only)"
            );

            // 轮 2 后快照含 think → 再中转同一工具即回 corrective 三态。
            let corrective = bridge
                .call(
                    "use_tool",
                    serde_json::json!({
                        "tool_name": "think",
                        "tool_input": { "thought": "again" }
                    }),
                    "call-use-2",
                )
                .await
                .expect_err("in-defs native tool must be corrective, not relayed");
            assert!(
                corrective.detail.contains("native tool"),
                "B-12 three-state: in-defs tool keeps the corrective error, got {}",
                corrective.detail
            );
        })
        .await;
}

/// B-8 回退锚（Phase B 部分）：flag=true → false 后 defs 字节级回
/// Phase A 完成态（现状全量 defs，== committed golden 视图）。关闭路
/// 径无残留——切分是纯投影，关 flag 即直通。
#[tokio::test(flavor = "current_thread")]
async fn b8_flag_off_byte_identical_to_baseline() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let mk = |flag: bool| async move {
                let (gateway_tx, _) =
                    tokio::sync::mpsc::unbounded_channel::<cf_acp_lib::AcpClientMessage>();
                let (persistence_tx, _) =
                    tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
                let mut actor =
                    create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
                actor.agent.replace(test_agent_dynamic_tools_fixture().await);
                actor.dynamic_tools = flag;
                actor
            };
            let actor_off = mk(false).await;
            let defs_off = actor_off.prepare_tool_definitions_inner().await;
            let bridge = actor_off.agent.borrow().tool_bridge().clone();
            // flag=false 直通 bridge 全量 builtins（无残留切分）。
            let raw = bridge.tool_definitions_builtins_only().await;
            assert_eq!(
                defs_fingerprint(&defs_off),
                defs_fingerprint(&raw),
                "B-8: flag=false must be a byte-level pass-through (no split residue)"
            );
            // flag=false 不发布 defs 快照（B-12 门控保持 corrective 默认）。
            assert!(
                bridge
                    .read_resource::<cf_tools::types::resources::DefsSnapshot>()
                    .await
                    .is_none(),
                "B-8/fail-safe: flag=false must not publish a DefsSnapshot"
            );
        })
        .await;
}

/// imported 生命周期**状态语义**（设计稿 Q2.4）——状态级声明 + 断言
/// （**非**四场景接线验收；接线落地后另立真·场景测试）。resume 沿用 /
/// fork 继承 / subagent 隔离（从零）/ compact 保留；/clear 清空。解锁
/// 状态是 per-session 资源，生命周期语义落在这些操作如何处置它。
#[test]
fn imported_lifecycle_state_semantics() {
    use cf_tools::types::resources::DynamicToolState;

    // 基线：会话中途解锁了两个工具。
    let session = DynamicToolState::default();
    session.record_unlock("think");
    session.record_unlock("video_gen");
    assert_eq!(session.unlocked(), vec!["think", "video_gen"]);

    // ① resume：解锁状态随会话持久化恢复——资源是同一份（同 Arc），
    //    resume 重建投影读到相同解锁列表（「历史全部 imported 全量
    //    恢复」，Q2.4 resume 行；早期窗 N 重置为 O-8 存储点事项）。
    let resumed = session.clone();
    assert_eq!(
        resumed.unlocked(),
        vec!["think", "video_gen"],
        "resume: imported unlock set is fully restored"
    );

    // ② fork（verbatim-fork）：子代理继承父前缀 / 管线开关（Q2.4 fork
    //    行 + flag 继承表）——同源 → 同解锁集。
    let forked = session.clone();
    assert_eq!(
        forked.unlocked(),
        session.unlocked(),
        "fork: child inherits the parent's imported unlock set"
    );

    // ③ subagent：从零开始，不继承父 imported（Q2.4 subagent 行）。
    let subagent = DynamicToolState::default();
    assert!(
        subagent.unlocked().is_empty(),
        "subagent: imported unlock set starts empty (isolated)"
    );

    // ④ compact（auto-compact）：registry 全量不动、投影状态保留
    //    （Q2.4 compact 行）——压缩不触碰解锁状态。
    let after_compact = session.clone();
    assert_eq!(
        after_compact.unlocked(),
        vec!["think", "video_gen"],
        "compact: imported unlock state is preserved across a rebuild"
    );

    // /clear：新任务新抽屉——清空。
    session.reset();
    assert!(
        session.unlocked().is_empty(),
        "/clear: unlock state is cleared"
    );
}

/// Rework 1（GLM 审计 P1 返工）：backend-search 下 `web_search` 的剔除
/// **前移到切分输入**——`web_search` 即便被解锁，也**不进投影、不进快照、
/// 不进搜索面**（三口同滤，共用 `backend_search_drops_tool` 单一判据）。
///
/// 修前：剔除只发生在 `turn_base_tool_specs`（partition 之后），故解锁后
/// `web_search` 进投影与快照（B-12 gate 视其可见）——模型 `use_tool` 得
/// corrective 直接调，必然 provider 400；且 `web_search` 白占 imported 段
/// token。修后：切分输入即滤除，三者皆无。三面同时断言钉住该修复。
#[tokio::test(flavor = "current_thread")]
async fn backend_search_drops_web_search_from_snapshot_projection_and_surface() {
    use cf_tools::types::resources::{
        BuiltinToolSearchSurface, DefsSnapshot, DynamicToolState,
    };
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<cf_acp_lib::AcpClientMessage>();
            let (persistence_tx, _) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let mut actor =
                create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            // 夹具：`web_search`（kind WebSearch → imported）+ `read_file`
            // （resident），且 backend-search 使能（agent 标志 + 会话快照）。
            actor.agent.replace(test_agent_backend_search().await);
            actor.dynamic_tools = true;
            actor.supports_backend_search.set(true);
            let bridge = actor.agent.borrow().tool_bridge().clone();

            // 首轮组装（懒建并发布 DynamicToolState）——空解锁：web_search
            // 作为 imported 工具被藏（不进投影）。
            let defs_turn1 = actor.prepare_tool_definitions_inner().await;
            assert!(
                !defs_names(&defs_turn1).contains(&"web_search".to_string()),
                "rework1: web_search is hidden (imported) with no unlock"
            );

            // 最坏情形：模型经 search_tool 解锁了 web_search（剔除前会把
            // 它带进投影/快照）。
            let state = bridge
                .read_resource::<DynamicToolState>()
                .await
                .expect("assembly must publish DynamicToolState under flag=true");
            state.record_unlock("web_search");

            let defs = actor.prepare_tool_definitions_inner().await;
            let names = defs_names(&defs);

            // ① 投影：即便解锁，web_search 也不得出现。
            assert!(
                !names.contains(&"web_search".to_string()),
                "rework1: backend-search + unlocked web_search must NOT enter the \
                 projection, got {names:?}"
            );

            // ② 快照（B-12 gate 视图）：web_search 不得出现——否则 gate 视其
            //    可见，模型直调 400。
            let snap = bridge
                .read_resource::<DefsSnapshot>()
                .await
                .expect("assembly must publish DefsSnapshot under flag=true");
            assert!(
                !snap.contains_client_name("web_search"),
                "rework1: web_search must be absent from the defs snapshot (B-12 gate view)"
            );

            // ③ 搜索面：web_search 不得出现在 BuiltinToolSearchSurface。
            let surface = bridge
                .read_resource::<BuiltinToolSearchSurface>()
                .await
                .expect("assembly must publish BuiltinToolSearchSurface under flag=true");
            assert!(
                surface.0.iter().all(|e| e.client_name != "web_search"),
                "rework1: web_search must be absent from the built-in search surface"
            );
        })
        .await;
}
