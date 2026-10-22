# 图纸渲染通道选型探针报告（A: ezdxf+matplotlib / B: cadcraft L2 / C: 现状 Pillow 占位）

日期：2026-10-22 · 本机：Windows 11 x64，CPython 3.12.3（原生进程）
纪律：全程只读 `G:\程序开发\标书软件` 与 `G:\程序开发\artcraft`，所有产物只落
`%TEMP%\wb_drawing_probe\`；未读写任何真实标书语料；两个仓库的 `git status` 与目录内容前后无变化
（已核对：无新增 `__pycache__`，`git status --porcelain` 输出与开工前一致）。

---

## 0. 一句话结论

**接 A（ezdxf 官方 matplotlib 后端）做主链路，并加一步"SHX 样式→TTF 字体改写"预处理；B（cadcraft-cli 0.5.0）
作为原生读 DWG 的兜底候选保留观察；C（现状 dwg_to_png.py）必须立即停用——它在当前 ezdxf 1.4.4 下两条调用
分支都抛异常，一张图都出不来，不是"质量差"而是"已死"。**

---

## 1. 被测对象与真实身份核实

| 方案 | 实际调用 | 核实结果 |
|---|---|---|
| A | `ezdxf.addons.drawing` Frontend + `matplotlib.MatplotlibBackend`（ezdxf 1.4.4 / matplotlib 3.11.1） | 可用 |
| B1 | `cadcraft-cli.exe convert IN.dxf OUT.png`（默认环境） | 可用 |
| B2 | 同上 + `CADCRAFT_FONTALT=simhei.ttf`、`CADCRAFT_FONTFALLBACK=simhei.ttf`（即"SHX→TTF 字体替换"） | 可用 |
| C | `G:\程序开发\标书软件\skill_assets\bid-drawing\scripts\dwg_to_png.py` | **生产口径 100% 崩溃**（见 §6） |

关于任务书里的 **cad2pdf**：`G:\程序开发` 下 maxdepth 3 的文件名搜索、artcraft 全仓内容搜索、
标书软件仓内容搜索**均未找到任何 cad2pdf 可执行文件/脚本/引用**。本机存在的只有
`G:\程序开发\artcraft\bin\cadcraft-0.5.0-windows-x64-portable\`（cli 17.70 MB + gui 35.33 MB = 53.07 MB）。
因此本报告用 **cadcraft-cli 0.5.0 的 L2 路线**（自带单笔画制图字体 + 系统 TTF 回退 + FONTALT 替换）
代表"B 方案"，任务书中"47.2 MB / 单图 42–53 秒 / --window +20~23% 偏差 / SHX 覆盖率 85.3%"这些数字
**本机无法复核，全部记入 NOT_MEASURED**。实测的 cadcraft-cli 单图耗时是 **0.141 秒**（见 §5），
与"42–53 秒"差两个数量级，说明那组数字属于另一个工具，不要混用。

**任务书前提修正（实测）**：本机**有仿宋**。`C:\Windows\Fonts\simfang.ttf` 存在，PIL 读出族名
`('FangSong','Regular')`；按 "fangsong" 关键字搜不到只是因为文件名是 `simfang`。
黑体 `simhei.ttf`、宋体 `simsun.ttc`、微软雅黑 `msyh.ttc`、`NotoSansSC-VF.ttf`、`SourceHanSansCN.ttf` 也在位。
**但本机没有任何 `.shx` 形字体文件**（`C:\Windows\Fonts`、`C:\Program Files`、用户目录、标书软件目录全搜过）。

---

## 2. 合成语料（make_fixture.py，幂等）

图纸单位 mm，图幅 400×320，外框 + 内边框（`LWPOLYLINE closed`）。实体清单（fx_full.dxf）：
`LWPOLYLINE×4、LINE×4、ARC×2、ELLIPSE×1、CIRCLE×1、TEXT×3、MTEXT×2、INSERT×3`（含 1 个带 6 个
ATTRIB 的标题栏块 `TITLEBLOCK`，块内再嵌套 1 个 `SYMBOL` 子块 = 嵌套块引用）。

文字样式三档，专门用来把三家能力拉开：

| 样式 | 字体声明 | 用途 |
|---|---|---|
| `CNHEI` | `simhei.ttf`（存在） | 中文 TTF 基线 |
| `CNFANG` | `simfang.ttf`（存在） | 标题栏仿宋要求 |
| `SHX1` | `gben.shx` + 大字体 `chineset.shx`（**本机均不存在**） | 形字体缺失时的回退行为 |

内容含中文与特殊符号：`一层平面图`、`仿宋标题栏`、`标题栏 ±0.000`、`比例 1:100 层高 3.6m`、
`GSHX-01 轴线`、`SHX 覆盖测试 3.500`，属性含 `某某建筑工程有限公司 / 给排水管网 / 建施-01 / 1:500 / 2026-10-22 / 审 核`。

为做"归因不混叠"，同一套几何还派生了 6 个消融变体：`fx_empty`（无实体，空图底噪）、`fx_frame`（只有外框，
像素标定锚）、`fx_geom`、`fx_text_ttf`、`fx_text_shx`、`fx_block`。每个分区只出现在对应变体里，
所以"这块墨到底是谁画的"没有歧义。

---

## 3. 断言环设计（不自比）

1. **像素锚 = 图幅外框矩形**（外部几何，不是渲染器常量）。逐张图用墨迹包围盒把 DXF 的 `(0,0,400,320)`
   线性映射到像素，再按 `fixture_manifest.json` 里的分区矩形裁窗。自检：同一方案各变体的标定包围盒
   偏差 **全部为 0 px**（A/B/C 都是 `[0,0,0,0]`），说明裁窗没有错位。
2. **空图基线** = `fx_empty` 渲染结果，用它取"纸面众数色"（A/C 白 `[255,255,255]`，B 深灰 `[33,40,48]`），
   墨量按"与纸面色最大通道差 > 40"计，**不假设"非白"**（否则 B 的深底会被算成 100% 墨）。空图底噪实测 **0 px**（四家都干净）。
3. **成字判定 = RapidOCR（onnxruntime 中文模型，本机可用）**，对每个裁片做字符级召回；
   方块 `□`、空心笔画字、占位灰框在 OCR 下都会掉召回，不会被"有墨"骗过去。整图也各跑一次 OCR 作为交叉验证。
4. **图框是否闭合**单独量（四条边的墨覆盖比例）——这一项直接抓出 C 丢了 closed 多段线的一条边。

---

## 4. 三方案主对比表（全部为实测值）

| 指标 | A ezdxf+matplotlib | B1 cadcraft 默认 | B2 cadcraft+FONTALT | C 现状 Pillow 占位 |
|---|---|---|---|---|
| 生产默认调用能否出图 | 是 | 是 | 是 | **否（抛异常，0 张）** |
| 纸面背景 | 白 `#ffffff` | **深灰 `#212830`** | 深灰 `#212830` | 白 |
| 画布像素 | 2400×1600 | 2400×1600 | 2400×1600 | 4842×3897（dpi=300） |
| 整图非背景墨量 px（占比） | 125 665（3.27%） | 154 578（4.03%） | 171 947（4.48%） | **32 243（0.17%）** |
| LINE 区墨量 | 2 770 | 3 363 | 3 363 | **0** |
| LWPOLYLINE（含凸度弧） | 3 557 | 4 906 | 4 906 | 5 848（**图框左边缺失**） |
| **ARC 区墨量** | 1 475 | 1 349 | 1 349 | **0** |
| **ELLIPSE 区墨量** | 1 103 | 1 062 | 1 062 | **0** |
| CIRCLE 区墨量 | 649 | 614 | 614 | **0** |
| 图框四边覆盖(左/右/上/下) | 1/1/1/1 | 1/1/1/1 | 1/1/1/1 | **0.001**/1/1/1 |
| 中文 TTF 文字 OCR 召回（7 串均值） | **0.956** | 0.956 | 0.956 | **0.000** |
| SHX 文字 OCR 召回（6 串均值） | **0.433** | 0.745 | **1.000** | **0.000** |
| — 其中含中文的 SHX 串 | **0.150（豆腐块 □）** | 0.950 | 1.000 | 0.000 |
| — 其中纯字母数字 SHX 串 | 1.000 | **0.334（空心笔画字，OCR 不认）** | 1.000 | 0.000 |
| 块 INSERT 几何（标题栏线） | 31 511 px | 46 422 px | 47 719 px | **0 px** |
| 块属性 ATTRIB 文字 | 全部可 OCR | 全部可 OCR | 全部可 OCR | **全丢** |
| 嵌套块（块内 INSERT） | 渲染 | 渲染 | 渲染 | 不处理 |
| 中文是否"成字"（非方块/缺字） | TTF 样式成字；**SHX 样式中文=□** | 成字 | 成字 | 无字（只有灰框 + `Aa`） |
| 仿宋要求（simfang） | 正确成字 | 正确成字 | 正确成字 | 不适用 |
| `±0.000` 特殊符号 | 召回 1.0 | 召回 1.0 | 召回 1.0 | 0 |
| 单张耗时中位数（5 次） | 0.437 s（热）/ 2.312 s（冷） | **0.141 s** | **0.141 s** | 1.047 s（direct 模式） |
| 峰值 RSS 中位数 | 231.7 MB（热，累计上界）/ 166.4 MB（冷） | 61.6 MB | 43.0 MB | 126.5 MB |
| 依赖重量 | ezdxf 19.7 + matplotlib 30.4 + numpy 31.8 + Pillow 15.3 ≈ **97 MB**（需新增 matplotlib） | 单文件 **17.70 MB**，零 Python 依赖 | 同 B1 | ezdxf + Pillow ≈ 35 MB（现状） |
| 原生读 DWG | **否**（`ezdxf.readfile` → `OSError: not a DXF file`，必须先 ODA 转 DXF） | **是**（`convert x.dwg x.png` 0.397 s，与本例 DXF 路径**逐像素相同**，max diff 0） | 是 | 同 A（依赖上游 DXF） |

