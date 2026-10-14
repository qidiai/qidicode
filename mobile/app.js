/* qidicode 手机端瘦客户端 —— ACP over WebSocket
 * 纯 Vanilla JS（ES2020，classic script），零依赖、零 CDN、零构建、零额外网络请求。
 * 实现依据：规格书 T1 v2（协议细节 P0–P11）。
 * 帧约定：1 个 WS text 帧 = 1 条完整 JSON-RPC 2.0 消息（JSON.stringify 产生单行）。
 */
(function () {
  "use strict";

  /* ================================ 常量 ================================ */
  var PROTOCOL_VERSION = 1;                // ACP v1（必须是数字；字符串会被服务端解析成 V0）
  var KEY_PARAM = "server-key";            // 浏览器 WebSocket 无法自定义 header，用 query 鉴权
  var LS = {
    url: "qidiMobile.url",
    secret: "qidiMobile.secret",
    cwd: "qidiMobile.cwd",
    sessionId: "qidiMobile.sessionId"
  };
  var RECONNECT_DELAYS = [1, 2, 4, 8, 16]; // 秒（P9 退避）
  var MAX_RECONNECT = 5;
  var INIT_TIMEOUT_MS = 15000;             // P11：initialize 15s 无响应即断开走重连

  /* ============================== 小工具 =============================== */
  function $(id) { return document.getElementById(id); }
  function on(el, ev, fn) { if (el) el.addEventListener(ev, fn); }
  function store(k, v) { try { localStorage.setItem(k, v); } catch (e) { /* 隐私模式等，忽略 */ } }
  function load(k) { try { return localStorage.getItem(k); } catch (e) { return null; } }
  function drop(k) { try { localStorage.removeItem(k); } catch (e) { /* ignore */ } }

  function escapeHtml(s) {
    return String(s == null ? "" : s).replace(/[&<>"']/g, function (c) {
      return ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", "\"": "&quot;", "'": "&#39;" })[c];
    });
  }

  function el(tag, cls, text) {
    var n = document.createElement(tag);
    if (cls) n.className = cls;
    if (text != null) n.textContent = text; // textContent 天然转义
    return n;
  }

  function delay(ms) { return new Promise(function (r) { setTimeout(r, ms); }); }

  /* ======================= Markdown 渲染器（自研，禁外部库） ======================= */
  /* 顺序：先整体 HTML 转义 → 围栏代码块 → 行内 code → 粗/斜体 → 链接 → 标题/列表/水平线 → 段落。
   * 明确不实现：表格、脚注、嵌套结构、图片语法。
   * 返回的 HTML 已全部转义，可安全用于 innerHTML。
   */
  function renderMarkdown(src) {
    var text = escapeHtml(src);      // 第一步即转义，防 XSS
    var codes = [];

    // 1. 围栏代码块 ``` lang ```（lang 仅作 data 属性，不引入高亮库）
    text = text.replace(/```([^\n`]*)\n?([\s\S]*?)```/g, function (m, lang, code) {
      codes.push({ lang: lang.trim(), code: code.replace(/\n$/, "") });
      return "\u0000CODE" + (codes.length - 1) + "\u0000";
    });
    // 1b. 流式未闭合的围栏（回合进行中，尾随未闭合）
    text = text.replace(/```([^\n`]*)\n?([\s\S]*)$/, function (m, lang, code) {
      codes.push({ lang: lang.trim(), code: code.replace(/\n$/, "") });
      return "\u0000CODE" + (codes.length - 1) + "\u0000";
    });

    function inline(s) {
      // 2. 行内 code
      s = s.replace(/`([^`]+)`/g, "<code>$1</code>");
      // 3. 粗体、斜体
      s = s.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
      s = s.replace(/\*([^*]+)\*/g, "<em>$1</em>");
      // 4. 链接（url 必须 http/https 开头，否则仅显示文本）
      s = s.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, function (m, label, url) {
        if (/^https?:\/\//i.test(url)) {
          return '<a href="' + url + '" target="_blank" rel="noopener noreferrer">' + label + "</a>";
        }
        return label;
      });
      return s;
    }

    function codeHtml(idx) {
      var c = codes[idx];
      return '<pre class="md-pre"><code data-lang="' + c.lang + '">' + c.code + "</code></pre>";
    }
    function expandTokens(s) {
      return s.replace(/\u0000CODE(\d+)\u0000/g, function (m, i) { return codeHtml(+i); });
    }

    var lines = text.split("\n");
    var out = [];
    var para = [];
    var list = null;

    function flushPara() {
      if (para.length) { out.push("<p>" + para.join("<br>") + "</p>"); para = []; }
    }
    function flushList() {
      if (list) {
        var tag = list.ordered ? "ol" : "ul";
        out.push("<" + tag + ">" + list.items.map(function (t) { return "<li>" + t + "</li>"; }).join("") + "</" + tag + ">");
        list = null;
      }
    }

    for (var i = 0; i < lines.length; i++) {
      var line = lines[i];
      var m;
      var onlyCode = line.match(/^\u0000CODE(\d+)\u0000$/);
      if (onlyCode) { flushPara(); flushList(); out.push(codeHtml(+onlyCode[1])); continue; }
      if (/^\s*$/.test(line)) { flushPara(); flushList(); continue; }

      // 5. 标题 # ~ ####
      if ((m = line.match(/^(#{1,4})\s+(.*)$/))) {
        flushPara(); flushList();
        var lv = m[1].length;
        out.push("<h" + lv + ">" + inline(m[2]) + "</h" + lv + ">");
        continue;
      }
      // 5. 水平线 --- 或 *** 或 ___
      if (/^\s*([-*_])(\s*\1){2,}\s*$/.test(line)) { flushPara(); flushList(); out.push("<hr>"); continue; }
      // 5. 无序列表
      if ((m = line.match(/^\s*[-*+]\s+(.*)$/))) {
        flushPara();
        if (!list || list.ordered) { flushList(); list = { ordered: false, items: [] }; }
        list.items.push(inline(m[1]));
        continue;
      }
      // 5. 有序列表
      if ((m = line.match(/^\s*\d+\.\s+(.*)$/))) {
        flushPara();
        if (!list || !list.ordered) { flushList(); list = { ordered: true, items: [] }; }
        list.items.push(inline(m[1]));
        continue;
      }
      // 6. 段落 / 换行
      flushList();
      para.push(inline(expandTokens(line)));
    }
    flushPara(); flushList();
    return out.join("");
  }

  /* ================================ 状态 ================================ */
  var ws = null;
  var conn = null;                 // { url, secret, cwd }
  var clientReqId = 0;             // P0：客户端请求 id 从 1 递增
  var pending = {};                // id -> function(msg)
  var connected = false;
  var everConnected = false;
  var intentionalClose = false;
  var reconnectCount = 0;
  var reconnectTimer = null;
  var initTimer = null;            // P11 initialize 超时定时器
  var loadSessionSupported = false;
  var sessionId = null;
  var turnState = "idle";          // idle | running
  var curAssistantEl = null;
  var curAssistantBuf = "";
  var curThoughtBody = null;
  var curThoughtBuf = "";
  var sentPromptText = null;       // 本回合已发送的 prompt 文本（仅用于服务端不回声时的兜底）
  var sawUserMessageChunk = false; // 本回合是否已收到服务端 user_message_chunk 回声
  var toolCards = {};              // toolCallId -> card 对象
  var toolSeq = 0;                 // toolCallId 缺失时的兜底键
  var planCard = null;
  var permQueue = [];              // 并发权限请求 FIFO
  var currentPerm = null;
  var toastTimer = null;

  /* ============================ 发送与请求 ============================ */
  function nextId() { clientReqId += 1; return clientReqId; }

  function sendRaw(obj) {
    if (ws && ws.readyState === 1 /* OPEN */) {
      ws.send(JSON.stringify(obj)); // 单行 JSON == 单条 WS text 帧
    } else {
      console.debug("[qidi] socket 未就绪，丢弃消息：", obj);
    }
  }

  // 客户端主动请求：分配递增 id，按 id 匹配响应
  function request(method, params) {
    return new Promise(function (resolve, reject) {
      var id = nextId();
      pending[id] = function (msg) {
        if (msg.error) reject(msg.error);
        else resolve(msg.result);
      };
      sendRaw({ jsonrpc: "2.0", id: id, method: method, params: params });
    });
  }

  function errorText(err) {
    if (err == null) return "未知错误";
    if (typeof err === "string") return err;
    if (err.message) return err.message;
    if (err.data != null) return typeof err.data === "string" ? err.data : JSON.stringify(err.data);
    return JSON.stringify(err);
  }

  // auth_required（PC 端 qidi 未登录）识别：容错匹配错误码/文案/数据
  function isAuthRequired(err) {
    try { return /auth_required/i.test(JSON.stringify(err)); } catch (e) { return false; }
  }

  /* ============================ 连接生命周期 ============================ */
  function startSocket(isReconnect) {
    intentionalClose = false;
    var sep = conn.url.indexOf("?") >= 0 ? "&" : "?";
    var full = conn.url + sep + KEY_PARAM + "=" + encodeURIComponent(conn.secret);
    var socket;
    try {
      socket = new WebSocket(full);
    } catch (e) {
      if (isReconnect) startReconnect();
      else showConnectError("无法创建 WebSocket 连接：" + e.message);
      return;
    }
    ws = socket;
    socket.onopen = function () { onSocketOpen(isReconnect); };
    socket.onmessage = function (ev) { onSocketMessage(ev); };
    socket.onerror = function (ev) { console.debug("[qidi] WebSocket error", ev); };
    socket.onclose = function (ev) { onSocketClose(ev, isReconnect); };
  }

  function onSocketOpen(isReconnect) {
    hideBanner();
    hideConnectError();
    // 注意：不在此处重置 reconnectCount——只有会话真正就绪（onSessionReady）才算一轮成功，
    // 否则"WS 能连上但 initialize 一直超时"会让退避计数永远归零、无法收敛到 5 轮上限。
    handshake(isReconnect);
  }

  function onSocketClose(ev, isReconnect) {
    if (intentionalClose) return; // 主动断开不触发重连
    if (!everConnected) {
      // 首次连接失败：页内红字，保留输入
      showConnectError("连接失败：请检查 —— ① 服务器地址是否正确（含端口与 /ws 路径）；② 访问密钥是否与 --secret 一致；③ 电脑防火墙是否放行该端口。可能原因：地址不对 / 密钥不对 / 防火墙拦截。");
      return;
    }
    enterReconnectFlow();
  }

  // 握手：initialize →（session/load 恢复 或 session/new 新建）
  function handshake(isReconnect) {
    clearInitTimer();
    initTimer = setTimeout(onInitTimeout, INIT_TIMEOUT_MS); // P11
    request("initialize", { protocolVersion: PROTOCOL_VERSION, clientCapabilities: {} })
      .then(function (result) {
        clearInitTimer();
        loadSessionSupported = !!(result && result.agentCapabilities && result.agentCapabilities.loadSession);
        var storedId = load(LS.sessionId);
        if (storedId && loadSessionSupported) {
          // P2 会话恢复：三字段全部必填；响应不含 sessionId，沿用请求时的 id。
          // replay 历史通知会先于 load 响应到达，sessionId 已就绪 → 直接渲染。
          sessionId = storedId;
          updateHeader();
          return request("session/load", { sessionId: storedId, cwd: conn.cwd, mcpServers: [] })
            .then(function () { return { sessionId: storedId, fresh: false }; })
            .catch(function (err) {
              drop(LS.sessionId); // load 失败 → 清掉已存 id，转 P3 新建
              console.debug("[qidi] session/load 失败，改新建会话：", errorText(err));
              return newSession();
            });
        }
        return newSession(); // P3
      })
      .then(function (sess) {
        sessionId = sess.sessionId;
        store(LS.sessionId, sessionId);
        onSessionReady(isReconnect, sess.fresh);
      })
      .catch(function (err) { onHandshakeFail(err, isReconnect); });
  }

  // P3 session/new；失败时区分 auth_required（PC 未登录）与 cwd 无效
  function newSession() {
    return request("session/new", { cwd: conn.cwd, mcpServers: [] })
      .then(function (result) {
        if (!result || !result.sessionId) throw { kind: "protocol", error: "服务端未返回 sessionId" };
        return { sessionId: result.sessionId, fresh: true };
      })
      .catch(function (err) {
        if (err && err.kind) throw err;
        if (isAuthRequired(err)) throw { kind: "auth", error: err };
        throw { kind: "cwd", error: err }; // 多为 cwd 无效
      });
  }

  function onSessionReady(isReconnect, fresh) {
    connected = true;
    everConnected = true;
    reconnectCount = 0;
    hideBanner();
    hideConnectError();
    resetTurnRefs();
    updateHeader();
    setTurnState("idle");
    showView("session");
    if (fresh) {
      // 全新会话：消息区属于另一个会话，清空
      clearMessages();
      setHeaderTitle("");
      if (isReconnect) addSystem("已重新连接（服务端已无可恢复的历史，已新建会话）");
    }
    // 非 fresh（session/load 恢复）时：replay 历史已流式渲染进消息区，不得清空
  }

  function onHandshakeFail(err, isReconnect) {
    clearInitTimer();
    var msg = errorText(err && err.error ? err.error : err);
    if (isReconnect) { enterReconnectFlow(); return; }
    if (err && err.kind === "auth") {
      toast("PC 端 qidi 未登录，请先在电脑上完成登录");
    } else if (err && err.kind === "cwd") {
      toast("项目路径无效（cwd 被服务端拒绝）：" + msg);
    } else if (conn) {
      showConnectError("连接失败：" + msg + "。可能原因：地址不对 / 密钥不对 / 服务端未启动 / 防火墙拦截。");
    }
    closeSocket(true);
    showView("connect");
  }

  function closeSocket(intentional) {
    intentionalClose = !!intentional;
    clearInitTimer();
    if (reconnectTimer) { clearTimeout(reconnectTimer); reconnectTimer = null; }
    if (ws) { try { ws.close(); } catch (e) { /* ignore */ } ws = null; }
    pending = {};   // 丢弃在途请求，避免旧链路上的回调串扰
    connected = false;
  }

  // P11：initialize 15s 无响应 → 断开并走重连流程
  function onInitTimeout() {
    initTimer = null;
    console.debug("[qidi] initialize 15s 无响应，断开并进入重连流程");
    everConnected = true; // 让随后的 onclose 走 P9 重连分支而非首连失败分支
    closeSocket(false);
  }
  function clearInitTimer() { if (initTimer) { clearTimeout(initTimer); initTimer = null; } }

  // P9 重连入口：清空权限队列、保证横幅可见、启动退避重试
  function enterReconnectFlow() {
    clearPermQueue();
    everConnected = true;
    showView("session");
    startReconnect();
  }

  // P9 重连：退避 [1,2,4,8,16] 秒，失败达 5 次转手动重试
  function startReconnect() {
    if (intentionalClose) return;
    clearPermQueue(); // 断线时清空权限弹窗队列（P7）
    reconnectCount += 1;
    if (reconnectCount > MAX_RECONNECT) {
      showBanner("重连失败（已尝试 " + MAX_RECONNECT + " 次），请检查网络后手动重试", true);
      return;
    }
    showBanner("连接已断开，重连中… 第 " + reconnectCount + " 次", false);
    var wait = RECONNECT_DELAYS[reconnectCount - 1] || 16;
    reconnectTimer = setTimeout(function () {
      reconnectTimer = null;
      if (intentionalClose) return;
      startSocket(true);
    }, wait * 1000);
  }

  /* ============================ 消息分派 ============================ */
  function onSocketMessage(ev) {
    var msg;
    try { msg = JSON.parse(ev.data); }
    catch (e) { console.debug("[qidi] 无法解析的帧，忽略：", ev.data); return; }
    if (msg == null || typeof msg !== "object") { console.debug("[qidi] 非法消息，忽略：", msg); return; }

    // 服务端发起（有 method）：带 id 为反向请求，否则为通知
    if (typeof msg.method === "string") {
      if (msg.id !== undefined && msg.id !== null) handleReverseRequest(msg);
      else handleNotification(msg);
      return;
    }
    // 响应：有 id + (result|error)，无 method
    if (msg.id !== undefined && msg.id !== null && ("result" in msg || "error" in msg)) {
      handleResponse(msg);
      return;
    }
    // P10：既非响应也非已知通知 → 忽略 + console.debug
    console.debug("[qidi] 未知消息，忽略：", msg);
  }

  function handleResponse(msg) {
    var cb = pending[msg.id];
    if (cb) { delete pending[msg.id]; cb(msg); }
    else console.debug("[qidi] 收到无主响应，忽略 id=", msg.id);
  }

  function handleNotification(msg) {
    if (msg.method === "session/update") { handleUpdate(msg.params || {}); return; }
    if (msg.method === "_x.ai/session_notification") { handleExtNotification(msg.params || {}); return; }
    // P10：纯通知（无 id）未识别 → 忽略，不报错
    console.debug("[qidi] 未识别通知，忽略：", msg.method);
  }

  function handleReverseRequest(msg) {
    if (msg.method === "session/request_permission") { enqueuePermission(msg); return; }
    if (msg.method === "initialize") { console.debug("[qidi] 忽略服务端发来的 initialize"); return; }
    // P10 硬规则：任何带 id 的未识别方法必须回 -32601，否则 agent 侧永久挂起
    sendRaw({ jsonrpc: "2.0", id: msg.id, error: { code: -32601, message: "Method not found" } });
  }

  // P6b：_x.ai/session_notification ext 通知；turn_completed 是回合结束信号
  function handleExtNotification(params) {
    var update = params.update;
    var kind = (update && typeof update === "object") ? update.sessionUpdate : undefined;
    if (kind === "turn_completed") { endTurn(); return; }
    console.debug("[qidi] 未识别的 ext 通知，忽略：", kind);
  }

  // P6 session/update
  function handleUpdate(params) {
    var update = params.update;
    if (!update || typeof update !== "object") return;
    switch (update.sessionUpdate) {
      case "user_message_chunk": return onUserMessageChunk(update);
      case "agent_message_chunk": return onMessageChunk(update);
      case "agent_thought_chunk": return onThoughtChunk(update);
      case "tool_call": return onToolCall(update);
      case "tool_call_update": return onToolCallUpdate(update);
      case "plan": return onPlan(update);
      case "session_info_update": return onSessionInfoUpdate(update);
      case "turn_completed": return endTurn(); // 兜底：与本 fork 的 ext 通知信号幂等
      case "available_commands_update":
      case "current_mode_update":
      case "config_option_update":
      case "usage_update":
        return console.debug("[qidi] 明确忽略的 sessionUpdate：", update.sessionUpdate);
      default: console.debug("[qidi] 未知 sessionUpdate，忽略：", update.sessionUpdate); return;
    }
  }

  // content 容错读取：{type:"text",text} / 字符串 → 文本，其余 → null（忽略）
  function textOf(content) {
    if (typeof content === "string") return content;
    if (content && typeof content === "object" &&
        content.type === "text" && typeof content.text === "string") return content.text;
    return null;
  }
  // kind 容错读取：snake_case 字符串（"read"/"edit"/…）
  function kindText(kind) {
    if (typeof kind === "string" && kind) return kind;
    if (kind && typeof kind === "object" && typeof kind.type === "string") return kind.type;
    return "tool";
  }

  // P6 user_message_chunk：新用户气泡；replay 时它是回合边界（当前助手气泡先定稿）
  function onUserMessageChunk(update) {
    var t = textOf(update.content);
    if (t === null) return; // 非 text 忽略
    sawUserMessageChunk = true;                    // 本回合已有服务端回声，无需本地兜底
    curAssistantEl = null; curAssistantBuf = "";   // 助手气泡定稿，后续 chunk 另起新段
    curThoughtBody = null; curThoughtBuf = "";
    appendUserBubble(t);
  }

  function onSessionInfoUpdate(update) {
    var info = update.sessionInfo;
    if (info && typeof info === "object" && info.title != null) setHeaderTitle(String(info.title));
  }

  function onMessageChunk(update) {
    var t = textOf(update.content);
    if (t === null) return; // 非 text 忽略
    if (!curAssistantEl) curAssistantEl = appendAssistantBubble();
    curAssistantBuf += t;
    curAssistantEl.innerHTML = renderMarkdown(curAssistantBuf);
    scrollBottom();
  }

  function onThoughtChunk(update) {
    var t = textOf(update.content);
    if (t === null) return; // 非 text 忽略
    if (!curThoughtBody) {
      var wrapEl = el("details", "thought");
      wrapEl.appendChild(el("summary", "thought-summary", "思考过程")); // 默认折叠
      curThoughtBody = el("pre", "thought-body");
      wrapEl.appendChild(curThoughtBody);
      $("messages").appendChild(wrapEl);
      curThoughtBuf = "";
    }
    curThoughtBuf += t;
    curThoughtBody.textContent = curThoughtBuf; // 流式追加（textContent 天然转义）
    scrollBottom();
  }

  function onToolCall(update) {
    var key = update.toolCallId != null ? String(update.toolCallId) : ("\u0001anon" + (toolSeq += 1));
    var card = createToolCard(update);
    card.key = key;
    toolCards[key] = card;
  }

  function onToolCallUpdate(update) {
    var key = update.toolCallId != null ? String(update.toolCallId) : null;
    var card = key ? toolCards[key] : null;
    if (!card) { console.debug("[qidi] tool_call_update 未匹配到卡片，忽略：", update.toolCallId); return; }
    if (typeof update.status === "string" && update.status) setToolStatus(card, update.status);
    if (update.title != null) card.titleEl.textContent = String(update.title);
    if (update.kind != null) card.kindEl.textContent = kindText(update.kind);
    if (Array.isArray(update.content)) setToolContent(card, update.content); // 全量替换，非追加
    if ("rawInput" in update) setToolRaw(card, "input", update.rawInput);
    if ("rawOutput" in update) setToolRaw(card, "output", update.rawOutput);
    scrollBottom();
  }

  function onPlan(update) {
    var entries = (update.plan && typeof update.plan === "object") ? update.plan.entries : null;
    if (!Array.isArray(entries)) { console.debug("[qidi] plan 缺少 entries，忽略：", update.plan); return; }
    if (!planCard) planCard = createPlanCard();
    renderPlan(planCard, entries); // 全量替换重绘
  }

  /* ============================ 回合状态机 ============================ */
  function setTurnState(state) {
    turnState = state;
    var btn = $("btn-send");
    var ta = $("input");
    if (state === "running") {
      btn.textContent = "取消";
      btn.classList.add("cancel"); btn.classList.remove("send");
      ta.disabled = true;
    } else {
      btn.textContent = "发送";
      btn.classList.remove("cancel"); btn.classList.add("send");
      ta.disabled = false;
    }
  }

  function onSendClick() {
    if (turnState === "running") cancelTurn();
    else sendPrompt();
  }

  // P4 session/prompt
  function sendPrompt() {
    var ta = $("input");
    var text = ta.value;
    if (!text.trim()) return;
    if (!sessionId) { toast("会话尚未就绪"); return; }

    // 不做本地回显：用户气泡一律以服务端 user_message_chunk 回声为准（避免重复）
    sentPromptText = text;         // 暂存发送文本，供 endTurn 兜底渲染
    sawUserMessageChunk = false;
    ta.value = "";
    autoGrow();

    // 本回合流式引用重置
    curAssistantEl = null; curAssistantBuf = "";
    curThoughtBody = null; curThoughtBuf = "";

    var id = nextId();
    pending[id] = function (msg) {
      if (msg.error) { toast("请求出错：" + errorText(msg.error)); endTurn(); return; }
      if (msg.result && msg.result.stopReason != null) {
        console.debug("[qidi] 回合结束，stopReason=", msg.result.stopReason);
      }
      endTurn(); // 与 P6b turn_completed 双信号，幂等
    };
    setTurnState("running");
    sendRaw({
      jsonrpc: "2.0",
      id: id,
      method: "session/prompt",
      params: { sessionId: sessionId, prompt: [{ type: "text", text: text }] }
    });
  }

  // P5 session/cancel（通知，无 id 无响应）
  function cancelTurn() {
    if (!sessionId) return;
    sendRaw({ jsonrpc: "2.0", method: "session/cancel", params: { sessionId: sessionId } });
    toast("已发送取消请求…");
    // 不立即置 idle，等服务端响应 / turn_completed
  }

  function endTurn() {
    if (turnState !== "running") return; // 幂等
    // 兜底：本回合发送过 prompt 但始终未收到服务端 user_message_chunk 回声 → 补渲染本地气泡
    if (sentPromptText !== null && !sawUserMessageChunk) appendUserBubble(sentPromptText);
    sentPromptText = null;
    sawUserMessageChunk = false;
    setTurnState("idle");
  }

  function resetTurnRefs() {
    curAssistantEl = null; curAssistantBuf = "";
    curThoughtBody = null; curThoughtBuf = "";
    sentPromptText = null; sawUserMessageChunk = false;
    toolCards = {}; planCard = null;
  }

  /* ============================ 工具 / 计划卡片 ============================ */
  function setToolStatus(card, status) {
    card.el.classList.remove("status-pending", "status-in_progress", "status-completed", "status-failed");
    card.el.classList.add("status-" + (status || "pending"));
  }

  // P6 tool_call：kind 是 snake_case 字符串；rawInput / content 都在 update 顶层，全部容错
  function createToolCard(update) {
    var card = el("div", "tool-card");
    var head = el("div", "tool-head");
    var dot = el("span", "tool-dot");
    var titleEl = el("span", "tool-title",
      (update.title != null && update.title !== "") ? String(update.title) : "工具调用");
    var kindEl = el("span", "tool-kind", kindText(update.kind));
    head.appendChild(dot);
    head.appendChild(titleEl);
    head.appendChild(kindEl);
    card.appendChild(head);

    var contentEl = el("div", "tool-content");
    contentEl.style.display = "none";
    card.appendChild(contentEl);

    var rawBox = el("div", "tool-raws");
    card.appendChild(rawBox);

    var obj = {
      id: update.toolCallId, el: card, dot: dot, titleEl: titleEl, kindEl: kindEl,
      contentEl: contentEl, rawBox: rawBox, rawInputEl: null, rawOutputEl: null
    };
    if (Array.isArray(update.content)) setToolContent(obj, update.content);
    if ("rawInput" in update) setToolRaw(obj, "input", update.rawInput);
    if ("rawOutput" in update) setToolRaw(obj, "output", update.rawOutput);
    setToolStatus(obj, (typeof update.status === "string" && update.status) ? update.status : "pending");

    $("messages").appendChild(card);
    scrollBottom();
    return obj;
  }

  // content 是【全量替换】：取 content 里 type==="text" 的项重写内容区；content 缺失则不动
  function setToolContent(card, content) {
    if (!Array.isArray(content)) return;
    var parts = [];
    content.forEach(function (item) {
      var t = textOf(item);
      if (t !== null) parts.push(t);
    });
    card.contentEl.textContent = parts.join("");
    card.contentEl.style.display = parts.length ? "" : "none";
  }

  // rawInput / rawOutput 出现时替换对应显示块（缺失则保留原块）
  function setToolRaw(card, which, raw) {
    var isOut = which === "output";
    var key = isOut ? "rawOutputEl" : "rawInputEl";
    var node = buildRawBlock(isOut ? "输出结果（rawOutput）" : "输入参数（rawInput）", raw);
    if (card[key]) card.rawBox.replaceChild(node, card[key]);
    else if (!isOut && card.rawOutputEl) card.rawBox.insertBefore(node, card.rawOutputEl);
    else card.rawBox.appendChild(node);
    card[key] = node;
  }

  function buildRawBlock(label, raw) {
    var wrapEl = el("div", "tool-raw");
    var btn = el("button", "raw-toggle", label);
    btn.type = "button";
    var pre = el("pre", "raw-pre collapsed", prettyInput(raw)); // 默认折叠，展示前 3 行
    on(btn, "click", function () {
      pre.classList.toggle("collapsed");
      btn.classList.toggle("open");
    });
    wrapEl.appendChild(btn);
    wrapEl.appendChild(pre);
    return wrapEl;
  }

  function prettyInput(raw) {
    if (typeof raw === "string") {
      var s = raw.trim();
      if (s && (s.charAt(0) === "{" || s.charAt(0) === "[")) {
        try { return JSON.stringify(JSON.parse(s), null, 2); } catch (e) { /* 原样 */ }
      }
      return raw;
    }
    try { return JSON.stringify(raw, null, 2); } catch (e) { return String(raw); }
  }

  function createPlanCard() {
    var card = el("div", "plan-card");
    card.appendChild(el("div", "plan-head", "执行计划"));
    var body = el("div", "plan-body");
    card.appendChild(body);
    $("messages").appendChild(card);
    return { el: card, body: body };
  }

  // P6 plan：entries 是 [{content, priority, status}]，全量替换重绘
  function renderPlan(card, entries) {
    card.body.innerHTML = "";
    entries.forEach(function (item) {
      var st = (typeof item.status === "string" && item.status) ? item.status : "pending";
      var glyph = st === "completed" ? "✓" : (st === "in_progress" ? "◐" : "○");
      var row = el("div", "plan-item status-" + st);
      row.appendChild(el("span", "plan-glyph", glyph));
      row.appendChild(el("span", "plan-name", (item && item.content != null) ? String(item.content) : ""));
      if (item && item.priority != null) row.appendChild(el("span", "plan-priority", String(item.priority)));
      card.body.appendChild(row);
    });
    scrollBottom();
  }

  /* ============================ 权限弹窗（P7） ============================ */
  function enqueuePermission(msg) {
    permQueue.push(msg);
    if (!currentPerm) showNextPermission();
  }

  // P7：断线/重连时必须清空权限队列并关闭弹窗
  function clearPermQueue() {
    permQueue = [];
    currentPerm = null;
    hideSheet();
  }

  function showNextPermission() {
    currentPerm = permQueue.length ? permQueue.shift() : null;
    if (!currentPerm) { hideSheet(); return; }

    var params = currentPerm.params || {};
    var tc = (params.toolCall && typeof params.toolCall === "object") ? params.toolCall : {};
    $("perm-tool").textContent = (tc.title != null && tc.title !== "") ? String(tc.title) : "工具权限请求";

    // rawInput 容错：规格放在 toolCall 内，也接受位于 params 顶层
    var rawInput = ("rawInput" in tc) ? tc.rawInput : (("rawInput" in params) ? params.rawInput : undefined);
    var pre = $("perm-input");
    if (rawInput !== undefined && rawInput !== null) { pre.textContent = prettyInput(rawInput); pre.style.display = ""; }
    else { pre.textContent = ""; pre.style.display = "none"; }

    var box = $("perm-options");
    box.innerHTML = "";
    var options = Array.isArray(params.options) ? params.options : [];
    options.forEach(function (opt) {
      var b = el("button", "btn perm-opt", (opt && opt.name != null) ? String(opt.name) : "允许");
      b.type = "button";
      on(b, "click", function () { respondPermission(opt ? opt.optionId : undefined); });
      box.appendChild(b);
    });

    showSheet();
  }

  function respondPermission(optionId) {
    if (!currentPerm) return;
    var id = currentPerm.id; // 反向请求原 id，必须原样回传
    var payload;
    if (optionId !== undefined && optionId !== null) {
      payload = { jsonrpc: "2.0", id: id, result: { outcome: { outcome: "selected", optionId: optionId } } };
    } else {
      payload = { jsonrpc: "2.0", id: id, result: { outcome: { outcome: "cancelled" } } };
    }
    sendRaw(payload);
    currentPerm = null;
    showNextPermission(); // 处理队列中的下一个
  }

  function cancelPermission() {
    if (!currentPerm) { hideSheet(); return; }
    var id = currentPerm.id;
    sendRaw({ jsonrpc: "2.0", id: id, result: { outcome: { outcome: "cancelled" } } });
    currentPerm = null;
    showNextPermission();
  }

  function showSheet() { $("perm-sheet").classList.remove("hidden"); }
  function hideSheet() { $("perm-sheet").classList.add("hidden"); }

  /* ============================ 菜单动作（P8） ============================ */
  function onNewSession() {
    $("menu").classList.add("hidden");
    var oldId = sessionId;
    var go = function () {
      resetTurnRefs();
      clearMessages();
      setHeaderTitle("");
      request("session/new", { cwd: conn.cwd, mcpServers: [] })
        .then(function (res) {
          if (!res || !res.sessionId) { toast("新建会话失败：服务端未返回 sessionId"); return; }
          sessionId = res.sessionId;
          store(LS.sessionId, sessionId);
          updateHeader();
          setTurnState("idle");
          toast("已新建会话");
        })
        .catch(function (err) {
          // P3：auth_required（PC 未登录）→ 中文 toast + 返回连接页
          if (isAuthRequired(err)) {
            toast("PC 端 qidi 未登录，请先在电脑上完成登录");
            closeSocket(true);
            showView("connect");
            return;
          }
          toast("新建会话失败：" + errorText(err && err.error ? err.error : err));
        });
    };
    if (oldId) {
      // 先对旧会话 session/close（成功失败均继续），再走 P3
      var rel = request("session/close", { sessionId: oldId }).catch(function () { /* 忽略 */ });
      Promise.race([rel, delay(1500)]).then(go);
    } else {
      go();
    }
  }

  function onDisconnect() {
    $("menu").classList.add("hidden");
    if (turnState === "running") { toast("当前回合进行中，请先取消或等待完成"); return; }
    var finish = function () { closeSocket(true); showView("connect"); };
    if (sessionId) {
      var rel = request("session/close", { sessionId: sessionId }).catch(function () { /* 忽略 */ });
      Promise.race([rel, delay(1500)]).then(finish);
    } else {
      finish();
    }
  }

  /* ============================ DOM 渲染工具 ============================ */
  function appendAssistantBubble() {
    var b = el("div", "msg assistant");
    $("messages").appendChild(b);
    return b;
  }
  function appendUserBubble(text) {
    var b = el("div", "msg user", text); // 纯文本
    $("messages").appendChild(b);
    scrollBottom();
  }
  function addSystem(text) {
    var b = el("div", "msg system", text);
    $("messages").appendChild(b);
    scrollBottom();
  }
  function clearMessages() { $("messages").innerHTML = ""; }
  function scrollBottom() { var m = $("messages"); m.scrollTop = m.scrollHeight; }

  function cwdTail(p) {
    if (!p) return "会话";
    var parts = String(p).split(/[\\/]+/).filter(function (x) { return x.length > 0; });
    return parts.length ? parts[parts.length - 1] : String(p);
  }
  function updateHeader() {
    $("head-cwd").textContent = cwdTail(conn ? conn.cwd : "");
    $("head-sid").textContent = sessionId ? String(sessionId).slice(0, 8) : "—";
  }
  // P6 session_info_update：update.sessionInfo.title
  function setHeaderTitle(t) {
    var n = $("head-title");
    if (!n) return;
    n.textContent = t || "";
    n.classList.toggle("hidden", !t);
  }

  function showView(name) {
    $("view-connect").classList.toggle("hidden", name !== "connect");
    $("view-session").classList.toggle("hidden", name !== "session");
  }

  function showBanner(text, isError) {
    $("banner-text").textContent = text;
    $("banner").classList.remove("hidden");
    $("banner").classList.toggle("error", !!isError);
    $("btn-retry").classList.toggle("hidden", !isError);
  }
  function hideBanner() { $("banner").classList.add("hidden"); }

  function showConnectError(text) { var e2 = $("connect-error"); e2.textContent = text; e2.classList.remove("hidden"); }
  function hideConnectError() { $("connect-error").classList.add("hidden"); }

  function toast(text) {
    var t = $("toast");
    t.textContent = text;
    t.classList.remove("hidden");
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = setTimeout(function () { t.classList.add("hidden"); }, 3000); // 3s 自动消失
  }

  function autoGrow() {
    var ta = $("input");
    ta.style.height = "auto";
    var cs = window.getComputedStyle(ta);
    var lh = parseFloat(cs.lineHeight) || 22;
    var max = Math.ceil(lh * 6); // 最大 6 行
    ta.style.height = Math.min(ta.scrollHeight, max) + "px";
  }

  /* ============================ 连接页逻辑 ============================ */
  // 地址预填守卫：仅 location.protocol === "http:" 时预填 ws://<location.host>/ws；
  // https: 与 file: 一律预填空串（无法据此推断出正确的 ws:// 后端地址）。
  function defaultUrl() {
    if (location.protocol === "http:") return "ws://" + location.host + "/ws";
    return "";
  }

  function onConnect() {
    var url = $("in-url").value.trim();
    var secret = $("in-secret").value.trim();
    var cwd = $("in-cwd").value.trim();
    if (!url || !secret || !cwd) {
      showConnectError("请完整填写服务器地址、访问密钥和项目路径。");
      return;
    }
    store(LS.url, url); store(LS.secret, secret); store(LS.cwd, cwd);
    conn = { url: url, secret: secret, cwd: cwd };
    hideConnectError();
    clearPermQueue();
    setHeaderTitle("");
    everConnected = false;
    reconnectCount = 0;
    startSocket(false);
  }

  /* ================================ 初始化 ================================ */
  function init() {
    var savedUrl = load(LS.url);
    $("in-url").value = (savedUrl != null && savedUrl !== "") ? savedUrl : defaultUrl();
    $("in-secret").value = load(LS.secret) || "";
    $("in-cwd").value = load(LS.cwd) || "";

    // 输入即存
    on($("in-url"), "input", function () { store(LS.url, this.value); });
    on($("in-secret"), "input", function () { store(LS.secret, this.value); });
    on($("in-cwd"), "input", function () { store(LS.cwd, this.value); });

    on($("btn-connect"), "click", onConnect);
    on($("btn-send"), "click", onSendClick);
    on($("input"), "input", autoGrow);
    on($("input"), "keydown", function (e) {
      if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) { e.preventDefault(); onSendClick(); }
    });

    on($("btn-menu"), "click", function (e) {
      e.stopPropagation();
      $("menu").classList.toggle("hidden");
    });
    on($("menu-new"), "click", onNewSession);
    on($("menu-disconnect"), "click", onDisconnect);
    on(document, "click", function () { $("menu").classList.add("hidden"); });

    on($("btn-retry"), "click", function () { reconnectCount = 0; hideBanner(); clearPermQueue(); startSocket(true); });
    on($("perm-close"), "click", cancelPermission);
    on($("perm-cancel"), "click", cancelPermission);
    on($("perm-backdrop"), "click", cancelPermission);

    autoGrow();
    showView("connect");
  }

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", init);
  else init();
})();
