---
name: qwen-image-lowvram
agent_created: true
summary: 6GB 级低显存本机跑 Qwen-Image-2.1 生图（ComfyUI+GGUF Q4，1660 Ti 实测配方）+ 标书/工程效果图批量出图流水线
description: >
  当用户要在低显存（≤8GB，实测 GTX 1660 Ti 6GB / 16GB RAM）本机部署或使用 Qwen-Image 系列图像生成模型，
  或要求 AI 生成/重制工程效果图、标书配图、三维示意图写实渲染、批量出候选图时使用。
  基于 ComfyUI 0.37 portable + GGUF Q4 量化（turbo 4步快稿 / 原版 / UC 无审查三 DiT 并存），
  含全部踩坑修复（city96 Qwen3-VL 视觉塔补丁、IMG_ARCH_LIST 白名单、--force-fp16 图灵病态、
  PS BOM、hf-mirror 302 假长度、gh-proxy 下载加速、PNG 元数据泄密清理）与完整出图 SOP
  （提示词包解析 → turbo 无字快稿 → agnes 视觉泄密扫描 → PIL 保真叠字 → 元数据清零 → SMB 交付）。
  触发词：qwen-image、qwen-image-2.1、低显存生图、6GB 跑图、ComfyUI GGUF、AI 效果图、
  标书效果图、效果图重制、三维示意图、本地部署生图模型。
metadata:
  short-description: "低显存 Qwen-Image 生图与效果图流水线"
---

# 低显存 Qwen-Image-2.1 本地生图（1660 Ti 实测配方）

## 产出登记（强制）

产出任何交付文件（png 等）时：

1. 文件落地成功后登记：
   `python ~/.qidi/skills/office-artifact/scripts/card.py add <产物路径> --skill qwen-image-lowvram --note <一句话说明>`

---

## 一、本机现状（2026-09-25 部署完成，直接可用，勿重复安装）

