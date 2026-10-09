**终论：v1.1 不能作评审基线（Opus 三条诊断成立，∩ 处方已被正确放弃）；v3.1 也还不能按文末「Phase A 可派工」直接派工，需 v3.2 文档修订后再派 Phase A。** 必改见文末。

## 1. Opus 三条复核

- **白名单 ∪/∩：诊断成立，∩ 方向不成立。** 矛盾是真的（既要保留 `--tools` 裁剪力，又把解锁域写成可越过白名单）。v3.1 §4「设置时：常驻=白名单∪meta、解锁域⊆白名单、替换默认集」比 ∩ 正确：∩ 会把默认集外、用户显式点名的工具排除。裁剪力由 ⊆ 保住，不是由 ∩ 保住。残留：正文仍写「∪」，易被读成再次越界；须写明 meta 不在白名单裁剪域内、其余名字不得 ∪。
- **FinalizedTool 无直接 kind：成立且仍是承重缺陷，不是已关闭的锚点注记。** 分类只能走 `metadata.kind()`；`from_id` 路径 kind=None；`tool_for_kind` 单数查不全。v3.1 的 C-8b「kind() 与注册时附着 kind 一致」是循环的：MCP/自定义 metadata 的 kind 本来就来自 metadata，没有第二可信源，伪造 SearchTool/EnterPlan 无法被该断言抓住。Q1 新查询写成 `kind→Vec<client_name>`，与 use_tool 的 registry_id 域不一致，C-5 无法由该 API 满足。
- **「36 变体 vs 11 常驻」：当时是硬伤，降级只对 Phase A 成立。** 枚举 36 ≠ 注册实例，这句对，编译期穷尽 match 仍必要。但常驻是否 <55%、WebSearch 若进中部会挖穿前缀、List/ListDir 分错会改常驻集，都还没 B-10 token 表。降级成「非阻塞」不得外推为切分方案已核实。Phase B 任何 endpoint 打开前，B-10 应是闸，不是并行收集项。

## 2. 第三视角盲区（前几轮修订本身引入或未闭合）

- **A-3 极性写反，直接挡住 Phase A。** 要求 `dynamic_tools=false` 字节级等于现状，却写「flag=true 时投影短路为直通」。按字面实现：关 flag 不是空操作，开 flag 又切不开 Phase B。单路径应是 flag=false 短路直通；flag=true 才走常驻/imported。
- **C-2 用一条假不变量替换另一条假不变量。** 「相邻轮内容不改」确实为假；改成「任意对已有 item 的改写即失败」则批量 hard-clear 本身必失败。正确不变量：窗口内 strict prefix-growth；触发轮允许一次批量不连续，之后再恢复 prefix-growth。
- **C-9 兜底与 MVP 契约互斥。** 系统提示要求禁止猜 schema、必须用 search_tool 返回的 schema；兜底却只给全量名、不含 schema，再引导 use_tool。必然撞上「args mismatch → re-run search_tool」，在 3 次上限处死路。全量名列表对大 MCP 面也无 token 上界。
- **B-12 快照域未对齐 corrective 判定域。** 快照是 defs 的 registry_id 集；现状 `EnabledNativeToolNames` / “call it directly” 比对的是原生工具名（多半 client_name）。name_override 会把「已在 defs」判成「被藏」或相反。C-8a 只证快照=registry_id 集，不证判定谓词同一域。extensions 注入本身方向对。
- **B-12 改变的是执行面可达性，不是纯投影。** 现状 native correction 默认拒绝中转；门控后 `--tools` 摘掉的原生工具变为可执行。这比 monitor 旁路更宽。默认 false 不够：缺「开 flag = 相对现状放宽执行」的发布语义，以及与 `--disallowed-tools` 并存时谁赢。
- **resume 全量恢复 imported 与白名单/capability 变更冲突未定义。** 「必须对齐历史 tool_use」vs「解锁域⊆白名单」vs「未注册则不可投影」三者无优先级。flag 快照不重读当前配置：事后关掉 dynamic_tools 对 resume 无效。fork 继承 imported、subagent 从零，两条 spawn 路径未映射到代码（verbatim-fork vs `apply_to_subagent_definition`），实现时必有一条违反现成契约。
- **分段三律假设 tools 数组可前缀缓存，证据不够。** ling-B 66.4%→94.7% 是静态过滤后整集稳定，不是数组中部 append。若 provider 把整个 tools 当一块缓存键，常驻段稳定并不保值；Q2.3 的 `B_k×H_t` 会低估。STRUCTURED_OUTPUT 随 imported 后移，进一步否定「只有 append 点之后作废」。
- **MVP 间接成本未进杀死判据。** 每次拿回=2 轮采样，历史变长、compact 提前，公式只计一跳中转。高频工具若被误划 imported，MVP「独立成立」仍可能亏过全量 defs。杀死判据「全场景不盈利则砍整条 B-2」过粗，应为按工具/按类，并设样本量与判定人；默认翻转灰度没有成功/失败阈值（任务成功率、schema 错误率、自身 cold→warm），hermetic 脚本过不了模型解析嵌套 schema 这一真正风险。
- **回退粒度假完整。** A/B/C 声称独立回退，但 B-12 改的是 use_tool 全局 corrective；flag=false 若未包住该分支，关管线仍改变 MCP/offline 行为。R-7 测的是三态，没有「dynamic_tools=false 时 corrective 字节级等于改前」的单测（与 A-3 极性错误是同一类）。
- **搜索索引与投影时序（O-13）会制造「search 可见、defs/快照不可见」窗口。** 运行期 MCP 重注册下一轮才进 imported 尾部：当轮 use_tool 若按快照放行，等于未投影即可执行，与「MVP 零 defs 变化可接受」一致，但与 B-4「按解锁时序 append」不一致。须明确当轮只允许 registry 派发、不得把该名字写入当轮 imported。

## 3. 必改（v3.2，结构可不动）

- 改正 A-3：false=直通短路；true 才切分；补「flag=false 时 B-12 不改变 corrective」断言。
- kind 信任源：注册时把 kind 写入 FinalizedTool 旁路字段或不可伪造表；C-8b 对照该字段，不对照 metadata 自身。kind→Vec 同时返回 registry_id 与 client_name。
- 重写 C-2 为「窗口内 prefix-growth + 触发轮一次批量豁免」。
- C-9：兜底必须带 schema，或明确禁止无 schema 的 use_tool；全量列表需预算/分页。
- B-12 判定与快照同一名字域；写明开 flag 相对现状放宽执行，以及与 `--tools`/`--disallowed-tools`/capability 的优先级（capability 永远赢）。
- resume/fork/subagent：工具面冲突优先级 + 两条 spawn 路径对照表；灰度默认翻转闸=B-10 完成且自身 cold→warm 与任务成功率达标。