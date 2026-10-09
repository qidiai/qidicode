# 三视角终审报告（Sonnet-5.5）

## 〇、总判
- v1.1 的修订主要写在头注，正文大半未落实：§8 仍写"35 变体"，§0 仍引 tool_taxonomy.rs:69-70，§2.1 仍把 kind 画成 FinalizedTool 字段，Q1 表仍有 ImageToVideo 重复且 glob 归 ListDir，§4-Q3 仍留"不对，50% 是…"的自我纠正文字。
- 另有 A-4 与 A-5 互斥等新缺陷，本稿第二节给出。
- **终论：需 v2，不能作评审基线。**

## 一、对 Opus 三项发现的复核

1. **∪/∩ 矛盾：成立，但"∩"裁定方向对、字面公式不对。**
   - 正文有三处按 ∪ 写：§3、Phase A 范围、C-2 的"默认常驻集其余不动"。按 ∪，`--tools` 确实失去裁剪力。
   - 字面 ∩ 有新问题：`--tools video_gen` 这类默认常驻集之外的显式条目会被吞掉，或被迫先 tool_search 才能用，这比第一步"白名单即全集"的语义更退化。
   - 建议改为：
     - 设置白名单时，常驻段 = 白名单 ∪ meta（替换默认集），解锁域 ⊆ 白名单。
     - 未设置时，常驻段 = 默认集，解锁域 = 注册全集 − 排除面。
   - C-3 只定义了"白名单为空"时的解锁域，没定义"白名单非空"时的解锁域。若允许 tool_search 越过白名单，`--tools` 就不再是边界。

2. **FinalizedTool 无直接 kind：成立，影响中低。**
   - 投影对象是 wire defs，只能用 `tool_kind(name)` 反查。
   - `tool_for_kind(kind)` 是一对一，而 Search（grep 双实现）、List、ListDir 都是一 kind 多工具，不能用于常驻判定。
   - 必须逐 name 分类，并核实 name_override / client_name 下反查是否仍成立。
   - MCP 工具恒为 kind=Other（R-3 已隐含），属保守安全，但要写成显式规则。

3. **Q1 的 36 vs 11 缺口：成立，严重度中（非阻塞，但必改）。**
   - 缺陷不在"11 太少"，而在没有穷尽表和未列 kind 的默认归属。
   - Delete/Move 是写入类：Edit/Write 常驻而 Delete/Move 进 imported，会让重命名、删除流程多一跳。
   - 建议用无通配符的 `match` 强制每个新增 ToolKind 变体必须表态，并补 A 阶段测试。

## 二、第三视角新发现

**🔴 阻塞级**
1. **A-4 与 A-5 互斥。**
   - A-4 要求 Phase A 对现状"零差异、defs 全量字节级相同"，A-5 要求常驻集 <55% 全量。
   - Phase A 无 tool_search，常驻切分一旦生效，非常驻工具对模型不可见，等于功能回归。
   - 必须让 Phase A 在默认关闭 flag 下是空操作，或把切分挪到 Phase B 一并上线。
2. **defs 追加 = 整个历史前缀缓存作废，缺成本模型。**
   - tools 位于 system 与 messages 之前，每次解锁都使其后全部缓存失效，晚期解锁的重算成本可能远超 8.5K×剩余轮数。
   - §7 的 66.4%→94.7% 是静态过滤后保持稳定的数据，不能证明会话中途追加安全。
   - B-5 的"−2pp"不等于成本。需补盈亏平衡模型，或限定解锁时机，例如只在 compact 边界或会话早期。
3. **未评估更简单的替代方案：tool_search 把完整 schema 放进 tool_result，经现成的 `use_tool` 调用。**
   - 这样 defs 零变化，缓存零破坏，无需持久化 imported 集，也无需处理 plan 重过滤。
   - 代价是模型对包装调用的参数准确率，应做 eval 对比后再定。
   - Q2 没有备选对比表，直接选了最破坏缓存的路径。
4. **imported 集的生命周期完全缺失。**
   - resume、fork、subagent 继承、auto-compact、`/clear` 后 imported 状态怎么办，设计稿没写。
   - 恢复后 defs 与历史中已出现的 tool_use 不一致，会击穿缓存，部分 provider 还会拒绝未知 tool_use。

