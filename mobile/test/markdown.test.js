"use strict";
/* markdown 渲染器测试：代码块/表格/列表/引用/嵌套/行内/XSS 安全 */
const { test } = require("node:test");
const assert = require("node:assert/strict");
const C = require("../app.js");

test("标题 # ~ ####，##### 起按段落（v1 行为）", () => {
  assert.equal(C.renderMarkdown("# a"), "<h1>a</h1>");
  assert.equal(C.renderMarkdown("### c"), "<h3>c</h3>");
  assert.equal(C.renderMarkdown("#### d"), "<h4>d</h4>");
  assert.equal(C.renderMarkdown("##### e"), "<p>##### e</p>");
});

test("行内：加粗/斜体/删除线/行内 code", () => {
  assert.match(C.renderMarkdown("**b**"), /<strong>b<\/strong>/);
  assert.match(C.renderMarkdown("*i*"), /<em>i<\/em>/);
  assert.match(C.renderMarkdown("~~d~~"), /<del>d<\/del>/);
  assert.match(C.renderMarkdown("`c`"), /<code>c<\/code>/);
});

test("链接：仅 http/https，其余协议只显示文本", () => {
  const html = C.renderMarkdown("[a](https://x.com/p)");
  assert.match(html, /<a href="https:\/\/x\.com\/p" target="_blank" rel="noopener noreferrer">a<\/a>/);
  const bad = C.renderMarkdown("[a](javascript:alert(1))");
  assert.ok(!bad.includes("javascript:"), "javascript: 链接不得进入 href");
  assert.ok(!bad.includes("<a "), "非 http(s) 链接不得生成 <a>");
  assert.match(bad, /<p>a\)<\/p>/); // url 部分在第一个 ) 处截断，残留 ) 按文本显示
});

test("围栏代码块：转义 + 轻量高亮 span", () => {
  const html = C.renderMarkdown("```js\nconst x = 1;\n```");
  assert.match(html, /<pre class="md-pre"><code data-lang="js" class="lang-js">/);
  assert.match(html, /<span class="tok-kw">const<\/span>/);
  assert.match(html, /<span class="tok-num">1<\/span>/);
});

test("代码块内容必须转义（XSS）", () => {
  const html = C.renderMarkdown('```\n<img src=x onerror=alert(1)>\n```');
  assert.ok(!html.includes("<img"), "代码块内不得出现未转义标签");
  assert.ok(html.includes("&lt;img"));
});

test("流式未闭合围栏也渲染（回合进行中）", () => {
  const html = C.renderMarkdown("text\n```py\ndef f():\n  pass");
  assert.match(html, /<p>text<\/p>/);
  assert.match(html, /<pre class="md-pre"><code data-lang="py"/);
  assert.match(html, /<span class="tok-kw">def<\/span>/);
});

test("水平线", () => {
  assert.match(C.renderMarkdown("---"), /<hr>/);
  assert.match(C.renderMarkdown("***"), /<hr>/);
});

test("无序/有序/嵌套列表", () => {
  const ul = C.renderMarkdown("- a\n- b");
  assert.equal(ul, "<ul><li>a</li><li>b</li></ul>");
  const ol = C.renderMarkdown("1. a\n2. b");
  assert.equal(ol, "<ol><li>a</li><li>b</li></ol>");
  const nested = C.renderMarkdown("- a\n  - b\n    - c\n- d");
  assert.equal(nested, "<ul><li>a<ul><li>b<ul><li>c</li></ul></li></ul></li><li>d</li></ul>");
  const mixed = C.renderMarkdown("- a\n  1. x\n  2. y");
  assert.match(mixed, /<ul><li>a<ol><li>x<\/li><li>y<\/li><\/ol><\/li><\/ul>/);
});

test("引用块 + 嵌套引用", () => {
  const q = C.renderMarkdown("> hi **b**");
  assert.equal(q, "<blockquote><p>hi <strong>b</strong></p></blockquote>");
  const nested = C.renderMarkdown("> a\n>> b");
  assert.match(nested, /<blockquote><p>a<\/p><blockquote><p>b<\/p><\/blockquote><\/blockquote>/);
});

test("表格：表头/分隔/对齐/多行", () => {
  const html = C.renderMarkdown("| a | b | c |\n| --- | :-: | ---: |\n| 1 | 2 | 3 |\n| 4 | 5 | 6 |");
  assert.match(html, /<div class="md-table-wrap"><table><thead><tr><th>a<\/th><th style="text-align:center">b<\/th><th style="text-align:right">c<\/th><\/tr><\/thead><tbody>/);
  assert.match(html, /<td>1<\/td><td style="text-align:center">2<\/td><td style="text-align:right">3<\/td>/);
  assert.match(html, /<td>4<\/td><td style="text-align:center">5<\/td><td style="text-align:right">6<\/td>/);
  // 无对齐列时不输出 style
  assert.match(C.renderMarkdown("| a |\n| --- |\n| 1 |"), /<th>a<\/th>/);
});

test("表格未写完（只有表头）时按段落渲染，不崩", () => {
  const html = C.renderMarkdown("| a | b |");
  assert.match(html, /<p>\| a \| b \|<\/p>/);
});

test("段落内换行 <br>；空行分段", () => {
  const html = C.renderMarkdown("l1\nl2\n\np2");
  assert.equal(html, "<p>l1<br>l2</p><p>p2</p>");
});

test("整体 XSS：HTML 标签一律转义", () => {
  const html = C.renderMarkdown('<script>alert(1)</script>\n\n<img src=x onerror=alert(2)>');
  assert.ok(!html.includes("<script"));
  assert.ok(!html.includes("<img"));
  assert.ok(html.includes("&lt;script&gt;"));
});

test("代码块 + 表格 + 列表混合大文档稳定性", () => {
  const src = [
    "# 标题", "", "段落一。", "",
    "- 项目 A", "  - 子项", "",
    "> 引用", "",
    "| k | v |", "| --- | --- |", "| a | 1 |", "",
    "```sh", "echo hi # x", "```", "",
    "结尾 **粗**。"
  ].join("\n");
  const html = C.renderMarkdown(src);
  assert.match(html, /<h1>标题<\/h1>/);
  assert.match(html, /<ul><li>项目 A<ul><li>子项<\/li><\/ul><\/li><\/ul>/);
  assert.match(html, /<blockquote><p>引用<\/p><\/blockquote>/);
  assert.match(html, /<table>/);
  assert.match(html, /<span class="tok-kw">echo<\/span>/);
  assert.match(html, /<strong>粗<\/strong>/);
});
