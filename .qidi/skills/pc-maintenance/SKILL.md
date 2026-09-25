---
name: pc-maintenance
description: >
  Windows 电脑维护技能：修复 PowerShell 中文乱码、执行策略禁止脚本、bat 文件引号嵌套
  解析失败等常见问题。当用户要求"修复 PowerShell"、"中文乱码"、"脚本禁止运行"、
  "执行策略"、"引号嵌套"、"电脑维护"、"PowerShell 编码"、"gb2312 乱码"、
  "/pc-maintenance"时使用。
---

# 电脑维护（Windows PowerShell 环境修复）

本技能解决中文 Windows 环境下 PowerShell 的三类高频问题：
1. **中文乱码**：控制台默认 GB2312 编码，与 UTF-8 工具不兼容
2. **脚本禁止运行**：执行策略默认 Restricted
3. **引号嵌套解析失败**：bat 内 `powershell -Command "..."` 多层引号断裂

---

## 步骤 1：诊断环境

运行以下命令收集信息：

```powershell
Write-Output ("PSVersion: " + $PSVersionTable.PSVersion)
Write-Output ("OutputEncoding: " + [Console]::OutputEncoding.WebName)
Write-Output ("InputEncoding: " + [Console]::InputEncoding.WebName)
Write-Output ("ProfileExists: " + (Test-Path $PROFILE))
Write-Output ("Profile: " + $PROFILE)
Get-ExecutionPolicy -List
```

### 判断标准

| 指标 | 正常值 | 需修复值 |
|------|--------|----------|
| OutputEncoding | utf-8 | gb2312 |
| InputEncoding | utf-8 | gb2312 |
| ProfileExists | True | False |
| CurrentUser 执行策略 | RemoteSigned / Unrestricted | Restricted |

---

## 步骤 2：创建 PowerShell Profile（修复中文乱码）

在 PowerShell 中执行以下命令，创建带 UTF-8 BOM 的 profile 文件：