**🟠 必改级**
5. **plan 过滤与"常驻字节级不变"冲突。**
   - plan 开关切换会改变 defs 前缀，使 A-1 在 plan 进出时必然失败。
   - EnterPlan/ExitPlan 若随状态显隐，同样破坏前缀。
   - C-4 写"plan 过滤除外"，意味着 plan 下 meta 可能被剔除，模型就无法 tool_search。
   - 需明确：plan 相关易变工具放在尾部，或声明 plan 切换是被允许的缓存断点。
6. **R-2 的安全论证自相矛盾。**
   - 文中说安全边界是"server-side deny + plan 过滤器"，但 `filter_cursor_tools_by_plan_mode` 只作用于 defs，而 `use_tool` 可旁路 defs。
   - 因此 B-2 测试通过也不代表 plan 下无法执行被排除工具。需核查执行侧是否有 plan 闸，并补 B-8 之外的"use_tool 在 plan 下"用例。
7. **模型直呼未投影工具的行为未定义。** 是执行、拒绝，还是提示去 tool_search，不同选择对应不同的失败形态，应写入契约。
8. **system prompt / skill / agent 模板里硬写了工具名**（如 memory_search）。常驻裁剪后这些引用成为悬空指引，需做模板条件化或审计。
9. **Phase C 与既有层冲突。**
   - 既有 `prune_retained_conversation` 每个用户回合仍按 TTL=10 零碎清，C-2 的"窗口内请求内容不改"在既有层不退让时不成立。
   - 设计稿需明确新层如何取代或闸住既有逐回合路径。
   - 修改深处旧 ToolResult 会使其后全部前缀失效，批量只降频、不降单次成本，也缺成本模型。
10. **重放提示有回弹风险。** "重跑 grep"可能再次产出大输出，形成"清除→重跑→再清除"的抖动，需加防抖，例如同参数重放次数上限。
11. **灰度与回退粒度不足。**
    - `dynamic_tools` 是全局 flag，而 R-1 要求按 endpoint 灰度，需要 per-endpoint 覆盖。
    - 会话中途翻转 flag 会使 defs 变化，应改为会话启动时一次性读取。
    - Phase C 没有 flag 和回退路径，`low_watermark_percent` 缺省应为关闭。
12. **O-7 是 Phase B 的前置阻塞，不应作为开放问题后补。**
    - 若 search_tool 索引只覆盖 MCP 元数据，内建的 video_gen、browser 无法被找到，B-1 直接失败。

**🟡 可后补**
13. C-3 的"白名单未设置则 warn"会在默认运行时触发，属噪声，应改为 debug 级。
14. Q1 回退段称"tool 未命中遥测、description 无预知可查"是 Phase B 验收项，但 B-1～B-7 里没有对应条目。
15. A-5 的"<55%"来自 ling-B 目标线，没有逐工具 token 实测支撑，应先出 per-tool 定义 token 表。
16. STRUCTURED_OUTPUT_TOOL 位于 imported 之后，新增 imported 工具会把它后移，"纯追加"表述不准，但因其本就易变，影响小。
17. 引用不一致：Reasoning 锚点同时写作 request_builder.rs:302-305 与 mutations.rs:302-305；图中出现未定义的"R-006"。
18. 正文有疑似生成噪声：Konversation、сервер、回家 TTL、会议室、imposed-once、residentslice、O-6 的"motion"、`\te2e`。定稿前需统一清理。

## 三、v2 必改项（按优先级）
1. 重写白名单语义为"替换 + 解锁域 ⊆ 白名单"，同步改 §3、Phase A、C-2、C-3，并用测试固化。
2. 消解 A-4 与 A-5 矛盾：Phase A 默认空操作，或并入 B。
3. 补解锁缓存成本模型，并评估"schema 入 tool_result + use_tool"替代方案。
4. 定义 imported 集在 resume、fork、subagent、compact 下的持久化与继承。
5. 明确 plan 切换与前缀稳定的关系，并核查 `use_tool` 绕过 plan 与 server deny 的执行侧闸。
6. 给出 ToolKind 穷尽表（无通配符 match）、逐 name 分类规则和统一变体数。
7. Phase C 补与既有逐回合 prune 的取代关系、重放防抖、flag 与回退。
8. 正文落实 Opus 全部修订（锚点、glob、重复项、:225、35→36），清理乱码，并把 O-7 提前为 B 的前置任务。