# Phase C 终态独立审计——deepseek-flash（第六方收官审计）

> 2026-10-17 深夜。审计员：deepseek-flash（spawn deepseek-flash-official 官方直连通道——**首次使用、一次成功、零 429**——速率档独立的实证案例）。对象：HEAD `8fd14a72`（含 Phase C 全部 R1-R8 返工）。**结论：可靠，可合入；新阻塞 0，返工 0，盲面 10 项（无阻塞）。**

## ⚠️ 审计基线勘误（本审计的关键流程发现）

**`3c1eccff` 不是 HEAD（`8fd14a72`）的祖先**——它与 `44f21f04` 是镜像重复提交（同标题同时间戳）；R7/R8 收官在另一条线的 `8f4a0961`（已随 HEAD 出货）。此前所有审计文档按 `3c1eccff` 记账的部分**应修正为 `8f4a0961`（R7/R8）+ HEAD `8fd14a72`**——否则会把缺 R7/R8 的快照记为最终态。（双会话并行提交同一改动的镜像现象——feat 线 7273a60a + main 线 8f4a0961，同内容双 hash。）

## R1-R8 最终态判定（全过）

- **R1** ✅：Assistant 可见文本 sibling（`[reasoning summary]` 前缀）；三 wire 路径（Anthropic 合并/ChatCompletions content/Responses message）逐条实锚；**生产序列"Reasoning 恒在其 Assistant 之前"三 stream 生产者+恢复路径+测试注释全面实锚**——替换后 Assistant(文本)→Assistant(tool_calls)→ToolResult，配对不变量不破
- **R2** ✅：continue 不插表+merge extend+双 marker 过滤+merge 回归测试
- **R3** ✅：豁免在两分支共同上游（keep_last 之后）；soft_trim/hard_clear 两路各一测带对照组
- **R4** ✅：文案（updates.jsonl 无条件/segment 仅 Segments 模式）+pointer_preserving_replacement+防抖接线（第 4 次转 terse 断言）
- **R5** ✅：age 门与 ToolResult 共用 effective_threshold+补偿两侧同式同源
- **R6** ✅：测试助手构造序与生产 turn.rs:466 一致
- **R7/R8（HEAD 落地）** ✅：config 透传+地板三防线+cleared==0 修复+RAII+age 单源+滞回回收线+三判据+merge 回归+文案+N2 节流（**字面 3c1eccff 缺此全部——基线勘误的原因**）

## C-1b/C-6/C-2/C-3/C-4 对照（全过）

- **C-1b 3/3 过**（①pregenerate.rs:144-149+表缺失 continue；②应用侧/预生成侧双跳过+专测；③subset_check 失败返回纯 floor+共享 NUMBER_RE——**附盲面 5：截断边界窄洞**）
- **C-6 过**（batched_mode() 要求 flag+watermark；四挂点门控；should_prune 整数化数学等价；**附盲面 10：pregen 不检查 enabled 的小洞**）
- **C-2 过**（生产序构造+逐 item 字节对比+触发轮清净保持；**限制：断言状态前缀非最终请求字节**）
- **C-3 过**（keep_last=3/hard=1 与 keep_last=1/hard=1 两构造分别唯一 exercising 各分支；两分支各单测）
- **C-4 过**（档案回捞方向/不回显 args/防抖第 4 次转 terse/指针共用保留）

## 盲面 10 项（无阻塞，按价值排序）

1. 审计基线 hash 错位（流程问题——修正为 8f4a0961+HEAD）
2. turn-end 预生成挂点无集成测试（reasoning_pregenerate.rs:161-171 自留 TODO——建议补含 flag-off+LLM 失败的集成测试）
3. 子代理未做门控（dream 有 is_subagent 跳过，pregenerate 没有——成本放大风险）
4. 摘要模型非 flash 档（取会话主模型——贵模型会话下成本高）
5. 字节截断先于子集校验的窄洞（120→1 切数字——建议对未截断 floor 校验）
6. 指纹碰撞无应用期复核（碰撞概率可忽略，建议表值附带块长或应用期重跑 subset_check）
7. UI/折叠交互未覆盖（Assistant sibling 脱离 Reasoning 折叠——人工抽检项）
8. 合成 User 中推（C-2"触发轮"非严格用户轮边界——措辞口径差）
9. 滞回饥饿边缘（释放<5pp 时窗口不重开——O-4 数据下通常能重开，未测）
10. pregen 不检查 pruning_config.enabled（batched=true+enabled=false 时白烧 LLM——配置自洽小洞）

## 总评

> Phase C 最终态在正确性上没有发现需要返工的新阻塞：三条硬底、flag-off 零介入、批量语义、双禁区与软指引都有代码锚点和可执行断言；R1-R8 返工痕迹在 HEAD 全部可验证。真正的风险不在实现而在**审计/交付记账**（hash 错位）与**四类未测边界**（turn-end 端到端、子代理成本、截断窄洞、UI 呈现）——建议补一条 turn-end 集成测试（含 flag-off 与 LLM 失败）并修正 hash 记录，其余作为灰度期遥测项。
