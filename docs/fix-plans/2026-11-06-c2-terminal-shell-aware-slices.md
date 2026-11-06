# C2 剩余 5 处 · 短切片任务书（备用，等 r4 窗口恢复后派发）

**状态**：待派发。基线 HEAD = `953b51ea`（C2 单点已提交，工作区干净）。
**编制日期**：2026-11-06。**派发模型**：`r4-deepseek-v4-1-flash-222`（今日 429，恢复后派）。

---

## 〇、指挥官前置动作（派发前必做，缺一不可）

1. **5 秒 PONG 探针**：先 spawn 一个最小任务（如"回复 PONG，不要做任何其他事"），
   确认返回内容真的是 PONG——**不是看 task_status 的 done**。
   今天实测过 ai-bridge 的 workbuddy 会把 `context` 送到、`task` 丢掉；
   spawn 侧也必须防同类"参数静默丢失"。
2. **若 PONG 探针仍 429**：暂停，不自动转 qwen 长任务（qwen 已被 AGENTS.md §9
   第 7 条纪律暂停长任务）。等窗口，不盲烧。
3. **每片派发时**：把本文「§1 总则」+ 对应「§3.x 单片」一起粘贴，不要只贴单片。
4. **每片独立验收、独立提交**，不与 C2 单点（`9c613f45`）或其他改动混合。

---

## 一、总则（每片必附）

### 纪律（用户 2026-11-06 裁定）
1. **只改本片点名的调用点**。严禁触碰其他片的坐标——本文件 §2 的表格是唯一权威映射。
2. **禁止修改生产代码的 JobObject 包装**（`streaming_local_terminal.rs:943-947` 的
   `cmd.wrap(JobObject)` / `cmd.wrap(KillOnDrop)`）。本轮只修测试 snippet。
3. **禁止把"进程树消失"的测试缺口塞进本轮**——那件事另开后续小单
   （已知缺口：现有标记验收只证明睡眠被打断，不证明 sleep.exe 已回收；
   Job Object 的保证目前无测试钉住）。本轮不得新增该类断言。
4. **禁止 git add / commit / stash / checkout / reset**。提交由指挥官做。
5. **禁止静默跳过、禁止空转假绿**。若改法让测试"通过但没真跑语义"，视为失败。
6. 除 `streaming_local_terminal.rs` 外**不得改动任何文件**。
7. 报告结尾写 `CODER RESULT: DONE` 或 `CODER RESULT: SPEC GAP`。用中文，数字带来源。

### 复用的现成基建（C2 单点已交付，直接调用，不要重造）
- `shell_snippet(ps: &str, sh: &str) -> String`：`#[cfg(not(unix))]` 与 `#[cfg(unix)]`
  双支，按 `cf_config::shell::detect_windows_shell()` 选 snippet
- `marker_probe_snippet(start, finish, ps_sleep, sh_sleep)`：生成
  `echo started > {start}; <sleep>; echo finished > {finish}`
- `probe_marker_paths(tag)` / `cleanup_probe_markers(&markers)` / `quoted_path(path)`
- `posix_snippet_active()`：非 unix 且当前 shell 为 Git Bash 时为真

### 副作用验收范式（kill 类必须照此设计）
- **start 标记存在** → 证明睡眠真的开跑（不是进程秒死）
- **finish 标记不存在** → 证明睡眠真的被打断
- **对照组**：同结构、短睡眠、**不 kill**，断言两个标记都出现
  → 证明"finish 缺失"确是 kill 的效果而非机制失灵
  （范式样板：C2 交付的 `test_kill_probe_control_completes_without_kill`）

### 每片验收（四条全满足才算完成）
1. `CARGO_TARGET_DIR=/c/qidi-cfshell-test CARGO_PROFILE_TEST_DEBUG=0 RUSTC_WRAPPER='' cargo test -p cf-shell --lib terminal::`
   → **全绿、0 failed、0 skipped**（Git Bash 默认环境）
2. `QIDI_SHELL=powershell` 同命令 → **全绿、0 failed、0 skipped**
3. **回归保护**：C2 已修好的两条必须仍绿——
   `test_kill_returns_signal`、`test_kill_probe_control_completes_without_kill`
4. **无泄漏**：跑完立刻查无残留 `sleep.exe`（`Get-CimInstance Win32_Process -Filter "Name='sleep.exe'"`），
   且 `$env:TEMP` 无新增 `qidi-terminal-probe-*` 标记文件

**跑法硬约束**：`RUSTC_WRAPPER=''`（禁 sccache）；C 盘热 target；跑前 `df -h /c` 看余量
（低于 ~20GB 先报告，不要自己清）。shell 是 Git Bash，`&&`/管道可用。

---

## 二、事实基线：6 个坐标 → 5 个用例（**不要当成 6 个用例**）

C2 单点已修掉 1 个（原 `:1198` 的 `kill_probe_command`）。当前剩余：

