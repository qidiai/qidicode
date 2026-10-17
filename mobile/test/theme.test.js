"use strict";
/* 主题持久化逻辑测试（存储层在 DOM 侧，此处测纯解析规则） */
const { test } = require("node:test");
const assert = require("node:assert/strict");
const C = require("../app.js");

test("resolveTheme：显式选择优先于系统", () => {
  assert.equal(C.resolveTheme("dark", true), "dark");
  assert.equal(C.resolveTheme("light", false), "light");
});

test("resolveTheme：system/无记录跟随系统，默认深色", () => {
  assert.equal(C.resolveTheme("system", true), "light");
  assert.equal(C.resolveTheme("system", false), "dark");
  assert.equal(C.resolveTheme(null, true), "light");
  assert.equal(C.resolveTheme(null, false), "dark");
  assert.equal(C.resolveTheme(undefined, false), "dark");
});

test("resolveTheme：脏值按 system 处理", () => {
  assert.equal(C.resolveTheme("DARK", true), "light");
  assert.equal(C.resolveTheme("", false), "dark");
});
