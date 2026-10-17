/* qidicode 手机端 Service Worker —— 只缓存静态壳（app shell），零运行时逻辑。
 * 注册策略（app.js registerSW）：仅 http/https 注册；file:// 静默跳过。
 * 策略：壳资源 stale-while-revalidate；非 GET / 跨源请求一律旁路（WS 不经过 SW）。
 */
var CACHE = "qidi-mobile-shell-v1";
var ASSETS = [
  "./",
  "./index.html",
  "./style.css",
  "./app.js",
  "./manifest.webmanifest",
  "./icon.svg",
  "./icon-192.png",
  "./icon-512.png",
  "./icon-maskable-512.png"
];

self.addEventListener("install", function (e) {
  e.waitUntil(
    caches.open(CACHE)
      .then(function (c) { return c.addAll(ASSETS); })
      .then(function () { return self.skipWaiting(); })
      .catch(function () { /* 单个资源失败不阻塞安装 */ })
  );
});

self.addEventListener("activate", function (e) {
  e.waitUntil(
    caches.keys()
      .then(function (keys) {
        return Promise.all(keys.map(function (k) {
          if (k !== CACHE) return caches.delete(k);
        }));
      })
      .then(function () { return self.clients.claim(); })
  );
});

self.addEventListener("fetch", function (e) {
  if (e.request.method !== "GET") return;              // WS upgrade / POST 旁路
  var url;
  try { url = new URL(e.request.url); } catch (err) { return; }
  if (url.origin !== self.location.origin) return;     // 跨源旁路
  e.respondWith(
    caches.match(e.request).then(function (hit) {
      var net = fetch(e.request).then(function (res) {
        if (res && res.ok) {
          var cp = res.clone();
          caches.open(CACHE).then(function (c) { c.put(e.request, cp); });
        }
        return res;
      }).catch(function () { return hit; });
      return hit || net; // 有缓存先返缓存并后台更新；无缓存走网络
    })
  );
});
