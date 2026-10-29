# Security Policy

## Reporting a Vulnerability

QIDI Code 是基于 grok-build 改造的 Rust AI 编码助手。我们非常重视安全问题。

**请勿在公开 GitHub Issue 中提交安全漏洞报告。**

请通过以下任一渠道私下报告:

- **GitHub Security Advisory**(首选):
  https://github.com/qidiai/qidicode/security/advisories/new
- **邮箱**:security@qidi.ai

<!-- TODO(需维护者确认，勿静默修改):
     1) 仓库地址已按 `git remote -v`(github = https://github.com/qidiai/qidicode.git)
        校正；GitHub API 确认 https://github.com/qidiai/qidicode 存在且 public=true，
        而原写法 qidi-ai/qidicode 返回 404（仓库不存在）。
        `/security/advisories/new` 这条路径本身未能从当前网络做 HTTP 校验
        （github.com 网页端不可达），需确认该仓库已开启 private vulnerability reporting。
     2) 邮箱域名待确认：项目自有域名为 qidiai.ltd（站点 qidiai.ltd /
        qidiwork.qidiai.ltd 均返回 200），若 security@qidi.ai 不可达，
        建议改为 security@qidiai.ltd。 -->

报告时请尽量包含:

- 受影响版本(commit SHA 或 release tag)
- 复现步骤 / 最小可复现示例
- 评估的影响范围

我们承诺在 **5 个工作日内**确认收到报告,并在 **30 天内**给出初步评估与修复计划。

## Supported Versions

| 版本 | 是否接受安全修复 |
|------|------------------|
| 最新 `main` 分支 | ✅ |
| 历史发布 tag | 视影响程度而定 |

## Scope

在范围内:

- QIDI Code CLI 二进制(`qidi` / `qidi.exe`)
- `crates/` 下所有 `cf-*` 工作区成员
- 配置加载与凭据存储逻辑(`cf-config`、`cf-auth`、`cf-secrets`)

不在范围内(请向对应上游报告):

- `third_party/` 下的第三方依赖
- 用户自行接入的 MCP 服务器 / 外部 Auth Provider 二进制
- 用户机器本身的 OS 级凭据存储安全(keychain / credential manager)
