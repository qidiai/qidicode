# Finance Agent

本地优先的 AI 财务代理（Rust），支持自然语言记账、财务报表生成、银行对账。

## 功能

- 自然语言录入交易（-n / --input）
- 三大报表：利润表 / 资产负债表 / 试算平衡
- 交互式 REPL（--interactive）
- Web API 服务（--serve，默认 8080）
- 技能扩展：发票 OCR（stub）、报表、预算、对账

## 依赖

- Rust stable + Cargo
- SQLite3（usqlite bundled feature 会自动编译）

## 配置

环境变量（可选）：

| 变量 | 说明 | 默认值 |
|------|------|--------|
| LLM_API_BASE | LLM API 地址 | https://api.deepseek.com/v1 |
| LLM_API_KEY | API Key | 无（不配置则只能本地记账） |
| LLM_MODEL | 模型名 | deepseek-chat |

## 使用

`powershell
# 编译
cd tools/finance-agent
cargo build --release

# 自然语言记账
cargo run -- -n "买菜花了 50 元"

# 出报表
cargo run -- --report income_statement

# REPL
cargo run -- --interactive

# Web 服务
cargo run -- --serve --port 8080
`

## Web API

| 路由 | 方法 | 说明 |
|------|------|------|
| / | GET | 健康检查 |
| /api/chat | POST | 自然语言对话/记账 |
| /api/accounts | GET | 科目列表 |
| /api/reports/{type} | GET | 报表（income_statement / balance_sheet / trial_balance） |

## 项目结构

`
tools/finance-agent/
├── Cargo.toml
├── src/
│   ├── main.rs          # CLI + Web 入口
│   ├── agent.rs         # Agent 核心 + LLM 工具调用
│   ├── db.rs            # SQLite 数据库（async 封装）
│   ├── ledger.rs        # 复式记账引擎
│   ├── export.rs        # CSV 导出
│   ├── models/          # 数据模型
│   └── skills/          # 可扩展技能
│       ├── invoice.rs   # 发票 OCR（stub）
│       ├── report.rs    # 报表生成
│       ├── budget.rs    # 预算检查
│       ├── reconcile.rs # 银行对账（stub）
│       └── chat.rs      # 对话
└── migrations/          # 数据库迁移（预留）
`

## License

Apache-2.0
