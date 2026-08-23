# 防御性模式（Defensive Patterns）

来之不易的缺陷类别规则：每条对应一类本仓库真实发生或差点发生的缺陷，以规则形式陈述以防复发。测试层面的对应规则见 CONTRIBUTING.md。编写路径、子进程、清理、持久化代码之前先读本文。

维护约定：新条目按来源分为「历史事故」（已发生）与「外部对标」（源自 deepseek-harness docs/defensive-patterns.md，经确认对本仓适用）。每条注明出处。

## 1. 路径处理（Windows 优先仓库）

### 1.1 禁用 `std::fs::canonicalize` / `tokio::fs::canonicalize`

一律使用 `dunce::canonicalize`。原因：Windows 上 `canonicalize` 返回 `\\?\C:\...` verbatim 路径，传给子进程/外部工具时大面积失效。这是 clippy.toml 铁律。
来源：历史事故（本项目 CI/clippy 长期规则）。

### 1.2 删除可能是指向链接的路径：先 `lstat` 再 `unlink`

路径可能是符号链接或 Windows junction 时，先判断 `is_symlink()`（`std::fs::symlink_metadata`），再决定 `remove_file`（只删链接本身）或 `remove_dir_all`（真实目录）。对 junction 直接 `remove_dir_all` 可能**穿透 junction 进入目标目录递归删除**。
来源：外部对标（dsh defensive-patterns）。本仓已有零散防御（signed_policy.rs:243-244、watcher.rs:466-491），本条把它升格为全局规则：所有新增递归删除必须带链接检查。

### 1.3 PowerShell `Set-Content` 会损坏非 ASCII 的 UTF-8 文件

写 UTF-8 文件用 .NET `[System.IO.File]::WriteAllText(path, text, [System.Text.UTF8Encoding]::new($false))` 或编辑器工具，绝不用 `Set-Content`（默认编码/行尾处理会毁内容）。
来源：历史事故（本仓文档与配置文件多次被 PowerShell 管道损坏后修复）。

### 1.4 PowerShell 的 `curl` 是 `Invoke-WebRequest` 别名

在 PowerShell 调用真 curl 必须写 `curl.exe`。本机环境还常见 `r.jina.ai` DNS 污染与代理 CONNECT 中断，网络工具失败时先区分"工具解析错了"和"网络真不通"。
来源：历史事故（调研工作流多次踩坑）。

## 2. 子进程与结果上报

### 2.1 正交结果独立上报

一个结果可以同时具有多种性质：进程可能已超时，却以退出码 0 结束（因为它捕获了终止信号后正常退出）。每个独立事实（`timedOut`、`signal`、`exit_code`）必须各自独立上报，绝不把一个标志的上报嵌套在另一个标志的分支里，否则调用方会把提前终止误判为正常成功。
现状基线：`background_task.rs` 的结构化结果已是独立字段（exit_code/signal 等），本条约束的是所有新增 runner——特别是 Job Object 终止路径（强杀整个进程树时，被杀子进程可能来不及写非零退出码）。
来源：外部对标（dsh defensive-patterns「正交结果独立上报」）。

### 2.2 dispose 必须等待完全停稳，而不是仅发出停止请求

清理流程只发 kill/abort 信号就返回，会留孤儿进程。正确做法：异步等待子进程真正退出（发终止信号后 await done），并在终止前先关闭监听器/通知注册表，让迟到的完成事件保持静默。
来源：外部对标（dsh defensive-patterns）。本仓 run_terminal_command 的 Job Object 语义（终止时杀整个后代树）符合此精神，新增后台任务遵循同标准。

## 3. 持久化（updates.jsonl / chat_history.jsonl）

### 3.1 updates.jsonl 只做加法

只追加新事件类型，永不改写/删除已有行。崩溃时最坏情况是"末尾多一行截断的行"，读取端 corruption-tolerant（坏行跳过 + warn）兜底。任何持久化 PR 合并前过三问：旧二进制读新日志？新二进制读旧日志？kill -9 在写入中途？
来源：外部对标（dsh「模型可见即已记录」+ 本仓 dsh-absorption 方案 v2 第 5.3 节，2026-08-23）。

### 3.2 chat_history.jsonl 重写必须保持原子

tmp 文件 + rename（现状 `acp_session.rs:1258-1311` 已实现，含双写者竞态处理）。任何新的整文件重写路径遵循同一模式。
来源：历史事故预防（现有实现的注释记录了双写者竞态的设计原因）。

### 3.3 生成的字节工件，其输入行尾必须被钉住

模板文件被逐字节 XOR 进生成源码（prompt_encrypted.rs）时，行尾漂移 = 加密产物漂移 = CI 门禁红。规则：一切"字节进产物"的输入（模板、fixture、embed 文件）必须有 .gitattributes 行尾钉住 + 生成脚本用 `write_bytes` 显式行尾。
来源：历史事故预防（本仓 2026-08-23 实施模板漂移门禁时发现：仓库无 .gitattributes、脚本文本模式跨 OS 输出不一致）。

## 4. 分发与回调

### 4.1 在分发器中隔离回调异常

用户提供的监听器/钩子抛异常，不得 reject 整个 promise 链或饿死排在后面的监听器。分发循环用 try/catch 包裹并记日志；一个行为不当的订阅者不能破坏核心生命周期。
来源：外部对标（dsh defensive-patterns）。

### 4.2 环境变量与可预测路径不暴露给不可信输入

启动的命令应使用净化后的环境变量（剔除名称匹配 `*KEY*`/`*SECRET*`/`*TOKEN*`/`*PASSWORD*` 的项），防凭据经命令输出或 env 泄漏。临时文件放私有目录、随机名、排他创建（`create_new(true)`）。
来源：外部对标（dsh defensive-patterns）。与本仓 cf-secrets/cf-auth 的既有审计方向一致。

## 5. 新条目维护

新事故复盘（postmortem）产出新规则进本文。复盘门槛（对标 dsh postmortem README，v2 方案 P3-⑩）：**隐蔽**（机制不显而易见）、**系统性**（逃逸原因是测试/工具/约定缺口而非笔误）、**重新发现代价高**（消耗真实调试时间且会复发）。三者齐备才写复盘，复盘必须链接它推动新增的防护措施（测试或本清单条目）。
