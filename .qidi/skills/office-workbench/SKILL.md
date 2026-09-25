---
name: office-workbench
description: "浏览器办公工作台前端：一键启动本地 Web 界面，豆包式三区布局（左侧任务工作区列表/中间内容区/右侧产物面板），支持 docx 转 HTML 预览、图片直显、PDF/表格内嵌、任务工作区切换、产物点击预览与系统打开、批注反馈闭环入口。当用户要求'打开工作台'、'看看产物'、'可视化界面'、'前端界面'、'预览方案'、'看画布'时使用。零依赖安装（Python 标准库 + mammoth + openpyxl + PIL）。"
---

# Office Workbench —— 浏览器办公工作台

QIDI 办公场景的**图形前端**。对标豆包侧边工作台 / WorkBuddy 卡片面板，
但不写一行 Electron——本地起 HTTP 服务 + 浏览器渲染，TUI 仍是主入口。

## 启动

```bash
python ~/.qidi/skills/office-workbench/scripts/workbench.py            # 默认 http://localhost:8710
python ~/.qidi/skills/office-workbench/scripts/workbench.py --port 8720
```

启动后自动打开浏览器。工作台读的是 `~/.qidi/office-workspaces/`（office-artifact
登记的任务工作区），所以**先用 card.py 跑任务、再来工作台看产物**——两个技能天然联动。

## 界面结构（豆包式三区）

```
┌──────────┬────────────────────────┬──────────────┐
│ 任务工作区│       内容区            │   产物面板     │
│ · default │  (docx→HTML 预览       │  卡片列表      │
│ · 投标A   │   图片直显/PDF内嵌      │  点击即预览    │
│ · 周报    │   表格渲染/文本查看)    │  双击系统打开  │
│ + 新建    │                        │  批注反馈入口  │
└──────────┴────────────────────────┴──────────────┘
```

- **左侧**：任务工作区列表（= card.py --task 的工作区），点击切换，产物互不干扰
- **中间**：预览区。docx 自动转 HTML（mammoth，含表格）；png/jpg 直显；
  pdf 用浏览器原生查看器；xlsx 转 HTML 表格；md/txt 直出
- **右侧**：当前工作区产物卡片（文件名/大小/时间/来源技能），单击预览、
  "打开"按钮调系统默认程序（WPS/Word 所见即所得，可继续画布批注流程）

## 与其他技能的联动

| 动作 | 后端 |
|---|---|
| 产物登记进工作台 | office-artifact `card.py add --task` |
| 在 WPS 改完+加批注 | office-canvas `collect`/`verify` 反馈闭环 |
| 本地修改后刷新 | 工作台"刷新"按钮重读 manifest |
| 删除工作区 | 左侧 ✕（只删登记，不动文件） |

## 实现说明

- 纯标准库 http.server + 线程，单文件无前端构建链
- API：`GET /api/workspaces`、`GET /api/artifacts?task=`、
  `GET /api/preview?task=&path=`、`POST /api/open`（系统打开）、
  `POST /api/newtask`
- 路径安全：预览仅限已登记产物，不做任意文件读取
- 端口默认 8710，可 `--port`；关闭 Ctrl+C 即可，无残留进程

## 依赖

Python 3.8+；可选 `pip install mammoth openpyxl pillow`（docx/xlsx 预览增强，
未装则降级为"用系统程序打开"）
