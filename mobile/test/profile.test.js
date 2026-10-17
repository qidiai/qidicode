"use strict";
/* Profile / 最近会话 序列化与 CRUD 测试 */
const { test } = require("node:test");
const assert = require("node:assert/strict");
const C = require("../app.js");

test("makeProfile：默认名/字段裁剪/id 唯一", () => {
  const p = C.makeProfile("  ", " ws://h/ws ", " s ", " c ");
  assert.equal(p.name, "未命名连接");
  assert.equal(p.url, "ws://h/ws");
  assert.equal(p.secret, "s");
  assert.ok(p.id && p.createdAt > 0 && p.lastUsedAt > 0);
  assert.notEqual(C.makeProfile("a", "u", "", "").id, C.makeProfile("a", "u", "", "").id);
});

test("sanitizeProfile：url 必填；脏对象被清洗", () => {
  assert.equal(C.sanitizeProfile(null), null);
  assert.equal(C.sanitizeProfile({ name: "x" }), null);
  const s = C.sanitizeProfile({ url: " ws://h/ws ", name: "  ", junk: 1, createdAt: "bad" });
  assert.equal(s.name, "未命名连接");
  assert.equal(s.url, "ws://h/ws");
  assert.equal(typeof s.createdAt, "number");
});

test("profiles 序列化 roundtrip + 垃圾过滤 + 上限 50", () => {
  const list = [C.makeProfile("a", "ws://a/ws", "", ""), C.makeProfile("b", "ws://b/ws", "", "")];
  const back = C.parseProfiles(C.serializeProfiles(list));
  assert.equal(back.length, 2);
  assert.equal(back[0].url, "ws://a/ws");
  assert.deepEqual(C.parseProfiles("not json"), []);
  assert.deepEqual(C.parseProfiles('{"a":1}'), []);
  assert.deepEqual(C.parseProfiles('[{"junk":1},null]'), []);
  const many = Array.from({ length: 60 }, (_, i) => C.makeProfile("p" + i, "ws://h" + i + "/ws", "", ""));
  assert.equal(C.parseProfiles(C.serializeProfiles(many)).length, 50);
});

test("upsertProfile：同 id 替换去重；缺 id 追加；上限 50", () => {
  const a = C.makeProfile("a", "ws://a/ws", "", "");
  let list = C.upsertProfile([], a);
  assert.equal(list.length, 1);
  const a2 = C.sanitizeProfile(Object.assign({}, a, { name: "a2" }));
  list = C.upsertProfile(list, a2);
  assert.equal(list.length, 1);
  assert.equal(list[0].name, "a2");
  const many = Array.from({ length: 60 }, (_, i) => C.makeProfile("p" + i, "ws://h" + i + "/ws", "", ""));
  assert.equal(C.upsertProfile(many, C.makeProfile("new", "ws://new/ws", "", "")).length, 50);
});

test("removeProfileById / findProfileById", () => {
  const a = C.makeProfile("a", "ws://a/ws", "", "");
  const b = C.makeProfile("b", "ws://b/ws", "", "");
  const list = [a, b];
  assert.equal(C.findProfileById(list, a.id).name, "a");
  assert.equal(C.findProfileById(list, "nope"), null);
  assert.equal(C.removeProfileById(list, a.id).length, 1);
  assert.equal(C.removeProfileById(list, a.id)[0].id, b.id);
});

test("recent 序列化 roundtrip + sid 必填 + 去重 + 按时间倒序 + 上限 20", () => {
  const e1 = { profileId: "p1", sid: "s1", title: "t1", ts: 1000 };
  const e2 = { profileId: "p1", sid: "s2", title: "", ts: 3000 };
  const back = C.parseRecent(C.serializeRecent([e1, e2]));
  assert.equal(back.length, 2);
  assert.equal(back[0].sid, "s2"); // ts 大的在前
  assert.deepEqual(C.parseRecent("junk"), []);
  assert.deepEqual(C.parseRecent('[{"title":"no sid"}]'), []);
  const ups = C.upsertRecent([e1, e2], { profileId: "p1", sid: "s1", title: "t1b", ts: 5000 });
  assert.equal(ups.length, 2); // 同 sid 去重
  assert.equal(ups[0].title, "t1b");
  const many = Array.from({ length: 30 }, (_, i) => ({ profileId: "p", sid: "s" + i, title: "", ts: i }));
  assert.equal(C.upsertRecent(many, { profileId: "p", sid: "sx", title: "", ts: 999 }).length, 20);
  assert.equal(C.removeRecent(ups, "s1").length, 1);
});
