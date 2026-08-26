# qidicode-portable

qidi.exe（Codely/团结 CLI agent）便携运行环境，含本地模型中转与联网搜索增强。

## 快速开始

```bat
启动qidi.bat              :: 自愈拉起本地服务（8765 relay / 8766 websearch shim）后进入 CLI
在指定目录启动qidi.bat     :: 以任意目录为工作区启动
```

## 目录结构

```
启动qidi.bat / 在指定目录启动qidi.bat   启动器（含服务自愈）
使用说明.txt                            用户向导
.qidi/
  ├─ config.toml     模型/密钥配置（含真实密钥，不入 git）
  ├─ ops/            运维脚本（巡检/自愈/配置校验）
  ├─ ops-README.md   运维手册（架构图、故障速查、日志、归档规则）
  ├─ services/       websearch-shim（联网搜索代理 v2.2，生产位）
  ├─ docs/ skills/   文档与技能
  └─ sessions/ memory/ 等运行时数据
_archive/             历史清理归档（只进不出，按日期组织）
```

## 文档索引

- 使用说明.txt —— 面向使用
- [.qidi/ops-README.md](.qidi/ops-README.md) —— 面向运维（架构 / 巡检 / 故障）
- [.qidi/services/websearch-shim/README.md](.qidi/services/websearch-shim/README.md) —— websearch 增强原理

## git 策略

白名单模式：只追踪启动器、文档、ops/services/skills 脚本；
config.toml（含密钥）、sessions、memory、二进制一律不入库。
