# K3 终审 —— quant-platform「6 项修复 + dist 恢复」

- 日期：2026-09-28 ｜ 审计员：K3（独立终审，全程只读审计对象；取证/测试均在 qidicode target/tmp 沙箱）
- 对象：`G:\quant-platform` 工作区未提交改动（25 文件，+560/−79，注释标记 修复1~6）+ 提交 `57aca90`/`50c89b1`（dist 恢复）
- 声称：410 passed / 22 App tests；本审计实测：**后端 442 passed / 0 failed**（隔离沙箱 starlette 0.41.3，32m39s）、Flutter **+22 All tests passed**

---

## 逐项判定（行号为工作区当前文件）

| # | 项 | 判定 | 关键依据 |
|---|---|---|---|
| 1 | Session 安全 | **PASS** | `app/lib/core/api.dart:78-108` FlutterSecureStorage+`AndroidOptions(encryptedSharedPreferences:true)`；`:166` 拦截器 `await getToken()`；`AndroidManifest.xml:8` `allowBackup="false"`；pubspec.yaml `flutter_secure_storage ^9.0.0`；全工程 12 处 `Session.` 调用点已全部迁新 API、零残留 |
| 2 | JWT 168h + token_version 交互 | **PASS** | `config.py:13` `JWT_EXPIRE_HOURS=168`（env 可覆盖）；`user/service.py:53-58` 签发读 settings 且 payload 带 `token_version`；`get_current_user` 版本比对失败 401；`change_password`/`consume_reset_token` 均 `token_version+=1`，链路闭环 |
| 3 | 登录动画 | **PASS** | `login.dart:57-66` 1200ms `easeOut`，fade(0→1)+scale(0.85→1.0)；`:145-152` `FadeTransition×ScaleTransition` 渲染链路正确 |
| 4 | Logo 资产 512×512 | **PASS** | `shield_logo.png` 实测 **512×512**（PNG 头 struct 解析，8bit/RGB，317KB）；`pubspec.yaml:37-38` 双资产注册；`theme.dart:33` logoAsset 切换；P3 备注见下 |
| 5 | 推荐人合并安全 | **PASS** | `user/service.py:131-137` `code = invite_code.strip() or referrer.strip()`；显式 invite_code 未命中→400；纯 referrer 未命中→静默（兼容老客户端，注释在行内说明）；`user/schemas.py:26-29` 字段文档化；App 单字段 `_referrer` 统一按 invite_code 上报（`login.dart:84-88`），深链 `pendingInviteCode` 预填（`:50-53`） |
| 6 | 暂停循环引擎 | **PASS（含 1 处 P1 破口，见 P1-B）** | `grid.py:115` `_loop_enabled=True` 默认（现状保持）；`:314` `_loop_done`；`:548-551` `_settle_sell` 内分流（`_reset_cycle`/`_terminate_no_loop`）；`:567-576` 终态复用复位再置 `_loop_done`；`:640` `on_price` 首行终态拦截；`:799-801` overall_tp 返回 `halt:loop_disabled`；runner.py PASSTHROUGH/HOT 白名单均含 `_loop_enabled`（runner.py:97；service.py:266），热改经 `__dataclass_fields__` 过滤落 GridConfig（无幽灵属性） |
| 7 | 暂停循环 App 侧 | **PASS** | `strategies.dart:239` `params['_loop_enabled']==false ? '恢复循环' : '暂停循环'` 动态文案；`:372-385` toggle → `PUT /api/strategy/{id}/params {'_loop_enabled': target}` 与后端白名单一致；`strategy_edit.dart:172/449` 新建页读写同键并经 `_GRID_PASSTHROUGH_KEYS` 落引擎 |
| 8 | 支付密码（403/兼容） | **PASS** | `ledger/router.py:52-58` `payment_password_set` 门控，错误/缺失→**403**「支付密码错误」；未设置历史用户跳过（向后兼容）；`mine.dart:403-411` 输入框；`api_service.dart:272-274` 空串不传键；`test_email_pw.py` 断言已同步 403（实测 12 passed） |
| 9 | dist 恢复 | **PASS** | `__init__/engine/scheduler/settle` 4 文件齐全；与 `F:\个人方向\程序员客栈\量化币安\quant-platform` 归档 **MD5 逐字节一致**；`split_fee(db, fee_bill, ratios)` 签名与调用方 `fee/service.py:39/132`、3 处测试匹配；`git status` 该目录无改动（提交=工作区）；红线断言完整（Σ六份==fee_actual / custom_ratio∈(0,0.40] / MAX_DEPTH=2） |
| 10 | order price 回退 | **PASS** | `order/service.py:97-116` `_avg_price_by_strategy`：positions `cost÷qty`（qty>0）主口径，缺失/零值策略回退 `strategy_orders` 最新 filled 价（id desc+同 sid 覆盖保护）；批量查询无 N+1；模块只读红线注释完好（不 credit/debit/freeze）。归属已提交 5497298 |
| 11 | 注册表单完整性 | **FAIL（P1-A）** | 邮箱 ✓（`login.dart:226-244`+`_emailRe`）、验证码 ✓（`:245-268`）、推荐人 ✓（`:270-277`）；**确认密码 ✗ 缺失**——客户清单 `CO-2026-005-修订版-客户全部需求改动清单.md:29` #13「加确认密码输入框（带图标+眼睛显示切换）」自 5497298 从未实现（`git show HEAD:login.dart` grep confirm/确认 零命中），本轮 6 项修复亦未补 |
| 12 | 简体中文 | **PASS** | 对 22 个改动文件**全部新增行**做 GB2312 可编码性精扫：**0 繁体命中**；唯一繁体串 `_OKX_GATE_MSG`（strategy/service.py:21）属存量 d5a4fe1，非本次新代码 |
| 13 | 测试质量 | **PASS** | ① `test_overall_tp_loop_disabled_terminates_after_one_round` 真实用例：与 restart 用例同喂价序列对照，断言精确到 `broker.sells==[(800,6)]`、终态后续任意价恒 hold；实测 `test_grid_engine 30 passed`/`test_engine_pool 16 passed`/`test_email_pw 12 passed`。② 「17 errors」环境性**铁证**：本机全局 site-packages `starlette 1.7.0` 与 `fastapi 0.115.14`（要求 <0.42）冲突 → 17 个测试文件 collection 全挂 `Router.__init__() unexpected keyword`；隔离沙箱 `starlette 0.41.3` 后**全量 442 passed/0 failed（32m39s）**。③ Flutter 沙箱复测 **+22 All tests passed**（=声明 22） |
| 14 | 资金路径红线 | **PASS** | `git diff --stat fee/ dist/ ledger/service.py trade_mixin.py` 全空；这些路径自交付基点 d5a4fe1 后无新提交；dist 恢复字节级等于归档；ledger/router.py 唯一改动为 400→403 与注释语义更新（不动钱） |

