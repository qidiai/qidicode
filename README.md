# QIDI Code

> AI 驱动的命令行编码助手，原生支持 Windows / macOS / Linux。

QIDI Code 是一个基于 Rust 构建的终端 AI 编码工具，支持多模型 Provider（OpenAI、Anthropic、xAI Grok 等），提供代码生成、重构、审查、调试、自动化工作流等能力。

## ✨ 核心特性

- **多 Provider 支持**：OpenAI（GPT-4o/o1）、Anthropic（Claude 3.5 Sonnet）、xAI（Grok），可插拔切换
- **原生 Windows 支持**：不依赖 WSL，直接在 Windows 终端 / PowerShell / CMD 中运行
- **交互式 TUI**：基于 ratatui 的终端界面，支持 Markdown 渲染、Mermaid 图表、语法高亮
- **Agent 工作流**：支持子 Agent、Best-of-N、多步骤任务编排
- **工具集成**：内置 20+ 工具（文件读写、bash 执行、代码搜索、git 操作等）
- **MCP 协议**：支持 Model Context Protocol，可扩展外部工具
- **会话管理**：会话持久化、恢复、导出 Markdown
- **记忆系统**：跨会话记忆，支持 `.md` 笔记
- **沙箱执行**：可选的进程沙箱。Unix 用 Landlock/Seatbelt（内核级文件/网络限制）；Windows 用 Job Object（约束较 Unix 弱，主要提供进程组 kill-on-close，无等价的文件系统 deny）；部分构建平台可能降级为 stub
- **可配置**：分层配置（requirements > managed > user > project），TOML 格式

## 📦 安装

### 从源码构建

```bash
git clone <repo-url>
cd qidicode

# Windows 需要设置 protoc 路径
set PROTOC=<path-to-protoc.exe>

cargo build --release
# 二进制位于 target/release/qidi.exe (Windows) 或 target/release/qidi (Unix)
```

### 环境要求

- Rust 1.85+ (stable)
- protoc 27.5+（用于编译 protobuf 定义）
- Git（用于版本控制工具）

## 🚀 快速开始

### 首次运行

```bash
# 启动交互式 TUI
qidi

# 或以 Agent 模式运行（无 TUI）
qidi agent "帮我重构这个函数"

# 指定模型
qidi -m claude-3-5-sonnet "解释这段代码"
qidi -m gpt-4o "写一个单元测试"
```

### 配置

QIDI Code 的配置目录默认为 `~/.qidi/`，支持以下环境变量：

| 环境变量 | 说明 | 默认值 |
|---------|------|--------|
| `QIDI_HOME` | 配置目录 | `~/.qidi` |
| `QIDI_TEST_VERSION` | 测试用版本覆盖 | - |
| `QIDI_VERSION` | 编译时版本覆盖 | Cargo 包版本 |

> **向后兼容**：`GROK_HOME`、`XAI_API_KEY`、`GROK_API_KEY` 环境变量仍被支持，用于从旧版 grok CLI 迁移。

配置文件结构：
```
~/.qidi/
├── config.toml           # 用户配置
├── managed_config.toml   # 管理员配置（企业策略）
├── requirements.toml     # 需求约束（签名）
├── memory/               # 跨会话记忆（.md 文件）
├── sessions/             # 会话历史
└── bin/                  # 自带工具二进制
```

### API Key 配置

支持通过环境变量或配置文件设置 API Key：

```bash
# OpenAI
export OPENAI_API_KEY="sk-..."

# Anthropic
export ANTHROPIC_API_KEY="sk-ant-..."

# xAI Grok（向后兼容）
export XAI_API_KEY="xai-..."
export GROK_API_KEY="xai-..."  # 旧版别名
```

### 自定义模型端点（base_url 覆盖）

在 `~/.qidi/config.toml`（或项目级 `.grok/config.toml`）中通过 `[model.<id>]` 节可以覆盖内置模型的 `base_url`，或新增一个自定义模型条目。典型场景：把 OpenAI 兼容请求指向本机 AI Bridge 反代网关。

```toml
# 通过 AI Bridge 反代网关使用模型
[model."agnes-2.0-flash"]
model = "agnes-2.0-flash"
base_url = "http://127.0.0.1:9800/v1"
api_backend = "chat_completions"   # chat_completions | messages | responses
auth_scheme = "bearer"             # bearer | x_api_key
env_key = "AI_BRIDGE_TOKEN"        # 网关无 token 时可不设置该环境变量，请求不带 Authorization 头
context_window = 128000
fallback_models = ["glm-5.2"]      # 上游 5xx/超时且重试耗尽时按顺序切换到这些模型
```

说明：

