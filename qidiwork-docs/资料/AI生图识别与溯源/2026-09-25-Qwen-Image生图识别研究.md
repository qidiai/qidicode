# Qwen-Image 系列生图识别研究报告（2026-09-25）

> 任务：他人如何识别一张图是否由 Qwen-Image 系列模型生成。含型号核实 + 五层分析。
> 标注约定：【确认】= 有来源佐证；【推测】= 合理推断但本次未获直接证据。

## 0. 型号核实

| 型号 | 状态 | 来源 |
|---|---|---|
| Qwen-Image-2.1 | 【确认】2026-09-20 开源，兼顾生图/编辑、轻量高效 | ai-bot.cn/qwen-image-2-1/；163.com（网易）多篇 9月20日报道 |
| Qwen-Image-3.0 | 【确认】2026-07-21 发布（在线闭源），支持 4.5K token 输入 | g.pconline.com.cn/x/2178/21789403.html；news.mydrivers.com/1/1138/1138484.htm |
| **Qwen-Image-3.1** | **未检索到任何该型号**，判断为用户口误（最可能指 3.0 在线版或 2.1 开源版），两种情况本报告均覆盖 | — |

## 1. 官方服务层（通义千问 App / qwen.ai / wuli.art）

| 论断 | 判定 | 来源 |
|---|---|---|
| 通义千问/千问 App 免费版生图默认带**可见水印**（边角"AI生成"类标识，且用户端不可关闭），社区因此出现大量"千问去水印"教程 | 【确认】 | ai.so.com 科普页（"右下角水印，不可关闭"）；163.com/dy/article/KQ88I5PC0556KBCW.html |
| 官方在线服务按法规必须同时做**隐式标识**（文件元数据写入 AIGC 标签等），即官方图大概率"可见水印 + 元数据标识"双保险 | 【推测】（法规强制推导，见 §3） | m.gmw.cn/toutiao/2025-03/14/content_1303991453.htm |
| wuli.art（WuliArt Qwen-Image Turbo 镜像）未见自带可见水印的证据；检索到的教程全是"用户自己给生成图加水印/写 EXIF" | 【推测】：默认不加水印 | blog.csdn.net/weixin_35950531/article/details/158328194 |

## 2. 管线代码层（本地部署到底带不带水印）

| 论断 | 判定 | 来源 |
|---|---|---|
| 开源 Qwen-Image 管线（diffusers `QwenImagePipeline` / ComfyUI / vLLM / SGLang）**出货图自带零水印**——社区大量教程在教"如何自己给 Qwen-Image 生成图加版权水印/EXIF 后处理"，若管线自带水印这些教程无意义 | 【确认】（间接证据充分；本机网络打不开 GitHub raw 源码页，无法逐行复核，已在此声明） | blog.csdn.net/weixin_28793831/article/details/155583215；CSDN Qwen-Image-2512 实战教程 |
| 模型许可允许商用、无强制图片水印要求 | 【推测】（单一弱来源） | blog.csdn.net/weixin_42168902/article/details/155974933 |
| 历史参照：早期 Stable Diffusion 的 invisible-watermark 是发行方仓库加的，非模型本体能力；Qwen 无对应环节 | 【推测】 | cnblogs.com/apachecn/p/18492348（diffusers 源码解析系列） |

**结论：本地 ComfyUI+GGUF 跑出的 Qwen-Image-2.1 图，像素中与文件里都没有任何"官方水印"可被翻出来。**

## 3. 文件元数据层（最大的低级暴露面）