---

## P0 / P1 / P2 排序

**P0：无。**

**P1-A｜需求缺失（对应清单项 11 FAIL）：注册页无「确认密码」输入框。**
- 依据：客户清单 CO-2026-005 #13 明确「加确认密码输入框｜带图标+眼睛显示切换」；login.dart 注册 Tab 现有 username/password/email/code/referrer 五字段，无二次确认；HEAD 历史版本亦无。
- 影响：终验按客户清单打勾必被质疑；且配合修复1 持久化免登录，注册打错密码无自查机会（只能走邮箱重置补救）。
- 修法：注册 Tab 加 `_password2` 字段（图标+眼睛切换同 `#13`），validator 要求与 `_password` 一致；仅限前端校验（后端无该字段，无需动 API）。

**P1-B｜修复5 实现破口（清单项 6 内）：`stop_loss_halt=False`（默认）+ 循环关闭时止损清仓 → 僵尸策略。**
- 链：`grid.py:on_price` 止损分支 → `_do_sell_all` → `_settle_sell`（orders 空）→ `_terminate_no_loop()`（`_loop_done=True`，引擎此后恒 hold）→ 返回 `"halt:stop_loss"`；`runner.py:535` `action.startswith("halt:")` **优先拦截** → `_handle_halt`；`runner.py:567-571` `stop_loss_halt=False` 早退、不置 stopped。`runner.py:537-541` 的 loop_done 兜底分支**永远接不到** `halt:stop_loss`。
- 铁证：`runner.py:539` 注释自列兜底场景「如限价止损清仓」，但止损恰恰以 `halt:` 前缀返回——实现顺序与注释意图自相矛盾。
- 后果：库内 status=running 而引擎永久停摆（资金零风险——已清仓不再动钱；但账实不符、UI 误导、无告警）。触发组合 = 用户开「暂停循环」+ 设止损 + `stop_loss_halt` 缺省 False，属默认可达路径。
- 修法（一行级）：`runner.py:567` 早退条件改为
  `if action == "halt:stop_loss" and not self.engine.cfg.stop_loss_halt and not getattr(self.engine, "loop_done", False):`
  （等价做法：把 loop_done 检查提升到 `halt:` 判定之前）。补一条仿 `test_grid_engine` 的止损+循环关用例。