整图 OCR 交叉验证（外部字面锚，人读一眼就能复核）：

- A：`标题栏±0.000 一层平面图 比例1:100层高3.6m 仿宋标题栏 SHX 3.500 GSHX-01 某某建筑工程有限公司 给排水管网 □-01 1:500 2026-10-22`
  → **SHX 样式的中文全部变成 □，属性"审 核"整格丢失**
- B1：`… SH×覆盖测试3.500 轴线 GSHX-O1 … 建施-O1 … 审核` → 中文全在，**空心笔画字被 OCR 读成 ×/O**
- B2：`… SHX 覆盖测试3.500 轴线 GSHX-01 … 建施-01 … 审核` → **全串命中**
- C：`（空）` → **整图 OCR 零命中**

---

## 5. 耗时 / 内存实测口径

`fx_full.dxf`，每方案 5 次取中位数（`time.monotonic` + `psutil` 5 ms 轮询 RSS，含子进程树）。

| 方案 | 中位数 s | min–max s | 峰值 RSS 中位 MB | 口径 |
|---|---|---|---|---|
| A 热（进程内，模块已加载） | 0.437 | 0.406–0.469 | 231.7 | 生产 `drawing_service` 常驻进程的形态；RSS 是同进程累计，属**上界** |
| A 冷（新起 python 进程） | 2.312 | 2.219–2.438 | 166.7 | 与 B/C 同口径可比 |
| B1 | 0.141 | 0.125–0.172 | 61.6 | 子进程冷启动 |
| B2 | 0.141 | 0.125–0.156 | 43.0 | 子进程冷启动 |
| C（direct，绕过 main()） | 1.047 | 1.015–1.141 | 126.5 | 子进程冷启动 |
| C（生产 CLI 调用） | — | — | — | **NOT_MEASURED：5/5 次崩溃，无产物可计时** |