| 项 | 内容 |
|---|---|
| 安装根 | `F:\AI\ComfyUI\ComfyUI_windows_portable`（ComfyUI 0.37.0 portable cu126，pytorch 2.13.0+cu126，python_embeded 已装 gguf 包） |
| DiT ×4 并存 | `models\diffusion_models\`：`qwen_image_2.1_int8_convrot.safetensors`（7.26G，**✅质量档新默认**：官方原生权重+int8，比 GGUF 快 2.28×，2026-09-26 三明治实测采纳）/ `qwen_image_2.1_turbo_Q4_K_M.gguf`（4.19G，4步快稿档）/ `qwen-image-2.1-stock-Q4_K_M.gguf`（4.20G，备胎，从未实测）/ `qwen-image-2.1-UC-Q4_K_M.gguf`（4.60G，无审查档——**标书等正式交付不用**） |
| 文本编码器 | `models\text_encoders\qwen3vl_8b_heretic-Q4_K_M.gguf`（5.03G）+ **`mmproj-qwen3vl_8b_heretic-f16.gguf`（1.16G，必须同目录、文件名含 GGUF 名，不可改名）** |
| VAE | `models\vae\qwen_image_2.1_vae_bf16.safetensors`（0.68G） |
| 补丁节点 | `custom_nodes\ComfyUI-GGUF-main`（city96）+ `custom_nodes\ComfyUI-GGUF-Qwen3VL-TE-main`（**关键补丁**：修 12288 维 TE 报错） |
| 启动 | `cd F:\AI\ComfyUI\ComfyUI_windows_portable; .\python_embeded\python.exe -s ComfyUI\main.py --lowvram --port 8188` |
| ⚠️ 禁止 | **不要加 `--force-fp16`**（图灵上病态：卡 Model Initializing，GPU 100% 空转）；保持默认 fp32 计算 |

## 二、性能基线（1660 Ti 6GB 实测，fp32，LOW_VRAM 流式）

| 场景 | 耗时 |
|---|---|
| turbo 4步 @1024² | 127s 热跑 / 6min 冷启动 |
| turbo 4步 @1328² | ~9.5 分钟/张（含 TE 编码） |
| UC 25步 @1024² cfg1（**实测数据；原版 stock GGUF 下载后从未实测**——2026-09-25 GLM-5.3 审计发现原表误标） | 冷 15:17（26.35s/步） |
| **int8_convrot 25步 @1024² cfg1（2026-09-26 三明治实测 ✅ 新默认）** | **11.43s/步，25 步采样 6:33；全管线含冷加载 9.9 分钟（UC 同口径 16.3）；漂移校验 A 26.09 vs C 25.95（0.5%）；视觉金丝雀 8/10 与 fp32 肉眼等效** |
| **int8 @1328² 估算** | 步时 ~19.2s（∝面积），25 步采样 ~8 分钟——**原版质量档在标书分辨率上从"太慢"变"实用"** |
| 原版/UC 25步 @1328² **cfg4.5** | **未实测外推值** ~40-45 分钟/张；"CFG>1 每步双跑×2"逻辑已被代码证实（comfy\samplers.py:610 cfg1 跳负向），但该行数字无运行记录背书 |
| 分辨率换算 | 步耗时 ∝ token 数 ∝ 面积：2K(2048²)≈1024² 的 4 倍 |
| 显存峰值 | ~4.7GB/6GB；TE 7.9GB 常驻 RAM 流式上卡（16GB RAM 机器需关重载进程） |

**算子级提速实验（2026-09-25 全部实测，负结果，勿再试）**：

| 方案 | 结果 |
|---|---|
| `--force-fp16`（全局 fp16） | ❌ 病态：卡 Model Initializing、GPU 100% 空转，永久禁用。**机制（GLM 审计微基准坐实）：TU116 无 fp16 加速单元，fp16 GEMM 实测比 fp32 慢 6×（219ms vs 36.1ms @4096³）——fp16 方向永久关闭** |
| `UnetLoaderGGUFAdvanced(dequant_dtype=fp16)` | ❌ 零提速（26.2 vs 26.0s/步）——dequant.py 里 fp16 只是反量化中间量，随后 `.to(dtype)` 转回 fp32 计算（参数真生效：输出字节与基线不同） |
| `--fp16-unet`（DiT 级 fp16） | ❌ **输出哈希与 fp32 逐字节相同=彻底 no-op**。正向证据（GLM 审计补齐）：该次会话日志确有 `model weight dtype torch.float16, manual cast: torch.float32`（flag 已应用）；根因 nodes.py:175 —— **GGUF 加载链不把 unet_dtype 传进 load_diffusion_model_state_dict** |
| `QwenImage21Cache`（前缀 KV 缓存） | ⚠️ T2I 零提速（26.0-26.2s/步）；GLM 审计修正：缓存机制**默认就是开的**（model_base.py reset_prefix_cache），节点只调 device/dtype，文本前缀实为几十 token 量级（非数百）；**仅编辑/多参考图任务有意义**，该场景应接入 |
| int8_convrot 权重 | ✅ **采纳（2026-09-26 三明治裁决：2.28×）**。GLM-5.3 审计曾修正预期为 15-30%，实测 **11.43 vs 26.09/25.95 s/步 = 2.28×**——审计漏算的关键项：GGUF Q4_K_M 在 eager 里每步要逐算子做 Q4→fp32 反量化（开销≈矩阵乘本身），而 int8 走 Hadamard 旋转+动态 W8A8+`_int_mm`（DP4A 4.9×）整链更便宜。RAM 实测：加载期 commit +9GB、机器满载（物理剩 0.3-3.3GB）下照常跑完、无页换灾难，但 16GB 机上仍属薄余量运行 |
| **运维教训（2026-09-26 凌晨四连发车事故复盘）** | ① **PID 验身再归因**：追了 5 小时的"另一会话 11.9GB python"竟是自己的 ComfyUI 服务器（abort 后模型缓存未释放）——查 `(Get-CimInstance Win32_Process -Filter "ProcessId=$id").CommandLine` 先验身份；② **availPhys 不含待机缓存**，作止损判据必误杀（模型加载时系统自动逐出 standby，0.3GB 也能跑）；③ **TE+DiT 正常加载成本 = 10-13GB commit**，"增量>10GB 即中止"判据必误杀；④ 正确兜底 = commit 逼近上限（>48GB）+ 单片超时（25min） |
| **环境事实（GLM 审计发现，改认知）** | portable 为 cu126 → ComfyUI 核心把 comfy_kitchen **cuda 后端整体禁用**（需 CUDA 13+，quant_ops.py:26-27），全部算子实际走 eager；该 pyd 亦无 int8_linear 注册（无 cuBLASLt）、SM75 cutlass int8 内核仅服务 W4A8 回退且**按设备名拉黑 GTX 16 系**（is_turing AND NOT is_16series） |
| xformers / flash-attention | 预判无增益：flash 不支持 sm_75；ComfyUI "pytorch attention" 已走 SDPA mem-efficient 分支 |

## 三、全新机器复装配方（F:\AI 损毁时按此重建）

1. **ComfyUI portable cu126**：GitHub release 直连被墙（release-assets.githubusercontent.com 挂起），用 `https://gh-proxy.com/https://github.com/Comfy-Org/ComfyUI/releases/download/v0.37.0/ComfyUI_windows_portable_nvidia_cu126.7z` 单流下载（实测 12MB/s；该代理忽略 Range，并行分段无效）
2. 模型四件套走 hf-mirror（见上表文件名；302 后的 xethub CDN 国内可直连 ~23MB/s）
3. `custom_nodes\` 装 city96 ComfyUI-GGUF + ComfyUI-GGUF-Qwen3VL-TE（gh-proxy 拿 zip）
4. `python_embeded\python.exe -m pip install gguf -i https://pypi.tuna.tsinghua.edu.cn/simple`
5. **打 IMG_ARCH_LIST 补丁**：`ComfyUI-GGUF-main\loader.py` 第 12 行 `IMG_ARCH_LIST` 集合追加 `"qwen_image21"`（UC/unsloth 等带 `general.architecture` 元数据的 GGUF 没它就 `Unexpected architecture type` 拒载；**city96 节点更新会覆盖此补丁，更新后必须重打**）
6. 重启 ComfyUI，日志出现 `[GGUF-Qwen3VL-TE] ... patched` 即就绪

