---
name: seal-extractor
description: >
  从 PDF 扫描件或图片中自动检测并抠取红色公章/印章，清除章面叠印的黑色文字，输出透明背景 PNG 并生成白底预览图。当用户要求"抠章 / 抠公章 / 提取公章 / 提取印章 / 把章抠出来 / 印章抠图 / 透明底印章 / 公章 PNG / 红章提取"，或需要处理投标文件、合同、证书、授权书扫描件中的红章时使用。也响应 /seal-extractor。
metadata:
  short-description: "抠取红色公章为透明底PNG"
---

# seal-extractor — 红色公章抠取

## 产物呈现（强制）

本技能任何产出交付文件（docx/xlsx/pptx/pdf/png 等）的任务：

1. 文件落盘成功后立即登记：
   `python ~/.qidi/skills/office-artifact/scripts/card.py add <产物路径> --skill seal-extractor --note <一句话说明>`
2. 任务最终回复前必须输出卡片总览：
   `python ~/.qidi/skills/office-artifact/scripts/card.py show`
3. 不许只回一行文件路径就结束；卡片失败则降级为 Markdown 表格列出全部产物。


把 PDF / 图片里的红色公章自动抠成透明背景 PNG：自动定位红章、去除章面叠印的黑字、区分并跳过红色手印、逐页命名导出，并生成白底合成预览图。

## 运行方式

脚本绝对路径，任何工作目录可直接调用：

```powershell
python "C:\Users\ASUS\.qidi\skills\seal-extractor\scripts\extract_seals.py" <输入文件> [更多文件...] -o <输出目录>
```

- 输入：PDF 或图片（png/jpg/jpeg/tif/bmp/webp），可一次传多个；PDF 每页独立处理
- `-o` 省略时默认输出到 `<第一个输入所在目录>\公章PNG\`
- 运行日志走 stderr，**最终结果以 stdout 的 JSON 汇总为准**（`seals` / `skipped` / `errors` / `preview`），务必解析后向用户汇报

## 参数

| 参数 | 默认 | 说明 |
|---|---|---|
| `--dpi N` | 300 | PDF 渲染分辨率；图片输入忽略 |
| `--no-purify` | 关 | 关闭"黑字净化"，保留章面原始暗红像素 |
| `--keep-fingerprints` | 关 | 不做公章/手印区分，全部候选都导出 |
| `--min-size N` | 120 | 候选最小宽/高（像素，按 300dpi 计） |

## 标准工作流

1. 确认输入：用户给目录时先列出其中 PDF/图片，确认处理范围（默认全部）。
2. 运行脚本；输出目录默认 `<源目录>\公章PNG`，或按用户指定。
3. 解析 stdout JSON：提取数量、每枚章的 `来源_p页码_章N.png`、尺寸、`skipped`（手印等）。
4. 自检：数量是否符合预期；圆章约 500px、方章约 250px（300dpi）；有 `errors` 先处理再汇报。若 `read_file` 读不到新生成的 PNG，用终端统计（`Get-ChildItem`、尺寸、透明像素占比）代替目视检查。
5. 向用户汇报：逐枚列出章的来源文件/页码/尺寸、跳过项及原因、预览图 `_预览_白底合成.png` 路径；同一枚章多页重复出现时提醒用户挑最清晰的一枚。
6. 常见反馈处理：
   - "章上有黑字" → 净化默认开启，确认没误加 `--no-purify`；仍残留可提醒用户原件墨迹太淡。
   - "红色太艳/假" → 加 `--no-purify` 重跑对比。
   - "手印也要" → 加 `--keep-fingerprints` 重跑。

## 原理（调参参考）

- 红色掩膜：`d = R - max(G,B)`，`d > 35 且 R > 90`；25×25 椭圆核膨胀 2 次后取连通域
- 并排双章：过宽组件（>620px）在原始掩膜 ≥40px 的零列段处切开
- 公章判定：宽 ≥270px 直接通过；200~270px 需填充率 >0.30 且四角（各取 22% 区域）≥3 角有像素——圆环/方框章四角有墨，实心手印四角为空
- 透明度：`alpha = clip((d-20)/50)`；<30px 孤立块剔除；半透明边缘像素收敛到章主色（`d>120` 像素的 RGB 中位数），消除白边
- 黑字净化：`contam = clip((主色R - R)/(主色R×0.55))`，像素颜色按 contam 拉向主色，暗红"红盖黑"像素还原为纯章红
- 导出：裁掉全透明边并留 8px 边距，RGBA PNG

## 依赖

Python 3.8+，依赖 `pymupdf pillow numpy opencv-python`；缺失时脚本会报错并提示安装命令。

## 注意

- 只识别**红色**印章；蓝章/黑章需改脚本 `red_mask` 的通道差逻辑（如 `B - max(R,G)`）
- 红色手印/指印默认跳过并列入 `skipped`，用户明确要求时加 `--keep-fingerprints`
- 抠出的 PNG 仅供用户自己的文档排版使用；投标文件正式用章应以实体盖章或合规 CA 电子签章为准