注意 C 的画布是 A/B 的 4 倍像素（4842×3897 vs 2400×1600），墨量占比不能跨方案直接比大小，
本表的跨方案比较一律用"分区有无墨 + OCR 召回"，不用绝对墨量排名。

---

## 6. 失败模式清单（实测复现）

### C（现状通道）——两级故障

1. **入口级：两条分支都抛异常，一张图都出不来**
   - 生产默认调用（`services/drawing_service.py:97` 的 args，不带 `--layout`）：
     `dwg_to_png.py:134` `ps = [l for l in doc.layouts if l.is_paper_space]` →
     **`AttributeError: 'Modelspace' object has no attribute 'is_paper_space'`**
     （ezdxf 1.4.4 里 `doc.layouts` 迭代会带出 Modelspace；134 行注释还写着"真正渲染文字需 font 安装"）
   - 带 `--layout Model`：`dwg_to_png.py:131` `doc.layouts[args.layout]` →
     **`TypeError: 'Layouts' object is not subscriptable`**（应为 `doc.layouts.get(...)`）
   - 用 `fx_empty.dxf`（只有 2 个空布局、0 实体）复测同样两条崩溃 → **与图纸内容无关，纯 API 腐化**。
2. **实体级：即使绕过 main() 直接调 `render_layout_to_png()`，丢的比画的多的多**（本探针用 direct 模式量得）
   - `LINE` → `e.start` 不存在（应 `e.dxf.start`）→ AttributeError 被 `except Exception: pass` 静默吞掉 → **4/4 条直线消失**
   - `CIRCLE` → `e.center` 同上 → 消失；`TEXT` → `e.text` 不存在（应 `e.dxf.text`）→ **3/3 单行文字消失**
   - `ARC / ELLIPSE / INSERT` → 源码注释"ARC/ELLIPSE/INSERT 等后续可补"，**根本没实现** → 弧、椭圆、块、属性、图例、标题栏全丢
   - `MTEXT` → 只画 60×12 灰框 + 字面串 `Aa`（分区墨量 181 px，OCR 零命中）
   - `LWPOLYLINE` → 不处理 `closed` 标志 → **图框左边缺失**（四边覆盖实测 left=0.001）
   - 颜色/线型/线宽全部忽略（一律 1 px 黑）

