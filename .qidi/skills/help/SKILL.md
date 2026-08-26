---
name: help
description: >
  Grok 文档和配置帮助。在用户询问设置、配置、MCP 服务器、认证、技能、斜杠命令、键盘快捷键或任何 Grok 功能时使用。此外，当你检测到用户在设置或入门过程中遇到困难时，主动使用。
metadata:
  short-description: "Grok 文档 — 配置、MCP、认证、技能、命令"
---

# Grok 帮助

回答用户关于 Grok 设置、配置或功能的问题。

## 步骤

1. 如果问题是关于**当前配置**（哪些 MCP 服务器、模型或设置处于活动状态），
   读取 `C:\Users\ASUS\.qidi/config.toml`。MCP 服务器位于 `[mcp_servers.*]` 部分下。

2. 如果问题是关于**如何做某事**（设置、添加 MCP 服务器、创建技能、
   认证、键盘快捷键、故障排除），首先查看 `C:\Users\ASUS\.qidi/docs/user-guide/` 中的用户指南文档。可用指南包括：
   - `01-getting-started.md` — 安装、首次启动、基本交互
   - `02-authentication.md` — 浏览器登录、API 密钥、OIDC、外部认证
   - `03-keyboard-shortcuts.md` — 完整按键绑定参考
   - `04-slash-commands.md` — 所有 / 命令
   - `05-configuration.md` — config.toml、pager.toml、环境变量
   - `06-theming.md` — 主题、外观自定义
   - `07-mcp-servers.md` — MCP 服务器设置与管理
   - `08-skills.md` — 创建和使用技能
   - `09-plugins.md` — 插件市场
   - `10-hooks.md` — 生命周期钩子
   - `11-custom-models.md` — BYOK、Ollama、OpenAI 端点
   - `12-project-rules.md` — AGENTS.md 项目规则
   - `13-memory.md` — 跨会话记忆
   - `14-headless-mode.md` — CLI 脚本与 CI/CD
   - `15-agent-mode.md` — ACP/stdio IDE 集成
   - `16-subagents.md` — 子代理和角色
   - `17-sessions.md` — 会话管理
   - `18-sandbox.md` — 沙箱模式
   - `19-plan-mode.md` — 计划模式
   - `20-background-tasks.md` — 后台任务与监控
   - `21-terminal-support.md` — tmux、SSH、真彩色、剪贴板、/terminal-setup
   阅读用户问题相关的指南。如果 none 匹配，回退到 `C:\Users\ASUS\.qidi/README.md` 获取综合参考。

3. 要为用户**修改配置**，使用 search_replace 编辑 `C:\Users\ASUS\.qidi/config.toml`。

4. 要为用户**创建技能**，请创建 `C:\Users\ASUS\.qidi/skills/<name>/SKILL.md`
   （阅读 `C:\Users\ASUS\.qidi/docs/user-guide/08-skills.md` 了解 SKILL.md 格式）。
