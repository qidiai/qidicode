/* qidicode 手机端瘦客户端 —— ACP over WebSocket（移动端 Chat UI 重构版）
 * 纯 Vanilla JS（ES5 风格书写，兼容 Chromium 90+ WebView），零依赖、零 CDN、零构建、零额外网络请求。
 * 实现依据：规格书 T1 v2（协议细节 P0–P11）。
 * 帧约定：1 个 WS text 帧 = 1 条完整 JSON-RPC 2.0 消息（JSON.stringify 产生单行）。
 *
 * 文件结构（同文件内组织）：
 *   Part 1  QidiMobileCore —— 纯函数核心（无 DOM/localStorage 依赖）。
 *           UMD 双导出：node 下 `require("./app.js")` 直接测（node --test）；
 *           浏览器下挂 window.QidiMobileCore 供控制台调试。
 *   Part 2  DOM 应用 —— 全部界面与协议逻辑，仅在浏览器执行（typeof document 守卫）。
 *           原 app.js（v1）的功能一条不少保留，详见 README「不回归清单」。
 */

/* ==================================================================== */
/* ============ Part 1: 纯函数核心（UMD，可 node --test 直测） ============ */
/* ==================================================================== */
var QidiMobileCore = (function () {
  "use strict";

  var APP_VERSION = "2.0.0-mobile-ui";

  // localStorage 键（v1 四个旧键保持兼容，新增 profiles/recent/theme/activeProfile）
  var LS_KEYS = {
    url: "qidiMobile.url",
    secret: "qidiMobile.secret",
    cwd: "qidiMobile.cwd",
    sessionId: "qidiMobile.sessionId",
    profiles: "qidiMobile.profiles",
    recent: "qidiMobile.recent",
    theme: "qidiMobile.theme",
    activeProfile: "qidiMobile.activeProfile"
  };
  var KEY_PARAM = "server-key"; // 浏览器 WebSocket 无法自定义 header，用 query 鉴权

  /* ------------------------------ 基础 ------------------------------ */

  function escapeHtml(s) {
    return String(s == null ? "" : s).replace(/[&<>"']/g, function (c) {
      return ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", "\"": "&quot;", "'": "&#39;" })[c];
    });
  }

  // prettyInput：对象/JSON 字符串 → 缩进 JSON 文本；其余 → 字符串原文
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

  // cwdTail：取路径最后一段作标题
  function cwdTail(p) {
    if (!p) return "会话";
    var parts = String(p).split(/[\\/]+/).filter(function (x) { return x.length > 0; });
    return parts.length ? parts[parts.length - 1] : String(p);
  }

  /* --------------------------- 代码高亮（轻量） --------------------------- */
  /* 逐字符扫描 + 单趟转义输出，产物全部经过 escapeHtml，可安全 innerHTML。
   * 明确不做：完整语法树、模板字符串插值、各语言全量关键字——只做"关键字/字符串/
   * 数字/注释/函数名/全大写常量"六类轻量着色。css/html/markdown 不着色（原样转义）。
   */
  var LANG_ALIAS = {
    javascript: "js", jsx: "js", mjs: "js", node: "js",
    typescript: "ts", tsx: "ts",
    python: "py", python3: "py",
    bash: "sh", zsh: "sh", shell: "sh", console: "sh",
    rust: "rust", rs: "rust",
    golang: "go",
    yaml: "yml", yml: "yml", toml: "toml",
    json: "json", jsonc: "json",
    sql: "sql", mysql: "sql", postgres: "sql",
    java: "java", kotlin: "kt", swift: "swift", c: "c", cpp: "cpp", "c++": "cpp",
    ruby: "rb", php: "php", perl: "pl", lua: "lua", r: "r",
    dockerfile: "sh", makefile: "sh", ps1: "sh", powershell: "sh", bat: "sh", cmd: "sh"
  };
  var PLAIN_LANGS = { css: 1, html: 1, xml: 1, markdown: 1, md: 1, text: 1, txt: 1, diff: 1, "": 1 };
  var KEYWORDS = {
    common: ("async await break case catch class const continue debugger default delete do else enum export extends " +
      "finally for from function if implements import in instanceof interface let new of package private protected " +
      "public return static super switch this throw try type typeof var void while with yield true false null " +
      "def elif lambda pass None True False and or not is struct impl fn pub mut use mod trait match loop " +
      "func go chan defer select range map package echo then fi done esac local sudo exit set").split(" "),
    js: ("async await break case catch class const continue debugger default delete do else export extends finally " +
      "for from function get set if import in instanceof let new of return static super switch this throw try " +
      "typeof var void while with yield true false null undefined NaN").split(" "),
    ts: ("abstract as asserts async await break case catch class const continue declare default delete do enum " +
      "export extends finally for from function get set if implements import in instanceof interface keyof let " +
      "namespace new of private protected public readonly return satisfies static super switch this throw try " +
      "type typeof var void while with yield true false null undefined").split(" "),
    py: ("False None True and as assert async await break class continue def del elif else except finally for from " +
      "global if import in is lambda nonlocal not or pass raise return try while with yield match case").split(" "),
    sh: ("if then else elif fi for while until do done case esac in function select time return exit local export " +
      "source echo set unset shift trap sudo cd rm cp mv ls cat grep sed awk chmod chown mkdir touch kill ps").split(" "),
    rust: ("as async await break const continue crate dyn else enum extern false fn for if impl in let loop match " +
      "mod move mut pub ref return self Self static struct super trait true type unsafe use where while").split(" "),
    go: ("break case chan const continue default defer else fallthrough for func go goto if import interface map " +
      "package range return select struct switch type var true false nil iota").split(" "),
    java: ("abstract assert boolean break byte case catch char class const continue default do double else enum " +
      "extends final finally float for if implements import instanceof int interface long native new package " +
      "private protected public return short static super switch synchronized this throw throws transient try " +
      "void volatile while var record sealed true false null").split(" "),
    sql: ("SELECT FROM WHERE INSERT INTO VALUES UPDATE SET DELETE CREATE TABLE DROP ALTER ADD COLUMN JOIN LEFT " +
      "RIGHT INNER OUTER FULL ON AS AND OR NOT NULL IS IN EXISTS BETWEEN LIKE ORDER GROUP BY HAVING LIMIT " +
      "OFFSET UNION ALL DISTINCT PRIMARY KEY FOREIGN REFERENCES INDEX UNIQUE DEFAULT COUNT SUM AVG MIN MAX " +
      "CASE WHEN THEN ELSE END BEGIN COMMIT ROLLBACK TRANSACTION").split(" "),
    c: ("auto break case char const continue default do double else enum extern float for goto if int long " +
      "register return short signed sizeof static struct switch typedef union unsigned void volatile while " +
      "bool true false NULL").split(" "),
    cpp: ("alignas alignof and asm auto bool break case catch char class const constexpr continue decltype " +
      "default delete do double dynamic_cast else enum explicit export extern false float for friend goto if " +
      "inline int long mutable namespace new noexcept nullptr operator private protected public register " +
      "reinterpret_cast return short signed sizeof static static_cast struct switch template this throw true " +
      "try typedef typeid typename union unsigned using virtual void volatile while").split(" "),
    kt: ("as break by class companion const continue crossinline do else enum false for fun if import in infix " +
      "init inline inner interface internal is lateinit null object open operator out override package private " +
      "protected public reified return sealed super suspend this throw true try typealias val var vararg when " +
      "while").split(" "),
    swift: ("as associatedtype break case catch class continue default defer deinit do else enum extension " +
      "fallthrough false fileprivate final for func guard if import in indirect infix init inout internal is " +
      "lazy let nil open operator private protocol public repeat required rethrows return self Self static " +
      "struct subscript super switch throw throws true try typealias var weak where while").split(" "),
    rb: ("alias and begin break case class def defined? do else elsif end ensure false for if in module next " +
      "nil not or redo rescue retry return self super then true undef unless until when while yield").split(" "),
    php: ("abstract and array as break callable case catch class clone const continue declare default do echo " +
      "else elseif empty enddeclare endfor endforeach endif endswitch endwhile enum extends final finally fn " +
      "for foreach function global goto if implements include instanceof interface isset list match namespace " +
      "new or print private protected public readonly require return static switch throw trait try unset use " +
      "var while xor yield true false null").split(" "),
    lua: ("and break do else elseif end false for function goto if in local nil not or repeat return then true " +
      "until while").split(" "),
    yml: ("true false null yes no on off").split(" "),
    toml: ("true false").split(" ")
  };
  var COMMENT_STYLES = {
    js: { line: "//", block: ["/*", "*/"] }, ts: { line: "//", block: ["/*", "*/"] },
    java: { line: "//", block: ["/*", "*/"] }, c: { line: "//", block: ["/*", "*/"] },
    cpp: { line: "//", block: ["/*", "*/"] }, kt: { line: "//", block: ["/*", "*/"] },
    swift: { line: "//", block: ["/*", "*/"] }, go: { line: "//", block: ["/*", "*/"] },
    rust: { line: "//", block: ["/*", "*/"] }, php: { line: "//", block: ["/*", "*/"] },
    css: { line: "", block: ["/*", "*/"] },
    py: { line: "#" }, rb: { line: "#" }, sh: { line: "#" }, yml: { line: "#" },
    toml: { line: "#" }, lua: { line: "--" }, sql: { line: "--" }, r: { line: "#" }
  };

  function normalizeLang(lang) {
    var l = String(lang || "").trim().toLowerCase();
    if (!l) return "";
    if (LANG_ALIAS[l]) return LANG_ALIAS[l];
    return l;
  }

  function escapeRe(s) { return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"); }

  function highlightCode(code, lang) {
    var src = String(code == null ? "" : code);
    var key = normalizeLang(lang);
    if (PLAIN_LANGS[key]) return escapeHtml(src); // css/html/md 等不着色
    var kws = KEYWORDS[key] || KEYWORDS.common;
    var kwRe = kws.length ? new RegExp("^(?:" + kws.map(escapeRe).join("|") + ")(?![A-Za-z0-9_$])") : null;
    var comments = COMMENT_STYLES[key] || {};
    var out = "";
    var plain = "";
    function flushPlain() { if (plain) { out += escapeHtml(plain); plain = ""; } }
    function span(cls, text) { flushPlain(); out += '<span class="tok-' + cls + '">' + escapeHtml(text) + "</span>"; }
    var i = 0;
    var n = src.length;
    while (i < n) {
      var ch = src.charAt(i);
      var two = src.substr(i, 2);
      // 注释
      if (comments.line && two === comments.line) {
        var e = src.indexOf("\n", i); if (e < 0) e = n;
        span("cmt", src.slice(i, e)); i = e; continue;
      }
      if (comments.block && two === comments.block[0]) {
        var e2 = src.indexOf(comments.block[1], i + 2);
        e2 = e2 < 0 ? n : e2 + comments.block[1].length;
        span("cmt", src.slice(i, e2)); i = e2; continue;
      }
      // 字符串（` 模板串允许跨行，其余到行尾断）
      if (ch === "\"" || ch === "'" || ch === "`") {
        var j = i + 1;
        while (j < n) {
          if (src.charAt(j) === "\\") { j += 2; continue; }
          if (src.charAt(j) === ch) { j++; break; }
          if (src.charAt(j) === "\n" && ch !== "`") break;
          j++;
        }
        span("str", src.slice(i, j)); i = j; continue;
      }
      // 数字
      if (ch >= "0" && ch <= "9") {
        var k = i;
        while (k < n && /[0-9a-fA-FxXoObB_.]/.test(src.charAt(k))) k++;
        span("num", src.slice(i, k)); i = k; continue;
      }
      // 标识符 / 关键字 / 函数名 / 全大写常量
      if (/[A-Za-z_$]/.test(ch)) {
        var w = i;
        while (w < n && /[A-Za-z0-9_$]/.test(src.charAt(w))) w++;
        var word = src.slice(i, w);
        var after = src.slice(w).replace(/^\s+/, "").charAt(0);
        if (kwRe && kwRe.test(word)) span("kw", word);
        else if (after === "(") span("fn", word);
        else if (/^[A-Z][A-Z0-9_]{2,}$/.test(word)) span("const", word);
        else plain += word;
        i = w; continue;
      }
      plain += ch;
      i++;
    }
    flushPlain();
    return out;
  }

  /* --------------------------- Markdown 渲染器 --------------------------- */
  /* 自研，禁外部库。顺序：先在【原始文本】上抽取围栏代码块（保留原文供高亮），
   * 再对剩余文本整体 HTML 转义 → 块级解析（标题/水平线/引用/表格/嵌套列表/段落）
   * → 行内解析（code/粗/斜/删除线/链接）。
   * 支持：围栏代码块（含流式未闭合）、行内 code、粗体、斜体、删除线、http(s) 链接、
   *       # ~ #### 标题、水平线、有序/无序/缩进嵌套列表、引用块（可嵌套）、
   *       表格（含 :---/:--:/---: 对齐）、段落内 <br>。
   * 明确不实现：图片语法、脚注、任务列表、HTML 直通、转义竖线表格单元格。
   * 返回的 HTML 已全部转义，可安全用于 innerHTML。
   */
  var CODE_TOKEN_RE = /\u0000CODE(\d+)\u0000/g;

  function codeHtml(c) {
    var lang = (c && c.lang) || "";
    var safe = /^[a-z0-9_+-]+$/i.test(lang) ? lang : "";
    return '<pre class="md-pre"><code data-lang="' + escapeHtml(lang) + '"' +
      (safe ? ' class="lang-' + escapeHtml(safe) + '"' : "") + ">" +
      highlightCode(c ? c.code : "", lang) + "</code></pre>";
  }

  function expandTokens(s, codes) {
    return s.replace(CODE_TOKEN_RE, function (m, i) {
      var c = codes && codes[+i];
      return c ? codeHtml(c) : "";
    });
  }

  function inline(s) {
    // 2. 行内 code
    s = s.replace(/`([^`]+)`/g, "<code>$1</code>");
    // 3. 粗体、斜体
    s = s.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
    s = s.replace(/\*([^*]+)\*/g, "<em>$1</em>");
    // 3b. 删除线
    s = s.replace(/~~([^~]+)~~/g, "<del>$1</del>");
    // 4. 链接（url 必须 http/https 开头，否则仅显示文本）
    s = s.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, function (m, label, url) {
      if (/^https?:\/\//i.test(url)) {
        return '<a href="' + url + '" target="_blank" rel="noopener noreferrer">' + label + "</a>";
      }
      return label;
    });
    return s;
  }

  function isTableSep(line) {
    return /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/.test(line) && line.indexOf("-") >= 0;
  }

  function splitTableRow(line) {
    var s = String(line).trim();
    if (s.charAt(0) === "|") s = s.slice(1);
    if (s.charAt(s.length - 1) === "|") s = s.slice(0, -1);
    return s.split("|").map(function (c) { return c.trim(); });
  }

  function renderTable(lines, start, codes) {
    var header = splitTableRow(lines[start]);
    var aligns = splitTableRow(lines[start + 1]).map(function (c) {
      var l = c.charAt(0) === ":", r = c.charAt(c.length - 1) === ":";
      return l && r ? "center" : r ? "right" : l ? "left" : "";
    });
    function cell(tag, text, idx) {
      return "<" + tag + (aligns[idx] ? ' style="text-align:' + aligns[idx] + '"' : "") + ">" +
        inline(expandTokens(text || "", codes)) + "</" + tag + ">";
    }
    var html = '<div class="md-table-wrap"><table><thead><tr>';
    header.forEach(function (c, idx) { html += cell("th", c, idx); });
    html += "</tr></thead><tbody>";
    var i = start + 2;
    for (; i < lines.length; i++) {
      if (/^\s*$/.test(lines[i]) || !/^\s*\|/.test(lines[i])) break;
      var cells = splitTableRow(lines[i]);
      html += "<tr>";
      for (var k = 0; k < header.length; k++) html += cell("td", cells[k], k);
      html += "</tr>";
    }
    html += "</tbody></table></div>";
    return { html: html, next: i };
  }

  function renderListNodes(nodes, codes) {
    var html = "";
    var group = null;
    function flushGroup() {
      if (!group) return;
      var tag = group.ordered ? "ol" : "ul";
      html += "<" + tag + ">";
      group.nodes.forEach(function (n) {
        html += "<li>" + inline(expandTokens(n.text, codes));
        if (n.children.length) html += renderListNodes(n.children, codes);
        html += "</li>";
      });
      html += "</" + tag + ">";
      group = null;
    }
    nodes.forEach(function (n) {
      if (!group || group.ordered !== n.ordered) { flushGroup(); group = { ordered: n.ordered, nodes: [] }; }
      group.nodes.push(n);
    });
    flushGroup();
    return html;
  }

  function renderListBlock(lines, start, codes) {
    var items = [];
    var stack = [];
    var i = start;
    while (i < lines.length) {
      var m = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/.exec(lines[i]);
      if (!m) {
        if (/^\s*$/.test(lines[i])) { i++; continue; } // 松散列表：跳过列表内空行
        break;
      }
      var indent = m[1].replace(/\t/g, "  ").length;
      var node = { indent: indent, ordered: /\d/.test(m[2].charAt(0)), text: m[3], children: [] };
      while (stack.length && stack[stack.length - 1].indent >= indent) stack.pop();
      if (stack.length) stack[stack.length - 1].children.push(node);
      else items.push(node);
      stack.push(node);
      i++;
    }
    return { html: renderListNodes(items, codes), next: i };
  }

  function renderBlocks(lines, codes) {
    var out = [];
    var para = [];
    function flushPara() {
      if (para.length) { out.push("<p>" + para.join("<br>") + "</p>"); para = []; }
    }
    var i = 0;
    while (i < lines.length) {
      var line = lines[i];
      var m;
      var onlyCode = /^\u0000CODE(\d+)\u0000$/.exec(line);
      if (onlyCode) { flushPara(); out.push(codeHtml(codes[+onlyCode[1]])); i++; continue; }
      if (/^\s*$/.test(line)) { flushPara(); i++; continue; }
      // 标题 # ~ ####（##### 起按段落，保持 v1 行为）
      if ((m = /^(#{1,4})\s+(.*)$/.exec(line))) {
        flushPara();
        var lv = m[1].length;
        out.push("<h" + lv + ">" + inline(m[2]) + "</h" + lv + ">");
        i++; continue;
      }
      // 水平线 --- / *** / ___
      if (/^\s*([-*_])(\s*\1){2,}\s*$/.test(line)) { flushPara(); out.push("<hr>"); i++; continue; }
      // 引用块（连续 > 行，剥一层后递归渲染 → 天然支持嵌套引用）
      // 注意：此处文本已整体转义，行首 ">" 表现为 "&gt;"
      if (/^\s*&gt;/.test(line)) {
        flushPara();
        var qlines = [];
        while (i < lines.length && /^\s*&gt;/.test(lines[i])) {
          qlines.push(lines[i].replace(/^\s*&gt;\s?/, ""));
          i++;
        }
        out.push("<blockquote>" + renderBlocks(qlines, codes) + "</blockquote>");
        continue;
      }
      // 表格（| 行 + 分隔行）
      if (/^\s*\|/.test(line) && i + 1 < lines.length && isTableSep(lines[i + 1])) {
        flushPara();
        var tbl = renderTable(lines, i, codes);
        out.push(tbl.html); i = tbl.next; continue;
      }
      // 列表（缩进嵌套）
      if (/^(\s*)([-*+]|\d+[.)])\s+/.test(line)) {
        flushPara();
        var lst = renderListBlock(lines, i, codes);
        out.push(lst.html); i = lst.next; continue;
      }
      // 段落行
      para.push(inline(expandTokens(line, codes)));
      i++;
    }
    flushPara();
    return out.join("");
  }

  function renderMarkdown(src) {
    var text = String(src == null ? "" : src);
    var codes = [];
    function fenceFn(m, lang, code) {
      codes.push({ lang: lang.trim(), code: code.replace(/\n$/, "") });
      return "\u0000CODE" + (codes.length - 1) + "\u0000";
    }
    // 1. 围栏代码块（转义前抽取，保留原文供高亮；流式未闭合围栏也收）
    text = text.replace(/```([^\n`]*)\n?([\s\S]*?)```/g, fenceFn);
    text = text.replace(/```([^\n`]*)\n?([\s\S]*)$/, fenceFn);
    // 2. 剩余文本整体转义（防 XSS）
    text = escapeHtml(text);
    // 3. 块级 + 行内渲染
    return renderBlocks(text.split("\n"), codes);
  }

  /* ------------------------------ 行 diff ------------------------------ */
  /* 简单 LCS（O(n·m) 动态规划）。单侧超过 600 行时降级为"全删全增"，
   * 避免大文件卡死 UI——调用方（工具卡片）此时仍可读两侧原文。
   */
  var DIFF_MAX_LINES = 600;

  function diffLines(aText, bText) {
    var a = String(aText == null ? "" : aText).split("\n");
    var b = String(bText == null ? "" : bText).split("\n");
    if (a.length > DIFF_MAX_LINES || b.length > DIFF_MAX_LINES) {
      var ops = [];
      a.forEach(function (t) { ops.push({ type: "del", text: t }); });
      b.forEach(function (t) { ops.push({ type: "ins", text: t }); });
      return ops;
    }
    var n = a.length, m = b.length;
    var dp = [];
    for (var i = 0; i <= n; i++) dp.push(new Int32Array(m + 1));
    for (i = n - 1; i >= 0; i--) {
      for (var j = m - 1; j >= 0; j--) {
        dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
      }
    }
    var ops = [];
    i = 0; j = 0;
    while (i < n && j < m) {
      if (a[i] === b[j]) { ops.push({ type: "same", text: a[i] }); i++; j++; }
      else if (dp[i + 1][j] >= dp[i][j + 1]) { ops.push({ type: "del", text: a[i] }); i++; }
      else { ops.push({ type: "ins", text: b[j] }); j++; }
    }
    while (i < n) ops.push({ type: "del", text: a[i++] });
    while (j < m) ops.push({ type: "ins", text: b[j++] });
    return ops;
  }

  function diffStats(ops) {
    var s = { added: 0, removed: 0, same: 0 };
    (ops || []).forEach(function (o) {
      if (o.type === "ins") s.added++;
      else if (o.type === "del") s.removed++;
      else s.same++;
    });
    return s;
  }

  function renderLineDiffHtml(aText, bText, maxLines) {
    var ops = diffLines(aText, bText);
    var limit = maxLines || 400;
    var html = "";
    for (var i = 0; i < ops.length; i++) {
      if (i >= limit) {
        html += '<div class="diff-more">… 其余 ' + (ops.length - i) + " 行已省略</div>";
        break;
      }
      var op = ops[i];
      html += '<div class="diff-line diff-' + op.type + '">' + escapeHtml(op.text === "" ? " " : op.text) + "</div>";
    }
    return html;
  }

  /* --------------------------- ws URL 解析 --------------------------- */
  /* 配对二维码内容即完整 ws 链接（serve 启动打印）：接受 ws/wss/http/https，提取 server-key 与 cwd query */
  function decodeSafe(s) {
    try { return decodeURIComponent(s.replace(/\+/g, " ")); } catch (e) { return s; }
  }

  function parseWsUrl(text) {
    var s = String(text || "").trim();
    if (!s) return null;
    var m = /^(ws|wss|http|https):\/\/(.+)$/i.exec(s);
    if (!m) return null;
    var scheme = m[1].toLowerCase();
    var rest = m[2];
    var qi = rest.indexOf("?");
    var path = qi >= 0 ? rest.slice(0, qi) : rest;
    var query = qi >= 0 ? rest.slice(qi + 1) : "";
    var params = {};
    query.split("&").forEach(function (kv) {
      if (!kv) return;
      var eq = kv.indexOf("=");
      var k = decodeSafe(eq >= 0 ? kv.slice(0, eq) : kv);
      var v = decodeSafe(eq >= 0 ? kv.slice(eq + 1) : "");
      params[k] = v;
    });
    var wsScheme = (scheme === "wss" || scheme === "https") ? "wss" : "ws";
    return {
      url: wsScheme + "://" + path,
      secret: params[KEY_PARAM] || params.secret || params.key || "",
      cwd: params.cwd || params.path || params.dir || ""
    };
  }

  function buildWsUrl(url, secret) {
    var base = String(url || "").trim();
    if (!secret) return base;
    var sep = base.indexOf("?") >= 0 ? "&" : "?";
    return base + sep + KEY_PARAM + "=" + encodeURIComponent(secret);
  }

  /* --------------------------- 连接 Profile --------------------------- */

  function makeProfile(name, url, secret, cwd) {
    var now = Date.now();
    return {
      id: "p" + now.toString(36) + Math.random().toString(36).slice(2, 8),
      name: String(name || "").trim() || "未命名连接",
      url: String(url || "").trim(),
      secret: String(secret || "").trim(),
      cwd: String(cwd || "").trim(),
      createdAt: now,
      lastUsedAt: now
    };
  }

  function sanitizeProfile(o) {
    if (!o || typeof o !== "object") return null;
    var url = typeof o.url === "string" ? o.url.trim() : "";
    if (!url) return null; // url 是唯一必填
    var now = Date.now();
    return {
      id: (typeof o.id === "string" && o.id) ? o.id : makeProfile("", url, "", "").id,
      name: (typeof o.name === "string" && o.name.trim()) ? o.name.trim() : "未命名连接",
      url: url,
      secret: typeof o.secret === "string" ? o.secret : "",
      cwd: typeof o.cwd === "string" ? o.cwd : "",
      createdAt: typeof o.createdAt === "number" ? o.createdAt : now,
      lastUsedAt: typeof o.lastUsedAt === "number" ? o.lastUsedAt : now
    };
  }

  function serializeProfiles(list) {
    return JSON.stringify(Array.isArray(list) ? list.slice(0, 50) : []);
  }

  function parseProfiles(text) {
    var arr;
    try { arr = JSON.parse(text); } catch (e) { return []; }
    if (!Array.isArray(arr)) return [];
    var out = [];
    arr.forEach(function (o) { var p = sanitizeProfile(o); if (p) out.push(p); });
    return out.slice(0, 50);
  }

  function upsertProfile(list, profile) {
    var p = sanitizeProfile(profile);
    var base = Array.isArray(list) ? list.slice() : [];
    if (!p) return base;
    var out = base.filter(function (x) { return x.id !== p.id; });
    out.unshift(p);
    return out.slice(0, 50);
  }

  function removeProfileById(list, id) {
    return (Array.isArray(list) ? list : []).filter(function (x) { return x.id !== id; });
  }

  function findProfileById(list, id) {
    var arr = Array.isArray(list) ? list : [];
    for (var i = 0; i < arr.length; i++) if (arr[i].id === id) return arr[i];
    return null;
  }

  /* --------------------------- 最近会话记录 --------------------------- */

  function sanitizeRecent(o) {
    if (!o || typeof o !== "object") return null;
    var sid = typeof o.sid === "string" ? o.sid.trim() : "";
    if (!sid) return null;
    return {
      profileId: typeof o.profileId === "string" ? o.profileId : "",
      sid: sid,
      title: typeof o.title === "string" ? o.title : "",
      ts: typeof o.ts === "number" ? o.ts : Date.now()
    };
  }

  function serializeRecent(list) {
    return JSON.stringify(Array.isArray(list) ? list.slice(0, 20) : []);
  }

  function parseRecent(text) {
    var arr;
    try { arr = JSON.parse(text); } catch (e) { return []; }
    if (!Array.isArray(arr)) return [];
    var out = [];
    arr.forEach(function (o) { var r = sanitizeRecent(o); if (r) out.push(r); });
    out.sort(function (a, b) { return b.ts - a.ts; });
    return out.slice(0, 20);
  }

  function upsertRecent(list, entry) {
    var r = sanitizeRecent(entry);
    var base = Array.isArray(list) ? list.slice() : [];
    if (!r) return base;
    var out = base.filter(function (x) { return x.sid !== r.sid; });
    out.unshift(r);
    out.sort(function (a, b) { return b.ts - a.ts; });
    return out.slice(0, 20);
  }

  function removeRecent(list, sid) {
    return (Array.isArray(list) ? list : []).filter(function (x) { return x.sid !== sid; });
  }

  /* ------------------------------ 主题 ------------------------------ */
  /* stored: "dark" | "light" | "system" | null；systemPrefersLight: 系统是否偏好浅色 */

  function resolveTheme(stored, systemPrefersLight) {
    if (stored === "dark" || stored === "light") return stored;
    return systemPrefersLight ? "light" : "dark"; // "system"/无记录 → 跟随系统，默认深色
  }

  return {
    APP_VERSION: APP_VERSION,
    LS_KEYS: LS_KEYS,
    KEY_PARAM: KEY_PARAM,
    escapeHtml: escapeHtml,
    prettyInput: prettyInput,
    cwdTail: cwdTail,
    normalizeLang: normalizeLang,
    highlightCode: highlightCode,
    renderMarkdown: renderMarkdown,
    diffLines: diffLines,
    diffStats: diffStats,
    renderLineDiffHtml: renderLineDiffHtml,
    parseWsUrl: parseWsUrl,
    buildWsUrl: buildWsUrl,
    makeProfile: makeProfile,
    sanitizeProfile: sanitizeProfile,
    serializeProfiles: serializeProfiles,
    parseProfiles: parseProfiles,
    upsertProfile: upsertProfile,
    removeProfileById: removeProfileById,
    findProfileById: findProfileById,
    sanitizeRecent: sanitizeRecent,
    serializeRecent: serializeRecent,
    parseRecent: parseRecent,
    upsertRecent: upsertRecent,
    removeRecent: removeRecent,
    resolveTheme: resolveTheme
  };
})();