### B（cadcraft-cli 0.5.0）

- PNG 出图是**深色底**（`#212830`，屏幕配色），不是白图纸 → 直接塞进标书 DOCX/PDF 需要走 PLOT/白底导出或后处理反色。
- `CADCRAFT_FONTALT` 只认**字体族名或裸文件名**；传绝对路径 `C:\Windows\Fonts\simhei.ttf` 会被**静默忽略**
  （实测与默认输出**逐字节相同**，md5 一致）→ 这类"参数写错不报错"是运维地雷。
- 默认（B1）下 SHX 样式的 ASCII/数字用**自带单笔画空心字**：人眼可读，但 OCR 召回只有 0.334（`GSHX-01`→读不出、`SHX`→`SH×`、
  `01`→`O1`），意味着**下游任何"图纸文字抽取/图号对账"会失效**。
- 上游自述 pre-alpha（套件里唯一未过 alpha 门），"存 DWG 会丢未建模内容"，36 个测试全是合成 fixture；
  本探针只做只读出图，未触发写盘路径 → 写回能力 NOT_MEASURED。
- `convert` 的 stderr 会打一行 `wrote xxx.png (N bytes)`，不是错误（基准脚本里被我原样记录）。

### A（ezdxf matplotlib）

- **缺 SHX 时中文回退成豆腐块 `□`**（实测含中文 SHX 串召回 0.150，`轴线/覆盖测试/审核` 全丢，`审 核` 格 OCR 结果只有一个 `口`）。
  修法明确：出图前把 `font=*.shx` 的 TEXT STYLE 改写成 TTF（本机有 simhei/simsun/仿宋），或给 ezdxf 的 FontManager
  注册替换表。本探针**未测修复后的覆盖率**（属修复验证，不在选型轮范围）。
- **不能直接读 DWG**（`OSError: File ... is not a DXF file`）→ 必须保留 ODA 前置。
- 生产 ODA 探测路径写死 `C:\Program Files\ODA\ODAFileConverter 24.4dwg\ODAFileConverter.exe`，
  本机实装 `ODAFileConverter 27.1.0` → 实测 `probe_converters()` 返回
  `{"available_converters": [], "can_convert_dwg": false}`：**转换器明明在，系统却报"没有"**。
  这条不修，A/B 都拿不到 DWG 输入。
- 依赖比现状重（要新增 matplotlib 30.4 MB + numpy 31.8 MB）。
- 默认按 model space 出图；布局/视口出图需另写（本轮未测）。

---

## 7. 接入建议（给产线的动作，不含本次改动）

1. **停用 C**：`services/drawing_service.py:92` → `api/bid_tools.py:909/934/957` 这条 `dwg_to_png` 通道
   当前返回的是崩溃后的 error dict，上层若把它当"图纸已出图"会静默丢图。第一步就是把出图后端换掉。
2. **主链路接 A**（纯 Python、白底、块/属性/弧/椭圆/中文 TTF 全通、无外部 exe、无许可证新增）：
   在 `dwg_to_png.py` 同层新增一个 `render_dxf_to_png(dxf, png, dpi)`，内部用
   `RenderContext + Frontend + MatplotlibBackend(adjust_figure=False)`，
   `Configuration(background_policy=CUSTOM, custom_bg_color="#ffffff")`，画布固定 2400×1600 或按 dpi 换算；
   **出图前做一次 TEXT STYLE 清洗**：凡 `font` 以 `.shx` 结尾（含 `bigfont`）的样式，替换为
   `simhei.ttf`（或项目规定的仿宋 `simfang.ttf`），并把 `$LTSCALE/线宽` 保持默认。
   前置条件：`requirements.txt` 加 `matplotlib`；修 ODA 探测路径（改成 glob `C:\Program Files\ODA\*\ODAFileConverter.exe`）。
3. **B 作为兜底/对照保留**：它的两个实测优势值得留着——单图 0.141 s（A 冷启动的 1/16）、
   **原生读 DWG**（可跳过 ODA，本例与 DXF 路径逐像素相同）、零 Python 依赖单文件 17.7 MB。
   但接进产线前必须先解决：白底出图（PLOT/反色）、空心笔画字对下游文字抽取的影响、pre-alpha 稳定性。
   建议先在离线批量任务上跑真图 100 张做人工比对，再决定是否升级为主链路。
