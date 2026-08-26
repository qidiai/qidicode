---
name: secret-scan
description: >
  扫描 GitHub 公开仓库，查找意外提交的 API Key、Secret、Token 等敏感信息。
  当用户要求"扫描仓库密钥"/"查找泄漏 key"/"secret scan"/"扫描 github" 时使用。
triggers:
  - secret scan: 扫描仓库密钥/查找泄漏 key/secret scan/扫描 github/扫密钥
  - 开源审计: 扫描公开项目/审计密钥泄漏/找泄漏
  - 批量扫描: 扫描多个仓库/批量检测 key
metadata:
  short-description: "扫描 GitHub 公开仓库的密钥泄漏"
---

# /secret-scan -- GitHub 密钥泄漏扫描器

通过 GitHub 公开 API 搜索并克隆仓库，使用本地扫描检测密钥泄漏。

## 用法

| 命令 | 说明 |
|------|------|
| `/secret-scan` | 默认搜索近期含 api_key 的活跃仓库 |
| `/secret-scan "关键词"` | 自定义搜索词 |
| `/secret-scan --org <org>` | 扫描指定 org（需 gh 登录） |

## 扫描脚本

脚本位于：`C:\Users\ASUS\.qidi\skills\secret-scan\scripts\scan.py`

```powershell
python "$env:TEMP\gh_secret_scan.py" "关键词"
```

## 检测模式

| 类型 | 正则 |
|------|------|
| OpenAI/Agnes/DeepSeek Key | `sk-[a-zA-Z0-9]{20,}` |
| AWS Access Key | `AKIA[A-Z0-9]{16}` |
| JWT Token | `eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+` |
| Private Key | `-----BEGIN (RSA|EC|OPENSSH) PRIVATE KEY-----` |
| Stripe Key | `sk_(live|test)[a-zA-Z0-9]{24,}` |
| Google API Key | `AIza[a-zA-Z0-9_-]{30,}` |

## 安全规范

1. **只扫描公开仓库** — 不访问私有仓库
2. **不公开已确认的 key** — 只报告类型和位置，不展示 key 值
3. **通过 GitHub Security Advisory 报告** — 不要直接在 issue 中公开 key
4. **不尝试登录或使用发现的 key** — 仅作审计用途