/* UMD 双导出：node --test 直测；浏览器挂 window 供调试 */
if (typeof module !== "undefined" && module.exports) module.exports = QidiMobileCore;
if (typeof window !== "undefined") window.QidiMobileCore = QidiMobileCore;

/* ==================================================================== */
/* ==================== Part 2: DOM 应用（仅浏览器） ==================== */
/* ==================================================================== */
if (typeof document !== "undefined") (function () {
  "use strict";

  var Core = QidiMobileCore;
  var LS = Core.LS_KEYS;

  /* ================================ 常量 ================================ */
  var PROTOCOL_VERSION = 1;                // ACP v1（必须是数字；字符串会被服务端解析成 V0）
  var RECONNECT_DELAYS = [1, 2, 4, 8, 16]; // 秒（P9 退避）
  var MAX_RECONNECT = 5;
  var INIT_TIMEOUT_MS = 15000;             // P11：initialize 15s 无响应即断开走重连

  /* ============================== 小工具 =============================== */
  function $(id) { return document.getElementById(id); }
  function on(el, ev, fn) { if (el) el.addEventListener(ev, fn); }
  function store(k, v) { try { localStorage.setItem(k, v); } catch (e) { /* 隐私模式等，忽略 */ } }
  function load(k) { try { return localStorage.getItem(k); } catch (e) { return null; } }
  function drop(k) { try { localStorage.removeItem(k); } catch (e) { /* ignore */ } }
  function esc(s) { return Core.escapeHtml(s); }
  function msgRoot() { return $("msg-flow"); }

  function el(tag, cls, text) {
    var n = document.createElement(tag);
    if (cls) n.className = cls;
    if (text != null) n.textContent = text; // textContent 天然转义
    return n;
  }

  function delay(ms) { return new Promise(function (r) { setTimeout(r, ms); }); }

  function fmtDur(ms) { return ms < 1000 ? ms + "ms" : (ms / 1000).toFixed(1) + "s"; }

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
  var toolTimers = {};             // card.key -> interval（运行中工具卡的耗时 tick）
  var planCard = null;
  var permQueue = [];              // 并发权限请求 FIFO
  var currentPerm = null;
  var toastTimer = null;
  // 新增：连接 Profile / 最近会话 / 主题 / 抽屉
  var profiles = [];               // Core.parseProfiles(localStorage)
  var recent = [];                 // Core.parseRecent(localStorage)
  var activeProfileId = "";
  var editingProfileId = null;     // 非空 = 表单处于编辑某 profile 模式
  var themeMedia = null;           // matchMedia("(prefers-color-scheme: light)")

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
    var full = Core.buildWsUrl(conn.url, conn.secret);
    var socket;
    try {
      socket = new WebSocket(full);
    } catch (e) {
      if (isReconnect) startReconnect();
      else { showConnectError("无法创建 WebSocket 连接：" + e.message); setConnLight("off"); }
      return;
    }
    ws = socket;
    setConnLight("wait"); // 连接中/重连中 = 黄
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
      setConnLight("off");
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
    setConnLight("on"); // 绿灯
    resetTurnRefs();
    updateHeader();
    setTurnState("idle");
    showView("session");
    if (fresh) {
      // 全新会话：消息区属于另一个会话，清空
      clearMessages();
      setHeaderTitle("");
      showEmptyState();
      if (isReconnect) addSystem("已重新连接（服务端已无可恢复的历史，已新建会话）");
    }
    // 非 fresh（session/load 恢复）时：replay 历史已流式渲染进消息区，不得清空
    recordRecentSession(sessionId, "");
  }

  function onHandshakeFail(err, isReconnect) {
    clearInitTimer();
    setConnLight("off");
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
    if (intentional) setConnLight("off");
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
    setConnLight("wait");
    startReconnect();
  }

  // P9 重连：退避 [1,2,4,8,16] 秒，失败达 5 次转手动重试
  function startReconnect() {
    if (intentionalClose) return;
    clearPermQueue(); // 断线时清空权限弹窗队列（P7）
    reconnectCount += 1;
    if (reconnectCount > MAX_RECONNECT) {
      setConnLight("off");
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
    if (info && typeof info === "object" && info.title != null) {
      setHeaderTitle(String(info.title));
      updateRecentTitle(String(info.title));
    }
  }

  function onMessageChunk(update) {
    var t = textOf(update.content);
    if (t === null) return; // 非 text 忽略
    if (!curAssistantEl) curAssistantEl = appendAssistantBubble();
    curAssistantBuf += t;
    curAssistantEl.innerHTML = Core.renderMarkdown(curAssistantBuf);
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
      msgRoot().appendChild(wrapEl);
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
    var label = $("send-label");
    var ta = $("input");
    if (state === "running") {
      if (label) label.textContent = "停止"; // 只改文字，保留按钮内 SVG 图标
      btn.classList.add("cancel"); btn.classList.remove("send");
      ta.disabled = true;
    } else {
      if (label) label.textContent = "发送";
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
    clearToolTimers();
  }

  function clearToolTimers() {
    Object.keys(toolTimers).forEach(function (k) { clearInterval(toolTimers[k]); delete toolTimers[k]; });
  }

  /* ============================ 工具 / 计划卡片 ============================ */
  function setToolStatus(card, status) {
    card.el.classList.remove("status-pending", "status-in_progress", "status-completed", "status-failed");
    card.el.classList.add("status-" + (status || "pending"));
    if (status === "in_progress") {
      if (card.startedAt == null) card.startedAt = Date.now();
      if (!toolTimers[card.key]) {
        toolTimers[card.key] = setInterval(function () {
          if (card.durEl && card.startedAt != null && card.endedAt == null) {
            card.durEl.textContent = fmtDur(Date.now() - card.startedAt);
          }
        }, 500);
      }
    } else if (status === "completed" || status === "failed") {
      if (card.endedAt == null && card.startedAt != null) {
        card.endedAt = Date.now();
        if (card.durEl) card.durEl.textContent = fmtDur(card.endedAt - card.startedAt);
      }
      if (toolTimers[card.key]) { clearInterval(toolTimers[card.key]); delete toolTimers[card.key]; }
    }
  }

  // P6 tool_call：kind 是 snake_case 字符串；rawInput / content 都在 update 顶层，全部容错
  function createToolCard(update) {
    hideEmptyState();
    var card = el("div", "tool-card");
    var head = el("div", "tool-head");
    var icon = el("span", "tool-icon"); // 状态图标（CSS 按 status-* 出字形/转圈）
    var titleEl = el("span", "tool-title",
      (update.title != null && update.title !== "") ? String(update.title) : "工具调用");
    var kindEl = el("span", "tool-kind", kindText(update.kind));
    var durEl = el("span", "tool-dur", ""); // 耗时（in_progress 起实时，结束定稿）
    head.appendChild(icon);
    head.appendChild(titleEl);
    head.appendChild(kindEl);
    head.appendChild(durEl);
    card.appendChild(head);

    var contentEl = el("div", "tool-content");
    contentEl.style.display = "none";
    card.appendChild(contentEl);

    var rawBox = el("div", "tool-sections");
    card.appendChild(rawBox);

    var obj = {
      id: update.toolCallId, el: card, titleEl: titleEl, kindEl: kindEl,
      contentEl: contentEl, rawBox: rawBox, durEl: durEl,
      rawInputEl: null, rawOutputEl: null, diffEl: null,
      rawInputText: null, rawOutputText: null,
      startedAt: Date.now(), endedAt: null, key: null
    };
    if (Array.isArray(update.content)) setToolContent(obj, update.content);
    if ("rawInput" in update) setToolRaw(obj, "input", update.rawInput);
    if ("rawOutput" in update) setToolRaw(obj, "output", update.rawOutput);
    setToolStatus(obj, (typeof update.status === "string" && update.status) ? update.status : "pending");

    msgRoot().appendChild(card);
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
    var text = Core.prettyInput(raw);
    var node = buildToolSection(isOut ? "输出" : "输入", text, null);
    if (card[key]) card.rawBox.replaceChild(node, card[key]);
    else if (!isOut && card.rawOutputEl) card.rawBox.insertBefore(node, card.rawOutputEl);
    else card.rawBox.appendChild(node);
    card[key] = node;
    if (isOut) card.rawOutputText = text; else card.rawInputText = text;
    refreshToolDiff(card);
  }

  // 输入/输出均为文本时，生成轻量 LCS 行 diff 区块（单侧 >600 行时 Core 内部降级）
  function refreshToolDiff(card) {
    if (card.diffEl && card.diffEl.parentNode) card.diffEl.parentNode.removeChild(card.diffEl);
    card.diffEl = null;
    if (card.rawInputText == null || card.rawOutputText == null) return;
    var st = Core.diffStats(Core.diffLines(card.rawInputText, card.rawOutputText));
    if (st.added + st.removed === 0) return; // 无差异不显示
    var node = buildToolSection("差异 +" + st.added + " −" + st.removed, null,
      Core.renderLineDiffHtml(card.rawInputText, card.rawOutputText));
    card.rawBox.appendChild(node);
    card.diffEl = node;
  }

  function buildToolSection(label, text, diffHtml) {
    var wrapEl = el("div", "tool-sec");
    var btn = el("button", "sec-toggle", label);
    btn.type = "button";
    var body = el("div", diffHtml ? "sec-body diff-body" : "sec-body collapsed");
    if (diffHtml) body.innerHTML = diffHtml; // Core 产物已全量转义
    else body.textContent = text;             // textContent 天然转义
    on(btn, "click", function () {
      body.classList.toggle("collapsed");
      btn.classList.toggle("open");
    });
    wrapEl.appendChild(btn);
    wrapEl.appendChild(body);
    return wrapEl;
  }

  function createPlanCard() {
    hideEmptyState();
    var card = el("div", "plan-card");
    card.appendChild(el("div", "plan-head", "执行计划"));
    var body = el("div", "plan-body tl-steps");
    card.appendChild(body);
    msgRoot().appendChild(card);
    return { el: card, body: body };
  }

  // P6 plan：entries 是 [{content, priority, status}]，全量替换重绘（时间线样式）
  function renderPlan(card, entries) {
    card.body.innerHTML = "";
    entries.forEach(function (item) {
      var st = (typeof item.status === "string" && item.status) ? item.status : "pending";
      var row = el("div", "tl-step status-" + st);
      row.appendChild(el("span", "tl-dot"));
      var main = el("div", "tl-main");
      main.appendChild(el("div", "tl-name", (item && item.content != null) ? String(item.content) : ""));
      if (item && item.priority != null) main.appendChild(el("span", "tl-priority", String(item.priority)));
      row.appendChild(main);
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
    if (rawInput !== undefined && rawInput !== null) { pre.textContent = Core.prettyInput(rawInput); pre.style.display = ""; }
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

  /* ======================= 连接 Profile / 最近会话 ======================= */
  function persistProfiles() { store(LS.profiles, Core.serializeProfiles(profiles)); }
  function persistRecent() { store(LS.recent, Core.serializeRecent(recent)); }

  function renderProfiles() {
    var box = $("profile-items");
    if (!box) return;
    box.innerHTML = "";
    profiles.forEach(function (p) {
      var row = el("div", "profile-item");
      var main = el("button", "profile-main");
      main.type = "button";
      var name = el("div", "profile-name", p.name + (p.id === activeProfileId ? " · 当前" : ""));
      var meta = el("div", "profile-meta", p.url);
      var cwd = el("div", "profile-cwd", "cwd: " + Core.cwdTail(p.cwd));
      main.appendChild(name); main.appendChild(meta); main.appendChild(cwd);
      on(main, "click", function () { beginConnect(p); });
      var edit = el("button", "icon-btn profile-edit", "✎");
      edit.type = "button";
      edit.setAttribute("aria-label", "编辑 " + p.name);
      on(edit, "click", function (e) { e.stopPropagation(); startEditProfile(p); });
      var del = el("button", "icon-btn profile-del", "🗑");
      del.type = "button";
      del.setAttribute("aria-label", "删除 " + p.name);
      on(del, "click", function (e) {
        e.stopPropagation();
        if (!confirm("删除连接配置「" + p.name + "」？")) return;
        profiles = Core.removeProfileById(profiles, p.id);
        persistProfiles();
        if (editingProfileId === p.id) cancelEditProfile();
        if (activeProfileId === p.id) { activeProfileId = ""; drop(LS.activeProfile); }
        renderProfiles();
        toast("已删除");
      });
      row.appendChild(main); row.appendChild(edit); row.appendChild(del);
      box.appendChild(row);
    });
    $("profile-empty").classList.toggle("hidden", profiles.length > 0);
  }

  function renderRecent() {
    var box = $("recent-items");
    if (!box) return;
    box.innerHTML = "";
    recent.forEach(function (r) {
      var row = el("button", "recent-item");
      row.type = "button";
      row.appendChild(el("div", "recent-title", r.title || ("会话 " + r.sid.slice(0, 8))));
      row.appendChild(el("div", "recent-meta", new Date(r.ts).toLocaleString() + " · " + r.sid.slice(0, 8)));
      on(row, "click", function () { onRecentClick(r); });
      box.appendChild(row);
    });
    $("recent-empty").classList.toggle("hidden", recent.length > 0);
    $("recent-panel").classList.toggle("hidden", recent.length === 0);
  }

  function renderDrawerRecent() {
    var box = $("drawer-recent-items");
    if (!box) return;
    box.innerHTML = "";
    recent.slice(0, 8).forEach(function (r) {
      var row = el("button", "drawer-recent-item");
      row.type = "button";
      row.appendChild(el("div", "recent-title", r.title || ("会话 " + r.sid.slice(0, 8))));
      row.appendChild(el("div", "recent-meta", r.sid.slice(0, 8)));
      on(row, "click", function () { closeDrawer(); onRecentClick(r); });
      box.appendChild(row);
    });
    var empty = $("drawer-recent-empty");
    if (empty) empty.classList.toggle("hidden", recent.length > 0);
  }

  function recordRecentSession(sid, title) {
    if (!sid) return;
    recent = Core.upsertRecent(recent, {
      profileId: activeProfileId || "", sid: String(sid), title: title || "", ts: Date.now()
    });
    persistRecent();
    renderRecent();
    renderDrawerRecent();
  }

  function updateRecentTitle(title) {
    if (!sessionId || !title) return;
    var changed = false;
    recent = recent.map(function (r) {
      if (r.sid === sessionId && r.title !== title) { changed = true; return { profileId: r.profileId, sid: r.sid, title: title, ts: r.ts }; }
      return r;
    });
    if (!changed) return;
    persistRecent();
    renderRecent();
    renderDrawerRecent();
  }

  function onRecentClick(entry) {
    var p = Core.findProfileById(profiles, entry.profileId);
    if (!p) { toast("该连接配置已不存在，请重新填写连接信息"); return; }
    store(LS.sessionId, entry.sid); // handshake 会优先尝试 session/load 恢复它
    beginConnect(p);
  }

  // 统一连接入口：profile → upsert/记忆 → 置 conn → startSocket
  function beginConnect(profile) {
    var p = Core.sanitizeProfile(profile);
    if (!p) { showConnectError("连接配置无效（缺少服务器地址）"); return; }
    p.lastUsedAt = Date.now();
    profiles = Core.upsertProfile(profiles, p);
    persistProfiles();
    activeProfileId = p.id;
    store(LS.activeProfile, p.id);
    // v1 兼容键继续写（重连恢复/预填用）
    store(LS.url, p.url); store(LS.secret, p.secret); store(LS.cwd, p.cwd);
    conn = { url: p.url, secret: p.secret, cwd: p.cwd };
    renderProfiles();
    hideConnectError();
    clearPermQueue();
    setHeaderTitle("");
    everConnected = false;
    reconnectCount = 0;
    closeDrawer();
    startSocket(false);
  }

  function fillForm(p) {
    $("in-name").value = p.name || "";
    $("in-url").value = p.url || "";
    $("in-secret").value = p.secret || "";
    $("in-cwd").value = p.cwd || "";
  }

  function startEditProfile(p) {
    editingProfileId = p.id;
    fillForm(p);
    $("form-title").textContent = "编辑连接配置";
    $("btn-cancel-edit").classList.remove("hidden");
    $("in-url").focus();
    $("in-url").scrollIntoView({ behavior: "smooth", block: "center" });
  }

  function cancelEditProfile() {
    editingProfileId = null;
    $("form-title").textContent = "连接信息";
    $("btn-cancel-edit").classList.add("hidden");
  }

  function onConnect() {
    var url = $("in-url").value.trim();
    var secret = $("in-secret").value.trim();
    var cwd = $("in-cwd").value.trim();
    var name = $("in-name").value.trim();
    if (!url || !secret || !cwd) {
      showConnectError("请完整填写服务器地址、访问密钥和项目路径。");
      return;
    }
    var profile;
    if (editingProfileId) {
      var old = Core.findProfileById(profiles, editingProfileId);
      profile = Core.makeProfile(name || (old && old.name) || "", url, secret, cwd);
      profile.id = editingProfileId;
      if (old) profile.createdAt = old.createdAt;
    } else {
      profile = Core.makeProfile(name, url, secret, cwd);
    }
    cancelEditProfile();
    beginConnect(profile);
  }

  function saveProfileFromForm() {
    var url = $("in-url").value.trim();
    var secret = $("in-secret").value.trim();
    var cwd = $("in-cwd").value.trim();
    var name = $("in-name").value.trim();
    if (!url) { showConnectError("至少填写服务器地址才能保存连接配置。"); return; }
    var profile;
    if (editingProfileId) {
      var old = Core.findProfileById(profiles, editingProfileId);
      profile = Core.makeProfile(name || (old && old.name) || "", url, secret, cwd);
      profile.id = editingProfileId;
      if (old) profile.createdAt = old.createdAt;
    } else {
      profile = Core.makeProfile(name, url, secret, cwd);
    }
    profiles = Core.upsertProfile(profiles, profile);
    persistProfiles();
    renderProfiles();
    cancelEditProfile();
    hideConnectError();
    toast("已保存连接配置");
  }

  /* ============================ 扫码（粘贴降级） ============================ */
  function openScanSheet() {
    $("scan-error").classList.add("hidden");
    $("scan-input").value = "";
    $("scan-sheet").classList.remove("hidden");
    $("scan-input").focus();
  }
  function closeScanSheet() { $("scan-sheet").classList.add("hidden"); }
  function onScanParse() {
    var parsed = Core.parseWsUrl($("scan-input").value);
    if (!parsed || !parsed.url) {
      var e2 = $("scan-error");
      e2.textContent = "无法解析：请粘贴 ws:// 或 wss:// 开头的完整链接（可带 ?server-key=…&cwd=…）";
      e2.classList.remove("hidden");
      return;
    }
    if (parsed.url) $("in-url").value = parsed.url;
    if (parsed.secret) $("in-secret").value = parsed.secret;
    if (parsed.cwd) $("in-cwd").value = parsed.cwd;
    closeScanSheet();
    toast("已解析并填入连接信息");
  }

  /* ================================ 抽屉 ================================ */
  function openDrawer() {
    updateHeader(); // 刷新抽屉里的会话信息
    renderDrawerRecent();
    syncThemeGroup();
    $("drawer").classList.remove("hidden");
  }
  function closeDrawer() { $("drawer").classList.add("hidden"); }

  /* ================================ 主题 ================================ */
  function storedTheme() {
    var t = load(LS.theme);
    return (t === "dark" || t === "light" || t === "system") ? t : "system";
  }
  function systemPrefersLight() {
    return typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-color-scheme: light)").matches;
  }
  function applyTheme() {
    var eff = Core.resolveTheme(storedTheme(), systemPrefersLight());
    document.documentElement.dataset.theme = eff;
    var meta = document.querySelector('meta[name="theme-color"]:not([media])');
    if (meta) meta.setAttribute("content", eff === "light" ? "#f5f6f8" : "#101014");
    syncThemeGroup();
  }
  function syncThemeGroup() {
    var cur = storedTheme();
    var group = $("theme-group");
    if (!group) return;
    Array.prototype.forEach.call(group.querySelectorAll("button"), function (b) {
      b.classList.toggle("on", b.getAttribute("data-theme-val") === cur);
    });
  }
  function onThemeBtn(val) {
    if (val === "system") drop(LS.theme);
    else store(LS.theme, val);
    applyTheme();
  }
  function initTheme() {
    applyTheme();
    if (typeof window.matchMedia === "function") {
      themeMedia = window.matchMedia("(prefers-color-scheme: light)");
      var onChange = function () { if (storedTheme() === "system") applyTheme(); };
      if (themeMedia.addEventListener) themeMedia.addEventListener("change", onChange);
      else if (themeMedia.addListener) themeMedia.addListener(onChange); // 旧 WebView
    }
  }

  /* ============================ 占位功能（待后端） ============================ */
  function placeholderTap(what) {
    toast(what + "：待后端支持（后端就绪后开放）");
  }

  /* ============================ PWA 注册（协议探测） ============================ */
  function registerSW() {
    try {
      // file:// 与其它协议下静默跳过；仅 http/https 注册，SW 只缓存静态壳
      if (location.protocol !== "http:" && location.protocol !== "https:") return;
      if (!("serviceWorker" in navigator)) return;
      navigator.serviceWorker.register("sw.js").catch(function (e) {
        console.debug("[qidi] Service Worker 注册失败（忽略，不影响使用）：", e);
      });
    } catch (e) { /* ignore */ }
  }

  /* ======================= 输入法弹起适配（visualViewport） ======================= */
  function initViewport() {
    var vv = window.visualViewport;
    if (!vv) return;
    var apply = function () {
      document.documentElement.style.setProperty("--app-h", vv.height + "px");
    };
    vv.addEventListener("resize", apply);
    vv.addEventListener("scroll", apply);
    apply();
  }

  /* ============================ DOM 渲染工具 ============================ */
  function appendAssistantBubble() {
    hideEmptyState();
    var b = el("div", "msg assistant");
    msgRoot().appendChild(b);
    return b;
  }
  function appendUserBubble(text) {
    hideEmptyState();
    var b = el("div", "msg user", text); // 纯文本
    msgRoot().appendChild(b);
    scrollBottom();
  }
  function addSystem(text) {
    hideEmptyState();
    var b = el("div", "msg system", text);
    msgRoot().appendChild(b);
    scrollBottom();
  }
  function clearMessages() { msgRoot().innerHTML = ""; }
  function scrollBottom() { var m = $("messages"); m.scrollTop = m.scrollHeight; }

  function showEmptyState() { var n = $("empty-state"); if (n) n.classList.remove("hidden"); }
  function hideEmptyState() { var n = $("empty-state"); if (n) n.classList.add("hidden"); }

  function updateHeader() {
    $("head-cwd").textContent = Core.cwdTail(conn ? conn.cwd : "");
    $("head-sid").textContent = sessionId ? String(sessionId).slice(0, 8) : "—";
    var dw = $("drawer-title");
    if (dw) dw.textContent = Core.cwdTail(conn ? conn.cwd : "");
    var dc = $("drawer-cwd");
    if (dc) dc.textContent = conn ? conn.cwd : "";
    var ds = $("drawer-sid");
    if (ds) ds.textContent = sessionId ? String(sessionId).slice(0, 8) : "—";
  }
  // P6 session_info_update：update.sessionInfo.title
  function setHeaderTitle(t) {
    var n = $("head-title");
    if (!n) return;
    n.textContent = t || Core.cwdTail(conn ? conn.cwd : "") || "会话";
    var dw = $("drawer-title");
    if (dw && t) dw.textContent = t;
  }

  // 连接状态灯：on=绿（已连接） / wait=黄（连接中/重连中，脉冲） / off=红（断开）
  function setConnLight(state) {
    var n = $("conn-light");
    if (!n) return;
    n.classList.remove("light-on", "light-wait", "light-off");
    n.classList.add("light-" + (state || "off"));
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

  /* ============================ 菜单动作（P8） ============================ */
  function onNewSession() {
    closeDrawer();
    var oldId = sessionId;
    var go = function () {
      resetTurnRefs();
      clearMessages();
      setHeaderTitle("");
      showEmptyState();
      request("session/new", { cwd: conn.cwd, mcpServers: [] })
        .then(function (res) {
          if (!res || !res.sessionId) { toast("新建会话失败：服务端未返回 sessionId"); return; }
          sessionId = res.sessionId;
          store(LS.sessionId, sessionId);
          updateHeader();
          setTurnState("idle");
          recordRecentSession(sessionId, "");
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
    closeDrawer();
    if (turnState === "running") { toast("当前回合进行中，请先取消或等待完成"); return; }
    var finish = function () { closeSocket(true); showView("connect"); };
    if (sessionId) {
      var rel = request("session/close", { sessionId: sessionId }).catch(function () { /* 忽略 */ });
      Promise.race([rel, delay(1500)]).then(finish);
    } else {
      finish();
    }
  }

  /* ============================ 清空本地数据 / 关于 ============================ */
  function onClearData() {
    if (!confirm("确定清空所有本地数据？\n（连接配置 / 最近会话 / 主题选择 / 会话 id，不可恢复）")) return;
    Object.keys(LS).forEach(function (k) { drop(LS[k]); });
    profiles = []; recent = []; activeProfileId = ""; editingProfileId = null;
    persistProfiles(); persistRecent();
    renderProfiles(); renderRecent(); renderDrawerRecent();
    cancelEditProfile();
    applyTheme(); // 主题回到"跟随系统"
    toast("已清空本地数据");
  }

  function onAbout() {
    var box = $("about-box");
    if (box) box.classList.toggle("hidden");
  }

  /* ================================ 初始化 ================================ */
  function init() {
    initTheme();
    initViewport();
    registerSW();

    // 载入本地数据
    profiles = Core.parseProfiles(load(LS.profiles));
    recent = Core.parseRecent(load(LS.recent));
    activeProfileId = load(LS.activeProfile) || "";

    // v1 兼容：旧键预填表单
    var savedUrl = load(LS.url);
    $("in-url").value = (savedUrl != null && savedUrl !== "") ? savedUrl : defaultUrl();
    $("in-secret").value = load(LS.secret) || "";
    $("in-cwd").value = load(LS.cwd) || "";
    // 输入即存（v1 行为保留）
    on($("in-url"), "input", function () { store(LS.url, this.value); });
    on($("in-secret"), "input", function () { store(LS.secret, this.value); });
    on($("in-cwd"), "input", function () { store(LS.cwd, this.value); });

    on($("btn-connect"), "click", onConnect);
    on($("btn-save-profile"), "click", saveProfileFromForm);
    on($("btn-cancel-edit"), "click", cancelEditProfile);
    on($("btn-profile-add"), "click", function () {
      cancelEditProfile();
      fillForm({ name: "", url: "", secret: "", cwd: "" });
      $("in-url").focus();
      $("in-url").scrollIntoView({ behavior: "smooth", block: "center" });
    });

    // 扫码（粘贴降级）
    on($("btn-scan"), "click", openScanSheet);
    on($("scan-close"), "click", closeScanSheet);
    on($("scan-backdrop"), "click", closeScanSheet);
    on($("btn-scan-parse"), "click", onScanParse);

    // 发送 / composer
    on($("btn-send"), "click", onSendClick);
    on($("input"), "input", autoGrow);
    on($("input"), "keydown", function (e) {
      if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) { e.preventDefault(); onSendClick(); }
    });
    on($("input"), "focus", function () { setTimeout(scrollBottom, 300); });
    // 占位功能（待后端）：可见入口 + 点击明示
    on($("btn-at"), "click", function () { placeholderTap("@ 文件引用"); });
    on($("btn-slash"), "click", function () { placeholderTap("/ 斜杠命令"); });
    on($("btn-image"), "click", function () { placeholderTap("图片发送"); });

    // 抽屉
    on($("btn-menu"), "click", openDrawer);
    on($("btn-close-drawer"), "click", closeDrawer);
    on($("drawer-backdrop"), "click", closeDrawer);
    on($("btn-new-session"), "click", onNewSession);
    on($("btn-disconnect"), "click", onDisconnect);
    on($("drawer-model"), "click", function () { placeholderTap("模型切换"); });
    on($("drawer-commands"), "click", function () { placeholderTap("斜杠命令面板"); });
    on($("btn-clear-data"), "click", onClearData);
    on($("btn-about"), "click", onAbout);
    on($("theme-group"), "click", function (e) {
      var b = e.target;
      while (b && b !== this && !(b.getAttribute && b.getAttribute("data-theme-val"))) b = b.parentNode;
      if (b && b.getAttribute && b.getAttribute("data-theme-val")) onThemeBtn(b.getAttribute("data-theme-val"));
    });

    on($("btn-retry"), "click", function () { reconnectCount = 0; hideBanner(); clearPermQueue(); startSocket(true); });
    on($("perm-close"), "click", cancelPermission);
    on($("perm-cancel"), "click", cancelPermission);
    on($("perm-backdrop"), "click", cancelPermission);

    // 版本号
    var ver = $("app-version");
    if (ver) ver.textContent = "v" + Core.APP_VERSION;

    renderProfiles();
    renderRecent();
    renderDrawerRecent();
    autoGrow();
    setConnLight("off");
    showView("connect");
  }

  // 地址预填守卫：仅 location.protocol === "http:" 时预填 ws://<location.host>/ws；
  // https: 与 file: 一律预填空串（无法据此推断出正确的 ws:// 后端地址）。
  function defaultUrl() {
    if (location.protocol === "http:") return "ws://" + location.host + "/ws";
    return "";
  }

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", init);
  else init();
})();
