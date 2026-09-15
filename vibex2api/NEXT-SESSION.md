# 下个会话交接清单（qidi-build / vibex2api 主线）

> 本文件由指挥官会话于 2026-09-15 留下。重启会话后按此清单续作。

## 1. 正在后台编译（已脱离 qidi 进程树，与本会话无关）

- **机制**：Windows 计划任务 `qidi-build`（Task Scheduler 服务启动，qidi 退出不影响）
- **脚本**：`C:\qidi-build\build_qidi.ps1`；**日志**：`C:\qidi-build\build_log.txt`
- **源码**：worktree `G:\程序开发\qidicode\qidicode-portable-windows-x64\qidicode-src`，分支 `fix/browser-cjk`
  （含 commit `98deacf fix(browser): CJK insertText...`：browser_type 中文输入修复）
- **编译环境**（本机 GNU 工具链，中文路径需绕行）：
  `CARGO_TARGET_DIR=C:\qidi-build`，`PATH` 前缀 `C:\mingw-tools\mingw64\bin`
- **检查编译结果**：
  ```powershell
  Get-Content C:\qidi-build\build_log.txt -Tail 5
  # 看到 BUILD_EXIT=0 即成功；产物 C:\qidi-build\release\qidi.exe
  ```

## 2. 编译完成后的换装流程

1. 确认没有正在运行的 qidi 会话残留进程（换装只能在新 qidi 会话里做，先备份）：
   ```powershell
   Copy-Item qidi.exe ("qidi.exe.bak-" + (Get-Date -Format "MMdd-HHmm")) -Force
   Copy-Item C:\qidi-build\release\qidi.exe qidi.exe -Force
   .\qidi.exe --version   # 应为 0.1.220-alpha.4 或更新
   ```
2. **验证 CJK 修复**：用 browser_type 在网页输入框输入"中文测试一二三"，
   成功即修复生效（旧版必抛 SyntaxError）。

## 3. 可见浏览器 MCP（已配置，重启会话后生效）

- `.qidi/config.toml` 已追加 `[mcp_servers.chrome-devtools]`（chrome-devtools-mcp 1.9.0，
  msedge 通道，**带窗口**，登录态存 `C:\qidi-browser-profile`）
- 新会话开始后用 `search_tool` 搜 "chrome-devtools" 确认 MCP 工具已连上
- 用途：弹出可见 Edge 窗口 → 用户在窗口里**自己注册/登录 RunningHub** →
  通过 MCP 工具接管操作、提取 localStorage 的 `Rh-Accesstoken`

## 4. 登录后主线（vibex2api 联调）

1. token 写入 `vibex2api/vibex2api.config.json` 的 `token` 字段（或环境变量 RH_ACCESSTOKEN）
2. `python vibex2api/vibex2api.py --check`（应打印 JWT payload / providers / apps）
3. `python vibex2api/vibex2api.py` 起服务 → OpenAI 客户端接 `http://127.0.0.1:8787/v1`
4. 若 VibeX 服务端响应结构与前端 bundle 推断不符（如 /api/apps 包裹格式），
   出 R3 返工单给编码手（规格与协议逆向细节见 `vibex2api/SPEC-T1.md`）

## 5. 待用户决策

- `fix/browser-cjk` 分支（CJK 修复 commit）是否推回 Gitee `xuchangming/qidicode`
  （推法：`git push gitee fix/browser-cjk` 或合入 main；remote 已配好）

## 风险备忘（每次提醒用户）

- vibex2api 属非官方用法，可能违反 RunningHub 用户协议，有封号风险
- "每日100亿Token"是营销话术，免费额度按结算周调用次数计
- CJK 修复已通过 8/8 单测（指挥官亲手复跑），但真机 E2E 待换装后验证
