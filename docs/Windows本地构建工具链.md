# Windows 本地构建工具链搭建实录（qidicode-src）

> 日期：2026-09-05/06 ｜ 机器：Windows x64，rustup stable-x86_64-pc-windows-gnu
> 成果：从"无法构建"到 **37 分钟出 qidi.exe**，含完整诊断思路、根因分析与可复现脚本

---

## 0. 背景与起点

**目标**：在源码仓库（`G:\程序开发\qidicode\qidicode-src`）本地构建出可运行的 qidi.exe（本次实战目标：植入 think 工具）。

**起点有多惨**（动手前逐项确认过）：

| 项 | 现状 | 后果 |
|---|---|---|
| rustup 工具链 | `stable-x86_64-pc-windows-gnu`（rust-toolchain.toml 钉 stable → 默认 host 是 gnu） | GNU 路线意味着需要完整 MinGW 外围 |
| rustup 自带 binutils | `.../rustlib/x86_64-pc-windows-gnu/bin/self-contained/` 里有 dlltool.exe/ld.exe/gcc.exe（**链接器专用**） | 官方明说：捆绑 gcc 只能当链接器，编不了 C 代码 |
| as.exe（汇编器） | **全机没有** | dlltool 生成导入库时要拉起 as → 直接崩 |
| gcc.exe（C 编译器） | **全机没有** | cc-rs（jemalloc/dav1d/rusqlite/ring 等 C 依赖的构建器）全灭 |
| MSYS2 / MinGW | 未安装 | 无处补齐 as/gcc |
| VS Build Tools（MSVC link.exe） | 未安装 | 换 MSVC 路线也走不通 |
| GitHub 直连/代理 | 均不通（000） | WinLibs 等 GitHub 发行包路线被封死 |
| 网络环境 | 国内，TUNA/USTC 镜像全通（200） | 唯一活路：MSYS2 国内镜像 |
| 源码路径 | **含中文**（`G:\程序开发\...`） | 埋下第三颗雷（见下） |

**结论：三条路全断（无 as、无 gcc、无 MSVC），唯一可行方向 = 从国内镜像拼装 MinGW 外围。**

---

## 1. 第一关：dlltool "CreateProcess" 失败

### 症状
```
error: dlltool could not create import library with ...\self-contained\dlltool.exe
       -d ...bcryptprimitives.dll_imports.def ... --temp-prefix bcryptprimitives.dll:
       ...\dlltool.exe: CreateProcess ...
error: could not compile `getrandom` (lib)
```

### 诊断思路
1. 触发者是 `getrandom` → windows 系 API 用 `raw-dylib`（Rust 1.71+ 特性），rustc 会调 **dlltool** 把 .def 转成导入库
2. rustup self-contained 里的 dlltool 确实存在且被调用了 → 不是"找不到 dlltool"
3. dlltool 自己报 `CreateProcess` → 是 **dlltool 去拉起子进程失败**
4. dlltool 处理 `--temp-prefix` 时会写临时汇编再调汇编器 `as` → **as.exe 不在 PATH** → 实锤

### 试过的弯路（别再走）
- `RUSTFLAGS="--cfg windows_no_raw_dylib"`：只对 windows-sys 家族有效，**getrandom 0.4 自己声明 raw-dylib，不吃这套**

### 解法
从 TUNA 拉单包 `mingw-w64-x86_64-binutils`（6MB，含 as.exe），解压到 `C:\mingw-tools`，PATH 前置。
self-contained 的 dlltool 会从 PATH 找到 as 正常工作（跨版本兼容没问题：对象格式稳定）。

---

## 2. 第二关：cc-rs 找不到 gcc.exe

### 症状
```
cargo:warning=Compiler family detection failed ... failed to find tool "gcc.exe": program not found
error occurred in cc-rs: failed to find tool "gcc.exe"
```

### 根因
本项目依赖树里有大量 C 代码依赖（tikv-jemalloc、dav1d、rusqlite bundled、ring、zstd…），
全部经 cc-rs 构建 → 必须有真 gcc（self-contained 的 gcc 只管链接）。

### 解法
继续 TUNA 拼装 gcc 全家桶（见第 4 节包清单），**注意坑中坑**：

> **坑中坑**：下载 `mingw-w64-x86_64-gcc` + `crt` 后编译仍报 `stdio.h: No such file or directory`。
> MSYS2 新版布局把 **headers 拆成了独立包** `mingw-w64-x86_64-headers`（历史版本 crt-git 是含头的）。
> 目录枚举找包名时还被排序截断坑过一次（amf-headers/directx-headers 排在前面把真包挤出了前 8 行）。
> **必须装：gcc + crt + headers 三件齐**，再配 gmp/isl/mpc/mpfr（gcc 自身依赖）。

