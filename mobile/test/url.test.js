"use strict";
/* ws URL 解析 / 构建测试（扫码粘贴降级 + 连接鉴权 query） */
const { test } = require("node:test");
const assert = require("node:assert/strict");
const C = require("../app.js");

test("parseWsUrl：ws:// + server-key + cwd", () => {
  const p = C.parseWsUrl("ws://192.168.1.10:2419/ws?server-key=abc&cwd=G:%5Cwork%5Cproj");
  assert.deepEqual(p, { url: "ws://192.168.1.10:2419/ws", secret: "abc", cwd: "G:\\work\\proj" });
});

test("parseWsUrl：scheme 归一化（http→ws，https→wss）", () => {
  assert.equal(C.parseWsUrl("http://h:8000/ws").url, "ws://h:8000/ws");
  assert.equal(C.parseWsUrl("https://h/ws").url, "wss://h/ws");
  assert.equal(C.parseWsUrl("wss://h/ws").url, "wss://h/ws");
});

test("parseWsUrl：无 query / 空串 / 非 URL → null", () => {
  assert.deepEqual(C.parseWsUrl("ws://h:2419/ws"), { url: "ws://h:2419/ws", secret: "", cwd: "" });
  assert.equal(C.parseWsUrl(""), null);
  assert.equal(C.parseWsUrl("   "), null);
  assert.equal(C.parseWsUrl("192.168.1.10:2419"), null);
  assert.equal(C.parseWsUrl("garbage"), null);
});

test("parseWsUrl：secret 别名 secret/key 也识别", () => {
  assert.equal(C.parseWsUrl("ws://h/ws?secret=s1").secret, "s1");
  assert.equal(C.parseWsUrl("ws://h/ws?key=k1").secret, "k1");
});

test("buildWsUrl：无 secret 原样；有 secret 追加 ? 或 &，值编码", () => {
  assert.equal(C.buildWsUrl("ws://h/ws", ""), "ws://h/ws");
  assert.equal(C.buildWsUrl("ws://h/ws", "abc"), "ws://h/ws?server-key=abc");
  assert.equal(C.buildWsUrl("ws://h/ws?x=1", "a b&c"), "ws://h/ws?x=1&server-key=a%20b%26c");
});

test("build/parse  roundtrip", () => {
  const url = C.buildWsUrl("ws://h/ws", "s1");
  const p = C.parseWsUrl(url);
  assert.equal(p.url, "ws://h/ws");
  assert.equal(p.secret, "s1");
});
