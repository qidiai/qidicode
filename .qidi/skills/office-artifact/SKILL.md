---
name: office-artifact
description: "办公产物卡片与任务工作区登记。任何办公 skill（office-tools、bid-write、bid-review、bid-perfect、bid-diagram、bid-render、pdf-to-word-ocr、seal-extractor 等）产出交付文件（.docx/.xlsx/.pptx/.pdf/.png/.jpg/.html/.md）时必须调用本技能，将产物以统一卡片形式呈现给用户并登记到任务工作区。触发词：产物登记、产物卡片、present_files、交付物呈现、生成文件后展示。"
---

# Office Artifact —— 产物卡片与任务工作区

统一所有办公任务的**交付物呈现**规范。对标豆包"侧边工作台"、WorkBuddy `present_files`：
任务结束后，产物不是对话里的一行路径，而是一张**可打开、可预览、可追溯**的卡片。

## 强制规则（抄自 WorkBuddy result_presentation，已验证有效）

1. **任何产出交付文件的任务，最后一步必须生成本次任务的产物卡片总览**——不许只回一行文件路径就结束。
2. **只登记本次新产生的交付物**，不登记仅读取/修改的中间文件、临时文件、`__pycache__`。
3. 一次任务有多个产物时**一次性全部列出**（含子代理产出的），按生成顺序编号。
4. 卡片生成失败时降级为 Markdown 表格，**不许因卡片失败跳过呈现**。

## 使用方法

```bash
# 登记单个或多个产物（相对/绝对路径均可）
python <skill_dir>/scripts/card.py add 输出文件.docx
python <skill_dir>/scripts/card.py add a.docx b.xlsx c.png --task "周报汇总"

# 输出当前任务工作区的卡片总览（任务结束时必须调用）
python <skill_dir>/scripts/card.py show

# 打开某个产物（卡片 [打开] 动作的实现）
python <skill_dir>/scripts/card.py open 1        # 按编号打开
python <skill_dir>/scripts/card.py open <路径>    # 按路径打开

# 查看当前工作区登记簿
python <skill_dir>/scripts/card.py list
```

## 任务工作区（豆包"一次任务=一个工作区"的本地实现）

- 每个任务工作区 = `~/.qidi/office-workspaces/<工作区名>/`（默认 `default`）
- `manifest.json` 记录该工作区全部产物（路径/大小/时间/来源技能/摘要）
- 切换工作区：`--task 名称`；工作区之间互不干扰，可随时 `show` 回看历史任务产物
- **会话结束呈现规则**：对话中若未显式建工作区，产物自动落入 `default`，下次会话仍可 `list`/`open` 追溯

## 卡片形态（TUI 内渲染为文本卡片）

```
📦 产物卡片 [default] 共 3 项
──────────────────────────────────────────────
 1. 投标函.docx            38 KB   2026-08-31 11:02  [bid-write]
    ├ 打开方式: python card.py open 1
 2. 施工平面布置图.png     412 KB  2026-08-31 11:05  [site-plan-drawing]
 3. 标书-完整版.docx       2.1 MB  2026-08-31 11:12  [bid-perfect]
──────────────────────────────────────────────
 呈现规则: 完成即出卡片；[打开]=系统默认程序所见即所得
```

## 与其他 skill 的关系

- **被调用方**：office-tools、bid-* 全家、pdf-to-word-ocr、seal-extractor、prose-expression、
  site-plan-drawing、gantt-chart-drawing 产出文件后调用 `card.py add` 登记
- **调用时机**：文件落盘成功后立即 `add`；任务最终回复前必须 `show`
- **预览增强（可选）**：docx/xlsx 需要视觉自检时，先走 office-tools 的
  `scripts/preview.py`（LibreOffice 渲染页面图），再把渲染图一并登记