```powershell
New-Item -ItemType Directory -Force (Split-Path -Parent $PROFILE) | Out-Null
$lines = @(
'# === UTF-8 console fix for zh-CN Windows PowerShell 5.1 ===',
'try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; [Console]::InputEncoding = [System.Text.Encoding]::UTF8 } catch {}',
'$OutputEncoding = [System.Text.Encoding]::UTF8',
"`$PSDefaultParameterValues['*:Encoding'] = 'utf8'"
)
[System.IO.File]::WriteAllLines($PROFILE, $lines, (New-Object System.Text.UTF8Encoding($true)))
Write-Output "Profile created: $PROFILE"
```

### 关键点
- **必须用 `UTF8Encoding($true)`**（带 BOM），否则 PowerShell 5.1 会把无 BOM 文件按 GBK 读取
- profile 内容做三件事：
  1. `[Console]::OutputEncoding` / `InputEncoding` → UTF-8（控制台 I/O）
  2. `$OutputEncoding` → UTF-8（管道传给外部程序）
  3. `$PSDefaultParameterValues['*:Encoding'] = 'utf8'`（cmdlet 默认编码）

### 验证
```powershell
powershell -NoProfile -Command "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; Write-Output '中文测试：程序开发、标书软件'"
```
输出中文无乱码即成功。

---

## 步骤 3：设置执行策略（修复脚本禁止运行）

```powershell
Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser -Force
Get-ExecutionPolicy -List
```

### 说明
- `RemoteSigned`：本地脚本可直接运行，网上下载的脚本需数字签名（安全）
- 用 `-Scope CurrentUser` 不需要管理员权限
- 设置后**必须新开 PowerShell 窗口**才能生效

---

## 步骤 4：修复 bat 文件引号嵌套

### 问题模式
bat 文件中 `powershell -Command "..."` 内含多层引号时容易断裂：

```bat
# ❌ 脆弱写法：三层嵌套引号
for /f "usebackq delims=" %%D in (`powershell -Command "Add-Type ...; if ($f.ShowDialog() -eq 'OK') { ... }"`) do ...
```

### 修复模式：抽成独立 .ps1 文件

1. **创建独立脚本**（如 `.qidi\ops\pick_folder.ps1`），必须用 **UTF-8 with BOM** 编码：

```powershell
# 创建带 BOM 的 ps1 文件
$enc = New-Object System.Text.UTF8Encoding($true)
$content = @'
Add-Type -AssemblyName System.Windows.Forms
$f = New-Object System.Windows.Forms.FolderBrowserDialog
$f.Description = 'Select the project folder'
if ($f.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
    $f.SelectedPath
}
'@
[System.IO.File]::WriteAllText("path\to\pick_folder.ps1", $content, $enc)
```

2. **bat 中用 `-File` 替代 `-Command`**：

```bat
# ✅ 稳定写法：无嵌套引号
for /f "usebackq delims=" %%D in (`powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0.qidi\ops\pick_folder.ps1"`) do set "TARGET=%%D"
```

### 规则
- bat 调用 PowerShell：**优先用 `-File`，避免 `-Command`**
- 必须用 `-Command` 时：单引号包脚本逻辑、双引号包参数，最多两层
- `for /f` 语法是 **cmd 批处理专用**，不能粘到 PowerShell 窗口里执行

---

## 步骤 5：检查 .ps1 脚本编码

所有含中文的 `.ps1` 脚本必须有 UTF-8 BOM，否则 PowerShell 5.1 按 GBK 读取中文注释会乱码。

### 批量检查脚本编码

```powershell
Get-ChildItem ".qidi\ops\*.ps1", ".qidi\services" -Recurse -Filter *.ps1 | ForEach-Object {
    $bytes = [System.IO.File]::ReadAllBytes($_.FullName)
    $hasBom = ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF)
    $hasNonAscii = $false
    foreach ($b in $bytes) { if ($b -gt 127) { $hasNonAscii = $true; break } }
    Write-Output ("{0,-30} BOM={1,-5} NonAscii={2}" -f $_.Name, $hasBom, $hasNonAscii)
}
```

### 修复无 BOM 的中文脚本

```powershell
$enc = New-Object System.Text.UTF8Encoding($true)
$content = [System.IO.File]::ReadAllText("target.ps1")  # 自动检测现有编码
[System.IO.File]::WriteAllText("target.ps1", $content, $enc)
```

### 规则
- 含中文（NonAscii=True）的 `.ps1`：**必须有 BOM**
- 纯英文脚本（NonAscii=False）：BOM 可有可无，不影响

---

## 完整验证清单

修复完成后逐项验证：

- [ ] 新开 PowerShell 窗口无报错（profile 正常加载）
- [ ] `Write-Output '中文测试'` 输出无乱码
- [ ] 中文路径（如 `G:\程序开发\`）显示正确
- [ ] `Get-ExecutionPolicy` 显示 `RemoteSigned`
- [ ] bat 中 `powershell -File` 调用正常弹出对话框
- [ ] 引号嵌套文本（`'单引号' 和 "双引号"`）正常输出

---

## 换电脑/重装系统后的恢复

只需重跑两步即可恢复：

```powershell
# 1. 创建 profile
New-Item -ItemType Directory -Force (Split-Path -Parent $PROFILE) | Out-Null
$lines = @(
'try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; [Console]::InputEncoding = [System.Text.Encoding]::UTF8 } catch {}',
'$OutputEncoding = [System.Text.Encoding]::UTF8',
"`$PSDefaultParameterValues['*:Encoding'] = 'utf8'"
)
[System.IO.File]::WriteAllLines($PROFILE, $lines, (New-Object System.Text.UTF8Encoding($true)))

# 2. 设置执行策略
Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser -Force
```

然后新开 PowerShell 窗口即生效。