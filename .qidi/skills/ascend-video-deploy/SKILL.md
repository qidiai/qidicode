---
name: ascend-video-deploy
description: >
  在华为昇腾 NPU 容器上部署 MiniMax H3 INT8 文生视频服务（GitCode Notebook 免费单卡 910B 环境全流程实战验证）。
  覆盖：Notebook 容器规格选择、53GB 权重分片并行下载+SHA 校验、ComfyUI + NPU patch 环境搭建、
  单卡 32G HBM 适配（文本编码器走 CPU + turbo 8 步）、torchaudio/uv/submodule 等实战坑修复、
  ComfyUI API 封装出片、2 小时会话恢复。配套一键脚本 mm_h3_serve.sh（v3 实战版）。
  当用户要求"昇腾部署视频生成"、"Notebook 部署视频模型"、"910 跑 MiniMax"、"免费视频生成"、
  "Ascend 部署 ComfyUI"、"GitCode 容器跑视频"、"NPU 文生视频"、"/ascend-video-deploy"时使用。
---

# 昇腾视频生成部署（MiniMax H3 INT8 · GitCode Notebook）

> 2026-09-18 单卡 Ascend 910B4 32G HBM 上端到端实战验证通过：736×416@24fps 带音频视频，
> 冷启动端到端 13m25s，turbo 8 步采样 6.4s/it，热启动 ~2 分钟/条。

## 0. 何时用本技能

| 场景 | 判定 |
| --- | --- |
| 用户想在**免费昇腾容器**跑视频生成（GitCode Notebook） | ✅ 主场景 |
| 用户有自有昇腾服务器（双卡 910A3 64G 等） | ✅ 脚本自动适配（/data 优先） |
| 用户只想要 MiniMax API 调用（不想部署） | ❌ 走 MiniMax 官方付费 API |

## 1. 硬事实（动手前必须知道）

1. **GitCode Notebook 免费规格**：单卡 910B4（32G HBM）+ 16vCPU + 64G RAM，镜像 `ubuntu22-cann8.5-python3.11-jupyter`，存储选 **[限时免费] 100G**（50G 放不下 53GB 权重）
2. **会话 2 小时上限**，页面关闭/超时容器即回收；**/opt/atomgit 存储不跨会话持久** → 每次会话重跑一键脚本重建（权重分片并行 ~12 分钟）
3. **单卡 32G HBM 放不下全部权重**（27G 文本编码器 + 21G 扩散 + 5.8G VAE）→ 仓库的单卡方案：**文本编码器 CPU + 扩散 NPU + turbo 8 步**（实测可用）
4. 仓库：`https://gitcode.com/xujiashuai/minimax-h3-int8`（INT8 ConvRot 权重 + NPU patch + ComfyUI submodule）

## 2. 浏览器操作铁律（chrome-devtools MCP）

容器内 Linux 终端是 canvas 渲染读不到输出，**唯一可靠通道是 JupyterLab Notebook 单元格**：

| 操作 | 正确做法 | 事故案例 |
| --- | --- | --- |
| 往单元格输入命令 | `fill` 工具（uid=单元格 textbox）→ DOM 直写 | ❌ `type_text` 键盘模拟：Jupyter 命令模式把字符当快捷键（m=转Markdown、o/a=插单元格、dd=删除、00=重启kernel），已引发 3 次灾难 |
| 执行单元格 | `press_key` Control+Enter | — |
| 读输出 | `take_snapshot`（DOM 文本直接可读） | ❌ 终端截图：canvas 读不出；❌ screenshot filePath：MCP 路径白名单拦截 |
| 长文本进容器 | 单行 base64（zlib 压缩后 ~3.3KB）fill 进单元格执行 | 4KB+ 键盘转录必错 |
| 每次新命令 | **新开一个空单元格**或先 Control+A 全选 | 忘全选会拼进旧代码产生语法错误 |
| 长任务 | Notebook 单元 `os.popen("... setsid nohup ... &")` 后台化 | 前台跑会占死 kernel，后续单元全部排队 |

## 3. 部署流程（一键脚本）

### 3.1 开容器（浏览器，用户需已登录 GitCode）

1. `https://ai.gitcode.com/xujiashuai/minimax-h3-int8` → 「Notebook 快速开发」
2. 资源确认对话框：计算类型 **NPU**、规格 **1×NPU 910B · 16vCPU · 64GB**、镜像 **ubuntu22-cann8.5-python3.11-jupyter**、存储 **[限时免费] 100G** → 立即启动
3. 等 ~30s 进 JupyterLab → Launcher → 「Python 3 (ipykernel)」新建 Notebook

### 3.2 传输 + 运行一键脚本

脚本在本地 `minimax-h3-npu-deploy/mm_h3_serve.sh`（v3，14.6KB）。两种入容器方式：

- **方式 A（推荐）**：zlib+base64 转录包（本地跑 `python -c "import zlib,base64;print(len(base64.b64encode(zlib.compress(open(r'minimax-h3-npu-deploy/mm_h3_serve.sh','rb').read(),9))))"` 生成单行，fill 进单元格执行落盘 `/opt/atomgit/data/mm_h3_serve.sh`）
- **方式 B**：会话内分段 heredoc（每段 ≤2KB fill 追加）