## 四、踩坑速查（8 条，按踩中概率排序）

1. **PNG 元数据泄密**：SaveImage 默认把 workflow JSON（含提示词+模型文件名）写进 PNG tEXt，拖回 ComfyUI 即还原——**交付前必须 PIL 重存清零**（annotate.py 的 save 已内置）
2. **hf-mirror HEAD 假 Content-Length**（302 跳转页 ~1KB）：分段下载须先解析最终 CDN URL 再 HEAD（scripts\dlp.ps1）
3. **city96 不认 Qwen3-VL TE**：DiT 前向报 `Given normalized_shape=[4096] ... got [1,512,12288]` → 装 Qwen3VL-TE 补丁节点 + mmproj 同目录（勿改名，按文件名匹配）
4. **带元数据 GGUF 拒载**：`Unexpected architecture type: 'qwen_image21'` → 补丁见 §三.5
5. **--force-fp16 图灵病态** → 默认 fp32；另实测 `--fp16-unet` 对 GGUF 模型为 no-op（输出逐字节相同）、`dequant_dtype=fp16` 零提速——**GGUF 路径上 fp32 即地板，勿再试 fp16**
6. **PS 5.1 `Set-Content -Encoding UTF8` 写 BOM** → aiohttp 拒收 POST JSON；用 `[System.IO.File]::WriteAllText($p,$raw,[Text.UTF8Encoding]::new($false))`
7. **分辨率必须为 16 的倍数**（VAE 16× 压缩）：1920×1080 无效 → 用 1920×1088；1328/1664 可
8. **中文 md/提示词经 PS 控制台会 GBK 乱码**：读文件用 read_file 工具或 Python；别靠控制台回显判断内容

## 五、出图 API 工作流模板（无 UI 直跑）

节点链：`UnetLoaderGGUF(模型.gguf)` + `CLIPLoaderGGUF(qwen3vl_8b_heretic-Q4_K_M.gguf, type="qwen_image")` → `TextEncodeQwenImage21(prompt, negative_prompt, resolution=1024)`（正负条件一次出）→ `EmptyLatentImage(w,h,1)`（通道数自动对齐）→ `KSampler(seed, steps, cfg, euler/simple, denoise=1)` → `VAEDecode` → `SaveImage`。

