"use strict";
/* 基础纯函数测试：escapeHtml / prettyInput / cwdTail */
const { test } = require("node:test");
const assert = require("node:assert/strict");
const C = require("../app.js");

test("escapeHtml：五个敏感字符 + null/undefined/数字", () => {
  assert.equal(C.escapeHtml(`&<>"'`), "&amp;&lt;&gt;&quot;&#39;");
  assert.equal(C.escapeHtml(null), "");
  assert.equal(C.escapeHtml(undefined), "");
  assert.equal(C.escapeHtml(42), "42");
});

test("prettyInput：JSON 字符串美化 / 非法 JSON 原样 / 对象美化 / 循环引用兜底", () => {
  assert.equal(C.prettyInput('{"a":1}'), '{\n  "a": 1\n}');
  assert.equal(C.prettyInput("[1,2]"), "[\n  1,\n  2\n]");
  assert.equal(C.prettyInput("{oops"), "{oops");
  assert.equal(C.prettyInput("plain"), "plain");
  assert.equal(C.prettyInput({ a: [1] }), '{\n  "a": [\n    1\n  ]\n}');
  const cyc = {}; cyc.self = cyc;
  assert.equal(typeof C.prettyInput(cyc), "string"); // 不抛异常
});

test("cwdTail：Windows / POSIX / 空值", () => {
  assert.equal(C.cwdTail("G:\\work\\proj"), "proj");
  assert.equal(C.cwdTail("/home/u/proj"), "proj");
  assert.equal(C.cwdTail(""), "会话");
  assert.equal(C.cwdTail(null), "会话");
  assert.equal(C.cwdTail("proj"), "proj");
});