**P2：**
- P2-A｜本机全局 Python 环境污染（starlette 1.7.0）：任何不带隔离的本地复跑都会假红「17 errors」。声明的「410 passed」在当前全局环境下不可复现。建议在交接文档补一句「测试须走隔离环境/服务器」或由用户决断修复全局环境（本审计不动用户环境）。
- P2-B｜`bizconfig._APP_TEXTS_KEYS` 新增必备键 `deposit_bonus_text`：生产库已存的 app_texts 行若缺该键，后台下一次保存会被 `_validate_app_texts` 拒绝（“合规文案缺少键”），需管理员补键一次性通过；读取面不破（BIZ_DEFAULTS 已含默认值 `config.py:162`，App 端另有兜底文案）。

**P3：**
- `home.dart:205` 注释 `cacheWidth=(56*3)=168` 与实际 `size:44` 不符（最优约 132）；功能无碍（168≪512 原图）。
- `strategies.dart:371`、`strategy_edit.dart:28` 注释仍写「引擎不读/仅客户端标记」，修复5 后已过时（引擎真实消费 `_loop_enabled`）。
- `shield_logo.png` colortype=2（RGB 无 alpha）：`ClipRRect` 圆角外区域若底图非纯色会露方底（视觉提示，由设计确认）。
- `Session.user` 仍为内存态（token 本体已安全持久化，user 仅展示快照，可接受）。

---

## 总体判定

**有条件通过：14 项中 13 PASS / 1 FAIL（项 11 需求缺失）。**
资金安全与红线（项 1/2/8/9/10/14）全部 PASS，dist 恢复字节级可信，测试在干净环境下 442 全绿、App 22 全过。
**放行前提：先修 P1-A（确认密码框）与 P1-B（runner.py:567 早退条件补 loop_done 检查）**；P2/P3 可挂 backlog。
P1 两项均为小改动（前端一个字段 / 后端一行条件+一条测试），修复后建议复跑 `test_grid_engine/test_engine_pool/test_email_pw` 三文件 + Flutter 22 即足够回归。

### 附：审计取证物（qidicode 沙箱，可随时复核）
- `G:\qidicode\target\tmp\k3final-code.diff`（未提交改动全量）
- `G:\qidicode\target\tmp\st041\`（starlette 0.41.3 隔离层，复跑后端测试用）
- `G:\qidicode\target\tmp\app-audit\`（Flutter 测试沙箱副本）
- 后台测试日志：backend 全量 442 passed（run_terminal_command_65）、Flutter +22（run_terminal_command_53）