然后后台启动（避免占死 kernel）：

```python
import os
print(os.popen("cd /opt/atomgit/data && setsid nohup bash mm_h3_serve.sh > serve.log 2>&1 < /dev/null & echo STARTED").read())
# 之后轮询: print(os.popen("tail -c 800 /opt/atomgit/data/serve.log").read())
```

脚本 9 步全幂等：环境自检 → clone+sed 路径 → submodule(HTTPS覆写) → 权重分片并行下载(段级续传+SHA) → setup_comfyui(uv+venv+patch+软链) → torchaudio 修复 → 按卡数生成工作流 → mmgen.py → 启动+健康检查。

### 3.3 出片验证

```python
import os, subprocess, time
pr = subprocess.Popen(['/opt/atomgit/data/mm/third_party/ComfyUI/.venv/bin/python',
    '/opt/atomgit/data/mm/mmgen.py', '提示词. Audio: 音效', '--outdir', '/opt/atomgit/data/mm/out'],
    stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
for line in pr.stdout: print(time.strftime('%H:%M:%S'), line.strip()[:160], flush=True)
```

预期：首条 13 分钟（47G 上卡），8 步采样 6.4s/it，产出 736×416@24fps 带音轨 mp4（~639KB/5秒视频）。

## 4. 实战坑清单（v3 脚本已全部内置修复）

| # | 坑 | 现象 | 修复 |
| --- | --- | --- | --- |
| P1 | 仓库脚本硬编码 `/data`，非 root 建不了 | mktemp: No such file | sed 全量改写 `$WORK_ROOT` |
| P2 | submodule 是 SSH url | 无 key 用户拉不动 | `git config submodule.xxx.url https://...` |
| P3 | 分片脚本 mktemp 需父目录 | 下载秒挂 | 预建 4 个权重子目录 |
| P4 | 单连接 wget 7.5MB/s | 53GB 要 121 分钟 > 会话 2h | download_chunked 16 连接 ~85MB/s |
| P5 | curl 段挂起（无传输超时） | 尾部 4G 停滞 10 分钟+ | 段级大小检查→只补缺段→合并→SHA 校验 |
| P6 | setup 依赖 uv 且不在 PATH | line 41: uv not found | pip install uv + export PATH=$HOME/.local/bin |
| P7 | Notebook 镜像缺 torchaudio | ComfyUI 启动即崩 | venv 内 `--no-deps` 装 2.9.0（防 torch 被换 CUDA 版） |
| P8 | 32G HBM 装不下全部权重 | 加载期进程退出 | 单卡工作流：文本编码 CPU + add_turbo_lora.py 注入 |
| P9 | FP8 崩溃 | UNETLoader fp8_e4m3fn | 强制 weight_dtype=default（A910/910B 不支持 FP8） |
| P10 | 老版 wget 与分片合并冲突写坏文件 | SHA 不匹配 | 启动分片前 pkill 旧 wget + 删半成品 |

## 5. 关键参数与恢复

- **权重清单**（5 个共 ~53GB，SHA-256 以仓库 `MANIFEST.json` 为准）：扩散 INT8 21G、文本编码器 INT8 27G、视频 VAE FP16 5.2G、音频 VAE FP32 0.6G、turbo LoRA 1.9G
- **会话恢复**：重开 Notebook → 3.1 → 3.2 重跑同一脚本（幂等，~25 分钟回到出片状态）
- **性能基线（实测）**：冷启动 13m25s；热启动 ~2 分钟/条；采样 6.4s/it × 8 步；CPU 文本编码（单卡模式）约占 1-2 分钟/条
- **下载产物路径**：`/opt/atomgit/data/minimax_h3_int8/{diffusion_models,text_encoders,vae,loras}`
- **ComfyUI 日志**：`/opt/atomgit/data/mm/third_party/ComfyUI/logs/comfyui.log`

## 6. 多容器并联（用户若问）

多账号=多免费容器，但 **NPU 硬件直通单容器，"并联成双卡"不可行**；可行方案：
- **A 并行不并联**（推荐）：两账号各跑完整单卡方案，吞吐×2
- **B 节点拆分**：容器 B 跑文本编码 HTTP 服务，容器 A 自定义节点远程取条件向量（开发量大，收益仅省 1-2 分钟/条编码时间）
- 网络直连可实测：两容器 `hostname -i` 拿 Pod IP 互 ping（同 K8s 集群可能互通）

## 7. 配套文件

| 文件 | 位置 | 说明 |
| --- | --- | --- |
| `mm_h3_serve.sh` | 本 skill 目录 + `minimax-h3-npu-deploy/` | v3 一键部署/恢复脚本（内嵌 mmgen.py） |
| `部署手册.md` | `minimax-h3-npu-deploy/` | 完整操作手册 |
| `mmgen.py` | `minimax-h3-npu-deploy/` | API 封装独立版 |
| `write_mmgen_cell.txt` | `minimax-h3-npu-deploy/deploy/` | mmgen.py 转录安全单行包（zlib+base64） |