验证姿势：`gcc test.c -o test.exe && ./test.exe`（能跑才算闭环，光 `gcc --version` 不够）。

---

## 3. 第三关：中文路径 × MinGW ld（最阴的一关）

### 症状（三段演化）
**阶段 1**（原始路径直接编）：
```
ld: cannot find G:\程序开发\...\target\debug\deps\serde_derive-....rcgu.o: No such file or directory
```
明明文件存在（`Test-Path` 验证 True），ld 就是"找不到"。

**阶段 2**（上了 junction `G:\qidi-src → G:\程序开发\qidicode\qidicode-src`）：
部分路径变 ASCII 了，serde_derive 能编过 —— 但链接参数里**中文路径与 ASCII 路径混现**：
```
G:\qidi-src\target\...\libfiletime-*.rlib        ← junction 形式（能开）
G:\程序开发\...\target\...\libtar-*.rlib          ← 真身形式（开不了）
```

**阶段 3**（`cargo clean` 后全量重建）：混合依旧！

### 根因分析
1. MinGW ld 在 Windows 用 ANSI(GBK 936) 处理文件参数，链接链路某环节把中文搅碎（gcc→collect2→ld 的参数传递编码不透明）
2. junction 只救一半：**`fs::canonicalize` 会把 junction 解析回中文真身**——cargo 内部记录 canonical 路径的地方照样是中文，所以"混现"且 clean 也救不回

### 解法（绝杀）
```powershell
$env:CARGO_TARGET_DIR = "G:\qidi-target"   # 纯 ASCII 独立目录
```
- 所有 .rlib/.o/链接参数全部落在 ASCII 路径下 → ld 永远见不到中文
- 源码路径（中文）只出现在 debuginfo 字符串里，ld 不解析 → 无害
- 副作用：换个 target 目录等于换指纹，首次全量，之后增量很快

> **经验**：junction 能解决"rustc/ld 找不到中文源码路径"类问题，但**只要 cargo 会 canonicalize 传入 target 路径**，就别指望它。直接 CARGO_TARGET_DIR 走纯 ASCII，一步到位。

---

## 4. 最终方案

### 工具链布局
```
C:\mingw-tools\mingw64\bin\        ← 手工拼装的 MSYS2 组件（TUNA 源）
    gcc.exe (16.2.0)   as.exe (2.47)   dlltool.exe (2.47)   ld.exe ...
C:\Users\<user>\.rustup\toolchains\stable-x86_64-pc-windows-gnu\
    lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained\    ← rustup 自带，挂 PATH 兜底
G:\qidi-src                         ← junction → 源码仓库（可选，见第 3 节教训）
G:\qidi-target                      ← 构建产物目录（强制 ASCII）
```

### 标准构建姿势（每次必念三件套）
```powershell
Set-Location "G:\qidi-src"
$env:PATH = "C:\mingw-tools\mingw64\bin;" +
            "C:\Users\28970\.rustup\toolchains\stable-x86_64-pc-windows-gnu\lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained;" +
            $env:PATH
$env:CARGO_TARGET_DIR = "G:\qidi-target"

cargo check -p cf-tools -p cf-workspace -p cf-agent      # 快速验证（~7 分钟全量，之后增量秒级）
cargo test  -p cf-tools --lib think                      # 定向单测
cargo build --release -p cf-pager-bin                   # 出货 qidi.exe（首跑 ~37 分钟）
```
产物：`G:\qidi-target\release\qidi.exe`（本次 297MB；干净 PATH 下 `--version` 验证过无外部 DLL 依赖）。

### 三件套缺一不可
| 件 | 不设会怎样 |
|---|---|
| `C:\mingw-tools` PATH | dlltool 崩（as 缺）/ cc-rs 崩（gcc 缺） |
| self-contained PATH | getrandom 等直编包找不到 dlltool（"program not found"） |
| `CARGO_TARGET_DIR` ASCII | ld 混中文路径 "cannot find *.rlib"（clean 也救不回） |

---

## 5. 附带脚本（本目录 `build-scripts/`）