- turbo 版参数：steps=4，cfg=1.0（蒸馏版无需 CFG，负面词无效）
- **质量档（推荐）**：`UNETLoader(qwen_image_2.1_int8_convrot.safetensors, weight_dtype=default)` + 25 步 + cfg 1.0（PIL 叠字无字场景）——1024² 全管线 ~10 分钟、1328² 采样 ~8 分钟；比 GGUF 快 2.28× 且质量等效
- 原版正式档（图内文字场景）：int8 + cfg 4~5 + 负面提示词（每步双跑）；或 pottokao 分段法：前 12-17 步 cfg 1.0 锁构图、后段 cfg 3.0 重画字
- 提交：`curl -X POST -H "Content-Type: application/json" -d @api.json http://127.0.0.1:8188/prompt`；轮询 `GET /history/{prompt_id}` 直到 status_str 出现
- 结构化批量脚本见 scripts\phase_a_drafts.py（turbo 批量）/ phase_b_refine.py（原版精修，注：该脚本默认 cfg 4.5，无字场景改 1.0 省一半）

## 六、标书/工程效果图批量流水线（SOP）

1. **解析提示词包**：读任务 md，按 `**主提示词：**` 分段提取；草稿阶段把"文字要求：…"整段替换为"画面中不出现任何文字…"（§七无字版策略——AI 渲染长技术标注必乱码，文字全部后补）
2. **turbo 4 步无字快稿**（每张 ~10 分钟内）：看构图、试提示词方向
3. **agnes 视觉泄密扫描**（视觉子代理逐图查）：零文字字符 / 零公司名 logo 水印签名 / 无清晰面部 / 门头围挡帽背心板房门牌无字 / 构图与提示词要点核对（暗标泄密=该评分点零分）
4. 用户确认构图后可选：**原版 25 步精修**（cfg 4.5 + 文件负面提示词，40-45 分钟/张）
5. **PIL 保真叠字**（scripts\annotate.py + coords.json）：先让视觉代理给每个标注目标的百分比坐标 → 黑体图题（白描边）+ 宋体白底芯片 + 细引线；标题字号自适应横幅（`min(w*0.041, h*0.052)`）
6. **元数据清零**（annotate.py 保存即清）+ agnes 复查（图题完整/芯片无叠压/引线指向正确）
7. 交付 SMB 共享目录 + office-artifact 登记

- 步骤 4 之后（或并行）：做**编辑/多参考图**任务时，在 `UnetLoaderGGUF → KSampler` 之间插入 `QwenImage21Cache`（device=auto/cpu）——参考图前缀 KV 缓存，官方称多图输入场景效率收益显著（T2I 无感，实测零提速）
## 七、合规与溯源红线

- **许可**：Qwen-Image-2.1 及一切衍生（turbo/UC/GGUF）= Qwen RESEARCH LICENSE，**仅研究评估用途，商用交付（含标书）需向 model-business@notice.qwencloud.com 购授权**；上一代 Qwen-Image 1.0 为 Apache-2.0 可商用替代
- **溯源现状**：本地图无任何官方水印，但 AIGC 检测器可判"AI 生成"（频域伪影无法根除）；归因到具体 Qwen 模型尚属研究阶段；详见 `qidiwork-docs\资料\AI生图识别与溯源\2026-09-25-Qwen-Image生图识别研究.md`
- **标识合规**：若图被认定 AI 生成合成内容，按《标识办法》/GB 45438 需显式+隐式标识——缺标识是合规风险，不是优势

## 八、自带脚本（scripts\，含本机硬编码路径，跨机器先改 MD/API/OUT 常量）

| 脚本 | 用途 |
|---|---|
| dlp.ps1 | 多段并行下载（自动解析 302 最终 CDN URL + 分段校验合并） |
| phase_a_drafts.py | turbo 4步无字版批量出稿（解析提示词 md + 提交 + 轮询） |
| phase_b_refine.py | 原版 25 步 cfg4.5 带字精修批量 |
| annotate.py | PIL 图题+标注芯片+引线叠加，保存即清 PNG 元数据 |
| sandwich_runner.py | A/B/A 同会话对照跑批（判据已修为"commit>48GB 灾难+单片25min超时"，含 A 片环境校验短路） |
| dlp2.ps1 | HEAD 失效 CDN 的分段下载（ranged GET + Content-Range 取总长；ModelScope CDN 不吃 HEAD） |
| coords.example.json | 标注坐标文件格式示例（视觉代理产出的百分比坐标填这里） |

## 九、参考文档（repo 内，git 版本化）

- 部署全量实测与决策记录：`qidiwork-docs\会话纪要\2026-09-25-Qwen-Image-2.1-本地部署-1660Ti.md`
- 生图溯源研究报告：`qidiwork-docs\资料\AI生图识别与溯源\2026-09-25-Qwen-Image生图识别研究.md`
