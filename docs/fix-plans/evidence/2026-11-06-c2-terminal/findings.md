# C2 进程语义探针记录（2026-11-06）

**背景**：C2 把 `kill_probe_command()` 从硬编码 PowerShell `Start-Sleep -Seconds 30`
改为按 shell 选择（Git Bash → `sleep 30`），并用标记文件做副作用验收。原方案的
MSYS2 语义验收未闭环，本探针补上。

**探针复刻对象**：`cf-config/src/shell.rs:357-368` 的 `invocation_for` ——
Git Bash 走 `bash.exe -c <command>`（env 含 `MSYS_NO_PATHCONV=1`、
`MSYS2_ARG_CONV_EXCL=*`、`PYTHONUTF8=1`、`PYTHONIOENCODING=utf-8:surrogateescape`）。

## 结论

### Q1：`sleep` 在 Git Bash 下是外部二进制，不是内建

```
$ bash -c "type -a sleep; which sleep"
sleep is /usr/bin/sleep
/usr/bin/sleep
$ bash --version
GNU bash, version 5.2.37(1)-release (x86_64-pc-msys)
```

因此 bash **必然 fork** 才能跑它。实测进程树（CIM，Windows PID/PPID）：

```
bash.exe (17512, PPID 11568)   ← 生产 spawn 的顶层
└── bash.exe (12632, PPID 17512)
    └── sleep.exe (25484, PPID 12632)   ← /usr/bin/sleep.exe 30
```

**结论：MSYS2 没有"内建 sleep 导致进程树变浅"的问题**——`sleep` 是独立可执行文件，
进程树与 PowerShell 路径的差异只在于 PowerShell 的 `Start-Sleep` 是 cmdlet
（无独立子进程），而 bash 的 `sleep` 是外部进程（多一层）。这个差异**不影响**
kill 语义的真实性：两条路径都有真实的、需要被中断的活动进程。

### Q2：kill 后 probe 及其子进程是否全部消失 —— 生产零泄漏

**生产路径**（`streaming_local_terminal.rs:943-947`）：

```rust
cmd.wrap(CreationFlags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW));
// JobObject + KillOnDrop: dropping terminates every descendant.
cmd.wrap(JobObject);
cmd.wrap(KillOnDrop);
```

即 spawn 时把 bash 放进 Windows **Job Object** 并设 `KillOnDrop`，
Job 内所有后代随顶层终止而被终结。

**经验证据**（决定性，非推理）：

| 场景 | kill 后立刻查 `sleep.exe 30` |
|---|---|
| 生产路径（跑 `test_kill_returns_signal` 后立刻查） | **0 个** ✅ |
| 对照组（裸 `bash.exe -c "sleep 30"`，无 Job Object，同口径 kill） | **有残留** ✅ 泄漏 |

对照组证明探针**具备检出泄漏的能力**（不是测不出），生产路径零残留因此是有效结论。

### Q3：PowerShell 同口径对照

`powershell.exe -Command "Start-Sleep -Seconds 30"` 的进程树只有一个
`powershell.exe`（`Start-Sleep` 是 cmdlet，无独立子进程），kill 后即消失。
两条路径在"有真实活动进程需要被中断"这一点上同构，副作用验收（标记文件）
对两者同样有效。

## 已知残留缺口（不阻塞本次提交，建议后续增量）

**没有任何测试断言"进程树已消失"**。现有标记文件验收只证明"睡眠被打断"，
不证明"sleep.exe 已被回收"。虽然 Job Object 从设计与实测两端都保证回收，
但这条保证目前**无测试钉住**——将来若有人改动 spawn 的 Job Object 包装
（例如误删 `cmd.wrap(JobObject)`），测试仍会全绿而进程开始泄漏。

建议后续增量（与剩余 5 处 snippet 同批或单独）：在 kill 测试末尾加一条
"无 `sleep.exe <N>` 残留"的断言，把 Job Object 的保证变成可验证事实。

## 探针方法论教训（已耗费两轮）

第一轮探针用 bash 的 `$!` 取 PID——那是 **MSYS PID，不是 Windows PID**，
kill 错了对象，得出虚假的"泄漏"结论。第二轮改为"命令内注入唯一标记 →
CIM 按标记反查 Windows PID → kill 该 PID"，才得到有效结论。

**教训：在 Windows 上探测 MSYS 进程树，必须用 CIM/Win32_Process 的
ProcessId/ParentProcessId，bash 作业控制的 `$!` 不可用作 Windows PID。**
