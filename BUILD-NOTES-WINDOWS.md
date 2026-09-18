# Windows 构建与 CJK 修复复现笔记

> 随 `fix/browser-cjk` 分支（commit `98deacf`）附带，2026-09-16/17 实测记录。
> 修复内容：`browser_type` 工具对非 ASCII（中文/CJK）文本抛
> `SyntaxError: Invalid or unexpected token` —— 根因是旧实现把 `serde_json`
> 字面量嵌进带 `\` 行继续符的多行 JS 模板，CJK 经 CDP `Runtime.evaluate`
> 传输时表达式损坏。新实现 `build_insert_text_expression()` 生成单行、
> 100% ASCII（`\uXXXX` 转义，BMP 外拆 UTF-16 代理对）的表达式。

## 1. 快速验证修复（无需完整编译，~5 分钟 + 首次依赖下载）

```powershell
git clone https://gitee.com/xuchangming/qidicode
cd qidicode
git checkout fix/browser-cjk
rustup show        # 首次会按 rust-toolchain.toml 自动装 stable
cargo test -p cf-tools --lib computer::local::browser
```

预期：`insert_text_expression_*` 4 条断言全绿（含中文、emoji 代理对、
引号/反斜杠转义、单行纯 ASCII 断言）。纯离线逻辑，不需要浏览器。

## 2. 完整编译（Windows，实测 40~90 分钟）

### 前置
- Rust stable（rust-toolchain.toml 自动管理）
- **GNU 工具链的两个坑（重要，踩过）**：
  1. 若 rustup 活动工具链是 `stable-x86_64-pc-windows-gnu`：需要
     `dlltool`（mingw-w64 的 bin 目录加入 PATH，例如
     `C:\mingw-tools\mingw64\bin`），否则链接期报
     `error calling dlltool`。
  2. **MinGW `ld.exe` 无法链接位于非 ASCII（中文）路径下的对象文件**
     （binutils 已知限制）。若仓库被检出到中文路径（如
     `G:\程序开发\...`），必须把构建目录指到纯 ASCII 路径：
     ```powershell
     $env:CARGO_TARGET_DIR = "C:\qidi-build"
     ```
  3. 两个坑都源于 GNU 工具链；MSVC 工具链（`x86_64-pc-windows-msvc`）
     理论上可绕开，但本分支仅在 GNU 工具链上实测过
     （rustc 1.97.1，`8bab26f4`）。
- 无需 DotSlash / protoc（上游 README 的该要求对本 fork 的全量
  release 构建不适用，实测缺省即过）。

### 构建

```powershell
# 方式一：仓库自带脚本（-j 2，写 build_log.txt / build_result.txt）
.\build_release.ps1

# 方式二：手动（16GB/8核 机器 -j 4 实测约 40 分钟收尾）
$env:CARGO_TARGET_DIR = "C:\qidi-build"
cargo build --release -j 4
# 产物: C:\qidi-build\release\qidi.exe（约 295MB）
```

### 换装（如需真机验证）
qidi.exe 运行中时直接 `Rename-Item` 旧文件 + `Copy-Item` 新文件即可
（Windows 允许重命名运行中的 exe，下次启动生效）。回滚 = 把备份名改回。

## 3. 真机 E2E 验证（修复前 vs 后）

在 qidi 会话中对任意网页输入框执行 `browser_type` 输入中文：

- 修复前（旧 exe）：`IO Error: insertText failed: ... SyntaxError:
  Invalid or unexpected token`（V8 报错列号 67，落在旧模板行继续符处）
- 修复后（本分支构建）：正常 `typed into ...`，输入框中出现中文

实测记录：2026-09-17，RunningHub 登录页手机号框输入"中文测试一二三"，
旧版必炸 / 新版正常。

## 4. 已知限制

- 字节级不可复现：Rust 未启用 reproducible builds，不同机器产出的
  qidi.exe 哈希不同，但功能等价。
- E2E 需要 QIDI 便携包运行环境（`.qidi` 配置目录 + qidi.exe），该环境
  不在本仓库内。
- `cargo test` 首次冷启动需下载全部依赖（本机经 rsproxy.cn 镜像约
  数分钟；Cargo.lock 保证版本一致）。
