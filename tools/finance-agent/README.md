# Finance Agent

本地优先的 AI 财务代理（Rust），支持自然语言记账、财务报表生成、银行对账。

## 功能

- 自然语言录入交易（-n / --input）
- 三大报表：利润表 / 资产负债表 / 试算平衡
- 交互式 REPL（--interactive）
- Web API 服务（--serve，默认 8080）
- 技能扩展：发票 OCR（占位）、报表、预算、对账

## 配置

### finance_config.json（项目根）

支持 `finance_config.json` 与 环境变量回退：

| 字段 | 说明 | 环境变量 | 默认值 |
|------|------|----------|--------|
| llm_api_base | LLM API 地址 | LLM_API_BASE | https://api.deepseek.com/v1 |
| llm_api_key | API Key | LLM_API_KEY | 无 |
| llm_model | 模型名 | LLM_MODEL | deepseek-chat |
| default_entity | 默认实体名 | - | default |
| database_path | 数据库路径 | - | ./finance.db |

复制 `finance_config.json.example` 为 `finance_config.json` 并填入实际值。

## 使用

```powershell
# 编译
cd tools/finance-agent
cargo build --release

# 自然语言记账
cargo run -- -n "买菜花了 50 元"

# 出报表
cargo run -- --report income_statement
cargo run -- --report balance_sheet
cargo run -- --report trial_balance

# REPL
cargo run -- --interactive

# Web 服务
cargo run -- --serve --port 8080
```

## Web API

| 路由 | 方法 | 说明 |
|------|------|------|
| / | GET | 健康检查 |
| /api/chat | POST | 自然语言对话/记账 |
| /api/accounts | GET | 科目列表 |
| /api/reports/{report_type} | GET | 报表（income_statement / balance_sheet / trial_balance） |

## 项目结构

```
tools/finance-agent/
├── Cargo.toml
├── finance_config.json.example
├── src/
│   ├── main.rs          # CLI + Web 入口
│   ├── config.rs        # 配置加载（finance_config.json + 环境变量）
│   ├── agent.rs         # Agent 核心 + LLM 工具调用
│   ├── db.rs            # SQLite 数据库（async 封装）
│   ├── ledger.rs        # 复式记账引擎
│   ├── export.rs        # CSV 导出
│   ├── models/          # 数据模型
│   └── skills/          # 可扩展技能
│       ├── invoice.rs   # 发票 OCR（占位）
│       ├── report.rs    # 报表生成
│       ├── budget.rs    # 预算检查
│       ├── reconcile.rs # 银行对账
│       └── chat.rs      # 对话
└── migrations/          # 数据库迁移（预留）
```

## License

Apache-2.0

### Web API ��Ȩ
���� --serve ʱ����������˻������� FINANCE_API_KEY�������� /api/* ·����Ҫ�� X-API-Key header��
``powershell
$env:FINANCE_API_KEY = "my-secret-key"
cargo run -- --serve
``nδ����ʱ������Ȩ�������ؿ����Ƽ�����

