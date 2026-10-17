"use strict";
/* 行 diff（LCS）测试 */
const { test } = require("node:test");
const assert = require("node:assert/strict");
const C = require("../app.js");

test("完全相同 → 全 same", () => {
  const ops = C.diffLines("a\nb\nc", "a\nb\nc");
  assert.deepEqual(ops.map(o => o.type), ["same", "same", "same"]);
  assert.deepEqual(C.diffStats(ops), { added: 0, removed: 0, same: 3 });
});

test("中间替换 → del + ins 各一", () => {
  const ops = C.diffLines("a\nb\nc", "a\nx\nc");
  assert.deepEqual(ops.map(o => o.type), ["same", "del", "ins", "same"]);
  assert.deepEqual(C.diffStats(ops), { added: 1, removed: 1, same: 2 });
});

test("纯新增 / 纯删除", () => {
  assert.deepEqual(C.diffLines("a", "a\nb").map(o => o.type), ["same", "ins"]);
  assert.deepEqual(C.diffLines("a\nb", "a").map(o => o.type), ["same", "del"]);
});

test("空输入（空串按一个空行参与 diff）", () => {
  assert.deepEqual(C.diffLines("", "a").map(o => o.type), ["del", "ins"]);
  assert.deepEqual(C.diffLines("a", "").map(o => o.type), ["del", "ins"]);
  assert.deepEqual(C.diffStats(C.diffLines("", "")), { added: 0, removed: 0, same: 1 });
});

test("LCS 最优性：取最长公共子序列（x,A,z 长度 3）", () => {
  // a: x A y B z ; b: x B A z → LCS = x A z（长度 3）
  const st = C.diffStats(C.diffLines("x\nA\ny\nB\nz", "x\nB\nA\nz"));
  assert.equal(st.same, 3);
  assert.equal(st.added + st.removed, 3);
});

test("超 600 行降级：全删全增（不卡死 UI）", () => {
  const big1 = Array.from({ length: 700 }, (_, i) => "l" + i).join("\n");
  const big2 = Array.from({ length: 700 }, (_, i) => "m" + i).join("\n");
  const st = C.diffStats(C.diffLines(big1, big2));
  assert.equal(st.removed, 700);
  assert.equal(st.added, 700);
  assert.equal(st.same, 0);
});

test("renderLineDiffHtml：行类 + 内容转义 + 省略提示", () => {
  const html = C.renderLineDiffHtml("a\n<b>x</b>", "a\nc");
  assert.match(html, /<div class="diff-line diff-same">a<\/div>/);
  assert.match(html, /<div class="diff-line diff-del">&lt;b&gt;x&lt;\/b&gt;<\/div>/);
  assert.match(html, /<div class="diff-line diff-ins">c<\/div>/);
  const capped = C.renderLineDiffHtml("1\n2\n3", "9", 2);
  assert.match(capped, /diff-more/);
});