| 脚本 | 用途 |
|---|---|
| `get_mingw_binutils.py` | 从 TUNA 拉 binutils + 运行时 DLL 依赖包，解压到 `C:\mingw-tools`，自动试运行 as |
| `get_mingw_gcc.py` | 补 gcc/crt/headers/gmp/isl/mpc/mpfr 全家桶（zstandard 解包，bsdtar 不认 zst） |
| `qidi-build.ps1` | 一键构建：自动设三件套 → check → (可选)test → (可选)release 出货 |

**环境复现**（新机器两步）：
```powershell
py -3 build-scripts\get_mingw_binutils.py   # ① binutils
py -3 build-scripts\get_mingw_gcc.py        # ② gcc 全家
powershell -File build-scripts\qidi-build.ps1 -Release   # ③ 构建出货
```

---

## 6. 实战验证记录（think 工具，2026-09-05）

| 步骤 | 命令 | 结果 |
|---|---|---|
| 编译验证 | `cargo check -p cf-tools -p cf-workspace -p cf-agent` | **Finished**，6m53s 全量 |
| 单元测试 | `cargo test -p cf-tools --lib think` | **1 passed**（2576 过滤） |
| Release 出货 | `cargo build --release -p cf-pager-bin` | **Finished**，37m00s，qidi.exe 297MB |
| 独立性 | 干净 PATH `--version` | `qidi 0.1.220-alpha.4` 正常输出 |
| 编入确认 | 二进制内 grep think 描述/模块路径 | 双 HIT |

---

## 7. 故障速查表

| 报错关键词 | 根因 | 一行解法 |
|---|---|---|
| `dlltool ... CreateProcess` | as.exe 不在 PATH | PATH 前置 `C:\mingw-tools\mingw64\bin` |
| `error calling dlltool 'dlltool.exe': program not found` | rustc 找不到 dlltool | PATH 补上 rustup self-contained 目录 |
| `cc-rs: failed to find tool "gcc.exe"` | 无真 gcc | 跑 get_mingw_gcc.py（gcc+crt+headers 三件齐） |
| `stdio.h: No such file or directory` | 少 headers 独立包 | 补 `mingw-w64-x86_64-headers`（版本对齐 crt） |
| `ld: cannot find *.rlib / *.o`（文件明明在） | 中文路径被 ld 搅碎 | `CARGO_TARGET_DIR` 指纯 ASCII 目录 |
| 上述清缓存也没用 | canonicalize 把 junction 解析回真身 | 同上——junction 只救一半，以 CARGO_TARGET_DIR 为准 |
| `cannot find ... build_script_build-*.exe` | 同中文路径根因 | 同上 |
| 长路径日志打不开（FileNotFound 但枚举能见） | 会话目录名 URL 编码超 260 | 用 `\\?\` 前缀打开 |
| cargo 管道后 exit code 可疑 | PowerShell 管道假信号 | 认准输出里的 `Finished`，别只看 exit code |

---

## 8. 遗留事项

- `target/`（仓库内）残留历史构建缓存，建议删掉避免误用（一切以 `G:\qidi-target` 为准）
- MSVC 路线（stable-x86_64-pc-windows-msvc）需要 VS Build Tools，未装；GNU 路线已完全够用
- crates 走 rsproxy.cn 镜像（`.cargo/config.toml` 已配），新机器拉依赖无障碍
- think 工具的源码改动明细见项目记忆 MEMORY.md（think 模块 + 6 处注册链路）

---

## 9. 关于"中文路径习惯"的澄清（重要）

本方案**没有动你的中文路径习惯**——请区分两类目录：

| 目录 | 路径 | 角色 |
|---|---|---|
| **源码** | `G:\程序开发\qidicode\qidicode-src`（中文） | 你的工作文件，**原地不动**，一直正常使用 |
| **构建产物** | `G:\qidi-target`（ASCII） | 编译缓存/输出，类比 node_modules/target，**不属于日常工作文件** |

中文源码路径对 rustc/cc/gcc 全部无碍（他们吃 Unicode 没问题）；**只有 ld 读 .o/.rlib 长参数时**因 binutils 的 ANSI 代码页缺陷会搅碎中文。所以把"喂给 ld 的东西"（= target 目录）单独放 ASCII 即可，代价为零。

如果坚持连 target 也用中文，两条路（都不推荐现在做）：
1. 系统开"UTF-8 提供全球语言支持"（Beta）——可能连带弄乱老 GBK 软件，风险大
2. 装 VS Build Tools 走 MSVC 工具链（~3GB），link.exe 对 Unicode 原生友好——但要下 3GB 且全量重建

结论：**源码中文 + target ASCII 是最小代价最优解**，日常习惯不受影响。