4. **字体基线**：标书标题栏要求的仿宋本机有（`simfang.ttf`/FangSong），A/B 都正确成字；
   但**图纸常用的 `.shx` 形字体本机一个都没有**，正式环境要按甲方图纸清单补装 SHX 字库，
   否则任何方案都只能"替换成 TTF"，字宽/字距与甲方原图不一致（本探针无法量化该偏差）。

---

## 8. NOT_MEASURED 清单（未跑成功/未测，如实列）

| 项 | 原因 |
|---|---|
| cad2pdf 全链路（47.2 MB、单图 42–53 s、`--window` +20~23% 偏差、SHX 覆盖 85.3%、未落地补丁） | **本机不存在该工具**：`G:\程序开发` maxdepth3 文件名搜索、artcraft 全仓内容搜索、标书软件仓内容搜索均无 `cad2pdf`；无法复核任务书数字 |
| 真 SHX 字形还原度（笔画/字宽与甲方一致否） | 本机**无任何 `.shx` 文件**（系统字体目录、Program Files、用户目录、两个仓库均搜过），只能测"缺失时的回退行为" |
| C 的生产 CLI 计时/内存 | 5/5 次抛异常，无产物可测 |
| 真实甲方 DWG（复杂图、外部参照、代理实体）出图 | 纪律禁止读写真实标书语料；只用自造 DXF + 自经 ODA 转出的 17 KB 合成 DWG |
| DWG 直读保真度（B 方案 vs ODA 转换链） | 仅合成图；与 DXF 路径逐像素相同（max diff 0），不能外推到真图 |
| 布局/图纸空间（paper space + viewport）出图 | 三方案都按 model space 测；C 的 paper space 分支本身就是崩溃点 |
| A 加"SHX→TTF 样式改写"后的覆盖率 | 属修复验证，不在本轮选型范围（预期可达 B2 水平，未实测） |
| 100+ 实体真实图纸下的耗时/内存曲线、并发出图 | 合成语料仅 21 个顶层实体，未压测 |
| 峰值内存精确值 | 5 ms 轮询 RSS，短尖峰可能低估（已在口径里声明） |
| 中文 OCR 的整图 precision（误识别率） | 只做了分区字符召回 recall；OCR 本身有噪声（如"一层平面图"被读成"层平面图"，召回 0.8），未做双向对齐评分 |
| cadcraft 写盘（DWG/DXF 保存）能力 | 只做只读出图，未触发其已知"存盘丢内容"路径 |
| L1（COM 驱动真 CAD） | 本机无 AutoCAD/中望浩辰，`win32com` 亦未安装，任务书已列为不可用，未重复验证 |

---

## 9. 复现步骤与产物路径

```bat
cd /d %TEMP%\wb_drawing_probe
python make_fixture.py        :: 生成 7 个合成 DXF + fixture_manifest.json（幂等）
python probe_render.py        :: 渲染 4 方案 × 7 变体 + fx_full 基准（各 5 次）
python probe_assert.py        :: 像素锚 + 分区墨量 + RapidOCR 召回 -> assert_results.json
python probe_render.py --variants   :: 只出图
python probe_render.py --bench      :: 只跑基准
```

关键产物（绝对路径）：

- 脚本：`C:\Users\28970\AppData\Local\Temp\wb_drawing_probe\make_fixture.py`、`...\probe_render.py`、`...\probe_assert.py`
- 报告：`C:\Users\28970\AppData\Local\Temp\wb_drawing_probe\report.md`
- 语料/清单：`...\fx_full.dxf`（及 `fx_empty/frame/geom/text_ttf/text_shx/block.dxf`）、`...\fixture_manifest.json`
- 原始数据：`...\render_results.json`（耗时/内存/失败）、`...\assert_results.json`（像素/OCR）
- 三张主图（2400×1600 / C 为 4842×3897）：
  - A：`C:\Users\28970\AppData\Local\Temp\wb_drawing_probe\out\A_full.png`
  - B1：`...\out\B1_full.png`　B2（SHX→TTF）：`...\out\B2_full.png`
  - C：`...\out\C_full.png`（direct 模式产物；生产 CLI 模式无产物）
  - 消融变体：`...\out\{A,B1,B2,C}_{empty,frame,geom,text_ttf,text_shx,block}.png`
  - DWG 直读对照：`...\out\B_dwg.png`
  - OCR 裁片（76 张，人工复核用）：`...\crops\`