| # | 坐标 | 宿主函数 / 测试 | 类别 | 说明 |
|---|---|---|---|---|
| 1 | **`:1305`** | `process_group_probe_command()`（:1302）→ `test_process_group_kill_with_session`（:1752） | **kill / 进程组** | `"Start-Sleep -Seconds 300; Start-Sleep -Seconds 300"` = **600 秒**，两段连续睡眠，目的是让进程活得足够久以验证进程组 kill。测试轮询等 `InProgress`（最多 40 次）后断言 `kill_terminal` 返回 signal、`exit_code == Some(1)` |
| 2 | **`:1561`** | `test_ext_create_output_wait_release`（:1539） | **输出** | `"Write-Output hello"`。测试**已有**真实断言：`status.exit_code == Some(0)` 且 `output.output.contains("hello")`。当前 bash 下 127 而挂 |
| 3 | **`:1603`** | `test_ext_release_cleans_up`（:1584） | **release** | `"Start-Sleep -Seconds 30"`。只断言 release 后状态，bash 下进程秒死 → 空转通过 |
| 4 | **`:1643`** | `test_kill_and_release_all_kills_non_bg_and_preserves_bg`（:1623） | **kill + 选择性保留** | 该测试的 `normal_id` 终端（应被杀） |
| 5 | **`:1666`** | **同一个测试**（:1623） | 同上 | 该测试的 `bg_id` 终端（应被保留）。**#4 与 #5 是同一用例的两次调用，不是两个用例** |
| 6 | **`:1827`** | `test_kill_and_release_all_noop_for_other_sessions`（:1806） | **noop / 会话隔离** | session B 的终端，被 session A 的 `kill_and_release_all_for_session` 清理时不应受影响；断言 `get_terminal_output(&session_b, &id_b).is_some()`。bash 下进程秒死但注册记录可能仍在 → 空转通过 |

**共 6 个坐标、5 个测试用例。** 派工与验收都按 5 个用例计。

---

## 三、切片任务书

### §3.1 切片 1 —— 进程组 kill（1 个坐标：`:1305`）

**目标**：让 `process_group_probe_command()` 在 Git Bash 下返回真实可杀的 bash snippet，
使 `test_process_group_kill_with_session` 的进程组 kill 语义被真实执行。

**现状**：`"Start-Sleep -Seconds 300; Start-Sleep -Seconds 300"`（600s，两段连续）。

**要求**：
1. 用 `shell_snippet(ps, sh)` 改造，bash 侧等价物为两段连续睡眠（保持 600s 量级，
   **不得缩短到"够用就行"**——长睡眠是这个测试的前提，缩短会让"进程组 kill"失去意义）。
2. **加 start/finish 副作用**：让 probe 在睡眠前后各落一个标记，测试断言
   start 存在（进程真活过）+ kill 后 finish 不存在（真被打断）。
3. **加对照组**：同结构、短睡眠（如 300ms）、不 kill，断言两标记都出现。
4. 保留测试原有的全部断言（`InProgress` 轮询、signal 返回、`exit_code == Some(1)`），
   **不得放宽或删除任何一条**。

**为什么这片最难**：它验的是**进程组** kill（`CREATE_NEW_PROCESS_GROUP`），
而 bash 侧 snippet 若产生多层进程树，进程组边界可能与 PowerShell 不同。

**用户 2026-11-06 裁定（本片最硬的约束）**：若 bash 与 PowerShell 的进程组语义差异
**无法消除**，**立即停止该切片并带回证据**——不得 skip、不得缩短 600 秒睡眠、
不得降低或删除任何原有断言、不得提交半成品。停片时报告须包含：
- 两种 shell 下实测的进程树形态（PID/PPID 链，用 CIM 取，**不要用 bash 的 `$!`**，
  那是 MSYS PID 不是 Windows PID）
- 两侧 kill 的实际效果对比（哪些进程死了、哪些活着）
- 差异为何无法消除的具体原因

**若差异可消除**：在报告中说明是如何消除的，并给出双 shell 的等价性证据。

### §3.2 切片 2 —— 输出捕获（1 个坐标：`:1561`）

**目标**：让 `test_ext_create_output_wait_release` 在 Git Bash 下真实跑通并验证输出捕获。

**现状**：`"Write-Output hello"`；测试已断言 `exit_code == Some(0)` 与
`output.output.contains("hello")`。

**要求**：
1. 用 `shell_snippet(ps, sh)` 改造，bash 侧用 `echo hello`（或等价）。
2. **输出类验收标准**：必须断言**真实捕获的内容**，不能只看退出码。现有
   `contains("hello")` 已是真实断言——但你要**证明它不是恒真的**：
   在报告中给出"若 snippet 不产出 hello，该断言会失败"的证据
   （例如临时把 snippet 改成不输出的形式，展示测试变红，再改回）。
3. 检查 bash 与 PowerShell 下捕获到的输出是否一致（行尾、编码）。
   若 `echo hello` 在 bash 下产出 `hello\r\n` 而 PS 下是 `hello\n`，
   确认 `contains("hello")` 两者都成立，并在报告说明。
4. 对照组：本片是输出类，不需要"不 kill 对照"；但要确认
   `wait_for_terminal_exit` 等到的是**命令自然结束**而非进程崩溃。

### §3.3 切片 3 —— release 清理（1 个坐标：`:1603`）