- **同名覆盖 / 新增**：`[model.<id>]` 的 `<id>` 与内置模型同名时覆盖内置默认；新 ID 视为新增模型。
- **环境变量覆盖**：`QIDI_LLM_BASE_URL_<MODEL大写下划线化>`（如 `QIDI_LLM_BASE_URL_AGNES_2_0_FLASH`）可临时覆盖 base_url，优先级：**ENV > config.toml > 内置默认**。
- **base_url 归一化**（仅 `chat_completions`）：末尾斜杠自动去除；`http://127.0.0.1:9800` 与 `http://127.0.0.1:9800/v1` 两种写法等价（缺省路径时自动补 `/v1`）；误贴完整端点 `…/v1/chat/completions` 会剥回基址。带自定义路径的 URL（如 Azure 风格）保持原样。
- **故障转移（fallback_models）**：在 `[model.<id>]` 中配置 `fallback_models = ["glm-5.2"]` 后，主对话请求遇到**可转移失败**（连接错误、超时、HTTP 5xx）且重试耗尽时，按顺序自动切换到列表中的下一个模型（使用该模型自己的 base_url / 凭据），成功即继续；4xx 认证/参数错误不转移。链中不存在的模型 ID 会跳过并告警，重复/成环的条目自动去重，仅展开一层（不递归 fallback 的 fallback）；标题摘要等辅助请求不参与转移。
- **故障转移重试上限**：配置了 `fallback_models` 时，可转移失败最多重试 **2** 次即切换（可用 `QIDI_FAILOVER_MAX_RETRIES` 调整），避免在坏端点上耗完全部重试预算（默认 15 次、约 6 分钟）；`QIDI_MAX_RETRIES` 仍是全局上限（两者取较小值）。未配置 fallback 时重试行为不变。

## 📖 命令参考

```bash
qidi                          # 启动交互式 TUI（默认）
qidi agent "提示词"            # Agent 模式（无 TUI）
qidi inspect                  # 查看当前目录的配置
qidi login --oauth            # OAuth 登录
qidi logout                   # 登出并清除凭证
qidi models                   # 列出可用模型
qidi sessions                 # 管理会话
qidi export <session-id>      # 导出会话为 Markdown
qidi update                   # 检查/安装更新
qidi version                  # 打印版本信息
qidi completions <shell>      # 生成 shell 补全脚本
qidi worktree                 # 管理 git worktree
qidi mcp                      # 管理 MCP 服务器配置
qidi plugin                   # 管理插件
qidi memory                   # 管理跨会话记忆
```

### 自动批准（YOLO）与沙箱

`--yolo`（等价 `--always-approve`）让 agent 自动批准工具调用，适合 CI 等无人值守场景。

- **全开 YOLO 请配合沙箱**：`qidi -p "..." --sandbox=readonly --yolo`。全开 YOLO 且沙箱未激活时无任何 OS 层防护，应显式确认风险后再用。
- **工具白名单收敛风险**：权限层支持 YOLO 工具白名单（`cf-workspace` 的 [`YoloMode`](crates/codegen/cf-workspace/src/permission/yolo.rs)），仅放行选定工具（如 `read_file`、`grep`）自动批准，其余照常审批；无沙箱时优先用白名单而非全开。

## 🏗️ 项目结构

```
qidicode/
├── crates/
│   ├── codegen/              # 核心代码生成与 Agent 逻辑
│   │   ├── cf-config/           # 配置加载（原 xai-grok-config）
│   │   ├── cf-tools/            # 工具实现（原 xai-grok-tools）
│   │   ├── cf-workspace/        # 工作区管理（原 xai-grok-workspace）
│   │   ├── cf-pager/            # TUI 界面（原 xai-grok-pager）
│   │   ├── cf-shell/            # Agent 运行时（原 xai-grok-shell）
│   │   └── ...
│   ├── common/               # 共享库
│   └── build/                # 构建工具（proto 编译等）
├── third_party/              # 第三方依赖
└── Cargo.toml                # Workspace 根配置
```

## 🔧 开发

```bash
# 编译检查（快速）
cargo check --workspace

# 完整构建
cargo build --workspace

# 运行测试
cargo test --workspace

# 构建 release
cargo build --release
```

### Windows 开发注意事项

- 需要 `PROTOC` 环境变量指向 `protoc.exe`
- 使用 `rsproxy.cn` 镜像加速依赖下载（已配置在 `.cargo/config.toml`）
- 某些 Unix 专有功能（Landlock 沙箱、jemalloc）在 Windows 上自动禁用

## 📄 许可证

Apache-2.0

## 🙏 致谢

QIDI Code 基于 grok-build 开源项目改造，感谢原始项目的贡献者。
