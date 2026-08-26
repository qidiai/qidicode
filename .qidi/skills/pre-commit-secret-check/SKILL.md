---
name: pre-commit-secret-check
description: >
  Git pre-commit hook，检查即将提交的文件中是否包含 API Key、Secret、Token 等敏感信息。
  发现明文 key 时自动阻止提交。当用户要求"提交前检查key"/"pre-commit检查"/"阻止key泄漏"时使用。
triggers:
  - pre-commit: 提交前检查/pre-commit检查/阻止key泄漏/commit前检查
  - 密钥防护: 不要提交key/key检查/敏感信息检查/pre-commit
metadata:
  short-description: "Git pre-commit 密钥检查钩子"
---

# /pre-commit-secret-check -- 提交前密钥安全检查

在 \git commit\ 之前自动扫描暂存文件，防止 API Key、Token、密码等敏感信息被提交到版本控制。

## 安装

### 项目级（推荐）

\\\powershell
# 复制到项目 .git/hooks
Copy-Item "C:\Users\ASUS\.qidi\skills\pre-commit-secret-check\scripts\pre-commit.ps1" .git/hooks/pre-commit
\\\

### 全局（所有项目）

\\\powershell
git config --global core.hooksPath "C:/Users/ASUS/.qidi/skills/pre-commit-secret-check/scripts/hooks"
New-Item -ItemType Directory -Force -Path "C:\Users\ASUS\.qidi\skills\pre-commit-secret-check\scripts\hooks"
Copy-Item "C:\Users\ASUS\.qidi\skills\pre-commit-secret-check\scripts\pre-commit.ps1" "C:\Users\ASUS\.qidi\skills\pre-commit-secret-check\scripts\hooks\pre-commit"
\\\

## 检测模式

| 模式 | 正则示例 |
|------|---------|
| OpenAI | \sk-[a-zA-Z0-9]{20,}\ |
| Agnes | \sk-[a-zA-Z0-9]{20,}\ (同 OpenAI 格式) |
| AWS | \AKIA[A-Z0-9]{16}\ |
| Generic API Key | \pi[_-]?key["']\s*[:=]\s*["'][a-zA-Z0-9]{20,}["']\ |
| JWT | \eyJ[A-Za-z0-9_-]+\.eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\ |
| Password | \password["']\s*[:=]\s*["'][^"']{8,}["']\ |
| Private Key | \-----BEGIN (RSA | EC | OPENSSH) PRIVATE KEY-----\ |
| DeepSeek | \sk-[a-zA-Z0-9]{20,}\ |

## 工作流程

1. 运行 \git diff --cached --name-only\ 获取暂存文件列表
2. 运行 \git diff --cached\ 获取变更内容
3. 应用正则匹配检测敏感信息
4. 如发现疑似 key：
   - 打印文件路径、行号、匹配到的模式
   - 询问用户是否确认是误报（如示例配置）
   - 如确认是真实 key，阻止提交并返回 exit 1
   - 用户确认误报后，允许继续提交

## 排除示例文件

以下文件类型默认不检查（通常是模板/示例）：
- \*.example, \*.template, \*.sample
- \*config.json.example, \*.env.example
- docs/, README.md 中的示例配置

## 使用方式

安装后，每次 \git commit\ 自动运行。无需手动调用。

如需临时跳过检查：
\\\powershell
git commit --no-verify -m "message"
\\\
（仅在自己确认不会泄漏 key 时使用）