**目标**：让 `test_ext_release_cleans_up` 的 release 语义被真实执行。

**现状**：`"Start-Sleep -Seconds 30"`；bash 下进程秒死，release 一个已死的终端 → 空转。

**要求**：
1. 用 `shell_snippet` 改造为 bash 等价物（`sleep 30`）。
2. **副作用验收**：release 之前必须证明终端**真的在运行**（用 start/finish 标记，
   或等价手段）；release 之后断言清理生效。
3. 若 release 会打断睡眠，需断言 finish 标记不存在（证明是被 release 打断而非自然结束）。
4. 对照组：同结构、短睡眠、**不 release**，断言两个标记都出现
   → 证明"finish 缺失"确是 release 的效果。

### §3.4 切片 4 —— kill 全部但保留后台（**2 个坐标、1 个用例**：`:1643` + `:1666`）

**目标**：让 `test_kill_and_release_all_kills_non_bg_and_preserves_bg` 的两个终端
在 Git Bash 下都真实存活，使"杀非后台、保留后台"的选择性语义被真实验证。

**现状**：`:1643` 是 `normal_id`（应被杀），`:1666` 是 `bg_id`（应被保留）。
两者当前都是 `"Start-Sleep -Seconds 30"`。bash 下两个都秒死 → 测试空转。

**要求**：
1. 两处都要用 `shell_snippet` 改造（可共用同一次 helper 调用，但**两处的
   tag 必须不同**，否则标记文件互相覆盖）。
2. **副作用验收**：
   - 两个终端都必须先证明活着（各自的 start 标记存在）
   - kill 之后：`normal_id` 的 finish 标记**不存在**（真被杀），
     `bg_id` 的 finish 标记也**不存在**（它还在睡，没跑完）
   - 关键：必须能区分"bg 因为被保留而活着"与"bg 因为本来就死了"——
     用 start 标记的存在来区分（死了就不会有 start）
3. **对照组**：同结构、短睡眠、不 kill，断言四个标记（两个 start + 两个 finish）都出现。
4. 报告须说明：改造后"保留后台"这一语义在 bash 路径下如何被证明
   （提示：bg 终端在 kill 之后仍应可查询到输出/状态）。

### §3.5 切片 5 —— 会话隔离 noop（1 个坐标：`:1827`）

**目标**：让 `test_kill_and_release_all_noop_for_other_sessions` 真正验证
"清理会话 A 不影响会话 B 的存活终端"。

**现状**：`:1827` 是 session B 的终端，`"Start-Sleep -Seconds 30"`；
测试调用 `kill_and_release_all_for_session(&session_a)` 后断言
`get_terminal_output(&session_b, &id_b).is_some()`。bash 下进程秒死，
但注册记录可能仍在 → 断言空转通过。

**要求**：
1. 用 `shell_snippet` 改造。
2. **副作用验收**：在调用 `kill_and_release_all_for_session(session_a)` **之前**，
   必须证明 session B 的终端真的在运行（start 标记存在）。
3. 调用之后断言：start 标记仍在、finish 标记**不存在**（终端仍活着，没跑完也没被杀）。
4. **对照组（用户 2026-11-06 裁定，两种合法形式，任选其一）**：
   - **形式 A —— 独立正向对照**：**不得改变原测试的清理目标**，保留"清理 A 不影响 B"
     的原意图不动；另设一个独立的正向对照（新测试或用例），证明"清理 B 会移除 B"，
     从而反证原测试的"不影响"不是因清理本身无效而假阳性。
   - **形式 B —— 临时变异取证**：把清理目标临时改成 session B，跑一次证明 B 被移除
     （记录红/绿证据），**随后立即回滚**，原测试的清理目标必须恢复为 session A。
     报告须附回滚后的 `git diff` 证明目标已还原。
   - **两种都不许"假装有对照"**：若两种都构造不出来，如实说明原因并判 SPEC GAP，
     不得用"看起来合理"的叙述替代实证。

---

## 四、验收 checklist（每片指挥官复核用）

- [ ] 只改了本片点名坐标，`git diff --stat` 仅 1 个文件
- [ ] 未触碰 `JobObject` / `KillOnDrop` / `CreationFlags` 三行
- [ ] 未新增"进程树消失"类断言
- [ ] 未删除或放宽任何原有断言（逐条比对）
- [ ] Git Bash 下 `terminal::` 全绿、0 skip
- [ ] PowerShell 下 `terminal::` 全绿、0 skip
- [ ] C2 两条回归测试仍绿
- [ ] 无 `sleep.exe` 残留、无 `qidi-terminal-probe-*` 标记残留
- [ ] 回执三要素齐（命令+原始输出 / 改动摘要 / 未验证项声明）
- [ ] 若为 kill 类：start/finish + 不 kill 对照三件套齐全

## 五、已知不在本轮范围（明确排除，防scope creep）

- `:1290` 附近的 doc comment 更新（`process_group_probe_command` 的文档串描述）
  ——可顺手，但不得单独成为一片
- 生产代码任何改动
- "进程树消失"测试缺口的补齐（另开后续小单）
- 其他 crate 的测试