| 途径 | 说明 | 来源 |
|---|---|---|
| ComfyUI PNG 内嵌元数据 | SaveImage **默认**把完整 workflow JSON、提示词、节点参数、`ckpt_name`（模型文件名！）写进 PNG tEXt chunk；拖回 ComfyUI 即还原工作流；comfyui-metadata.com 等在线工具一键读取。模型文件名含 "Qwen"/"GGUF" 即直接暴露 | 【确认】blog.csdn.net/JuMengXiaoKeTang/article/details/140559303；comfyui-metadata.com；blog.csdn.net/weixin_29310631/article/details/159177252 |
| C2PA 内容凭证 | OpenAI 走"隐形水印+C2PA"双轨；Google Gemini 支持 C2PA 检测并开源 Credentio 验证库。本地 ComfyUI 图无 C2PA，但**缺凭证 ≠ 洗白**，只是无正向证明 | 确认：finance.sina.cn/tech/2026-05-20/detail-inhyptam6952238.d.html；donews.com/news/detail/4/6263682.html；blog.csdn.net/yuyanhome/article/details/163771914 |
| 《标识办法》+ GB 45438-2025（2025-09-01 施行） | 强制**显式标识**（图内文字/角标提示）+ **隐式标识**（文件元数据必含生成合成标签、服务提供者、内容编号等，鼓励数字水印）。约束对象是"服务提供者"，本地自用不触发，但投标场景若采购方按国标验收元数据，缺失/异常均可被查 | 确认：m.gmw.cn/toutiao/2025-03/14/content_1303991453.htm；sgpjbg.com/baogao/1281893.html |
| EXIF/Software 字段 | PhotoShop、截图工具、手机相册会重写或留下自家指纹；本地直出的"干净到反常"的 PNG（无 EXIF、无相机信息）本身就是弱信号 | 【推测】 |

## 4. 检测模型层（图像内在特征，洗不掉）

| 类别 | 代表 | 原理 | 来源 |
|---|---|---|---|
| 商业检测器 | Hive Moderation、Illuminarty、AI or Not、HF SDXL Detector | 分类器吃"生成痕迹"统计特征 | jianshu.com/p/626136837930；blog.csdn.net/m0_56647251/article/details/137968831 |
| 国内 API | 腾讯云"图片 AI 生成识别"（数据万象）；阿里云内容安全/AI 安全护栏 | 云端多模态检测，可直接返回 AI 概率 | cloud.tencent.com/document/product/1125/116997；aliyun.com/product/lvwang |
| 学术方法 | UnivFD（CVPR'23，CLIP 特征泛化检测）、DIRE/LaRE2/DRCT（扩散重建误差）、双域伪影（空域+频域） | 扩散图经 VAE/扩散模型"再重建"误差系统性更小；上采样算子留下频域伪影 | arxiv.org/abs/2302.10174；blog.csdn.net/qq_36332660（DIRE/DRCT 年表）；xjishu.com 专利 202610054931 |
| 模型归因 | GAN 指纹归因（Yu et al.）、DATA（深度伪造归因）、模型指纹主动嵌入 | "哪台相机/哪个模型拍的"细粒度溯源，仍有准确率局限 | arxiv.org/html/2505.04384v1；blog.csdn.net/qq_44681809/article/details/131328616 |

**关于"VAE 上采样网格的 FFT 频谱峰"**：属频域伪影类检测的具体描述，本次未检索到独立来源 URL，标记【推测】；但"上采样/解码器引入可检测频域伪影"这一原理有上方多源佐证【确认】。

## 5. 实操结论（投标场景）

| 暴露途径 | 可规避性 |
|---|---|
| ComfyUI workflow/提示词/模型文件名元数据 | ✅ 低成本根除：导出前 Strip metadata / 转 JPEG / 截屏再导出 |
| 可见水印（仅官方渠道图有） | ✅ 本地部署本无 |
| C2PA/AIGC 元数据标签 | ⚠️ 本地图本来没有；但若投标方按 GB 45438 要求"AI 内容须主动标识"，**缺失标识本身是合规风险而非技术优势** |
| 图像内在统计特征（频域伪影/重建误差） | ❌ 无法根除，Hive/腾讯云 API/学术检测器均可判"AI 生成"，重压缩、加噪、resize 只能降信噪比不能归零（且 DRCT 等已对对抗攻击做鲁棒性训练） |
| 归因到"Qwen"具体模型 | ⚠️ 现阶段【推测】准确率低、无成熟商用产品；多是"检出 AI"而非"检出 Qwen" |

**建议**：投标文件用图若含 AI 生成内容，元数据必须清理（防最低级的 workflow 泄露）；但"是 AI 生成"这一事实在检测模型面前藏不住，正确姿势是**依规主动标识或改用实拍/自制素材**，而非赌检测器失效。

*生成：2026-09-25 · 检索工具：web_search（GitHub 源码页本机网络不通，管线层结论以社区间接证据为准）*
