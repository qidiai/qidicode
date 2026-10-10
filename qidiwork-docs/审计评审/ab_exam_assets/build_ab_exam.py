# -*- coding: utf-8 -*-
"""build_ab_exam.py — 构建 27B 嫁接 A/B 对比考卷（本地跑，不占容器）。
主战场：30 道 HE 代码题（执行验证）+ 10 道真实工程任务（执行验证）+ 10 道真实开放题（rubric）
对照组：10 道风格/格式贴合 + 10 道简单 QA（预期两臂打平，验证考卷本身不偏）
输出：ab_exam_27b.jsonl（sections: he / real_exec / real_open / style / control）
"""
import io
import json
import random
import sys

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")

HE = r"G:\qidicode\target\tmp\c-line\bw_localeval\code_repo\assets\eval_prompts\humaneval_prompts.jsonl"
OUT = r"G:\qidicode\target\tmp\c-line\ab_exam_27b.jsonl"

rows = []

# ── 1. HE 代码题 30 道（seed 42 确定性抽样，带完整执行验证 harness）────────
he_all = []
with open(HE, encoding="utf-8") as f:
    for line in f:
        line = line.strip()
        if line:
            he_all.append(json.loads(line))
rng = random.Random(42)
he_pick = rng.sample(he_all, 30)
for d in he_pick:
    rows.append({
        "section": "he",
        "id": d["task_id"],
        "prompt": d["prompt"],
        "entry_point": d["entry_point"],
        "test": d["test"],
    })
print(f"[exam] he: {len(he_pick)} 题（HE 164 中 seed42 抽样，执行验证）")

# ── 2. 真实工程任务（执行验证）10 道——gig 风格，带自写测试 ────────────────
REAL_EXEC = [
    ("实现一个 LRUCache 类：capacity 上限，get/put 均 O(1)。超出容量淘汰最久未访问项。",
     "class LRUCache:\n    def __init__(self, capacity: int):\n        self.capacity = capacity\n        self.cache = {}\n\n    def get(self, key: int) -> int:\n        if key not in self.cache:\n            return -1\n        v = self.cache.pop(key)\n        self.cache[key] = v\n        return v\n\n    def put(self, key: int, value: int) -> None:\n        if key in self.cache:\n            self.cache.pop(key)\n        elif len(self.cache) >= self.capacity:\n            self.cache.pop(next(iter(self.cache)))\n        self.cache[key] = value",
     "def check(candidate):\n    c = candidate(2)\n    c.put(1, 1); c.put(2, 2)\n    assert c.get(1) == 1\n    c.put(3, 3)\n    assert c.get(2) == -1\n    c.put(4, 4)\n    assert c.get(1) == -1 and c.get(3) == 3 and c.get(4) == 4",
     "LRUCache"),
    ("写函数 dedup_csv_rows(rows)：输入 list[list[str]]（首行为表头），按整行内容去重并保持首次出现顺序，返回新列表。",
     None,
     "def check(candidate):\n    rows = [['a','b'],['1','2'],['a','b'],['3','4'],['1','2']]\n    out = candidate(rows)\n    assert out == [['a','b'],['1','2'],['3','4']]",
     "dedup_csv_rows"),
    ("写函数 count_error_lines(log_text)：统计多行日志文本中以 'ERROR' 开头的行数。",
     None,
     "def check(candidate):\n    log = 'INFO ok\\nERROR x\\nWARN y\\nERROR z\\nERROR w'\n    assert candidate(log) == 3\n    assert candidate('') == 0",
     "count_error_lines"),
    ("实现函数 merge_intervals(intervals)：输入 [[start,end],...]，合并所有重叠区间，返回排序后的合并结果。",
     None,
     "def check(candidate):\n    assert candidate([[1,3],[2,6],[8,10],[15,18]]) == [[1,6],[8,10],[15,18]]\n    assert candidate([[1,4],[4,5]]) == [[1,5]]\n    assert candidate([]) == []",
     "merge_intervals"),
    ("写函数 flatten_dict(d)：把嵌套 dict 展平成一层，键用 '.' 连接（如 {'a':{'b':1}} -> {'a.b':1}）。",
     None,
     "def check(candidate):\n    assert candidate({'a':{'b':1,'c':{'d':2}},'e':3}) == {'a.b':1,'a.c.d':2,'e':3}\n    assert candidate({}) == {}",
     "flatten_dict"),
    ("写函数 roman_to_int(s)：罗马数字转整数（I=1 V=5 X=10 L=50 C=100 D=500 M=1000，含 IV/IX 等减法组合）。",
     None,
     "def check(candidate):\n    assert candidate('III') == 3\n    assert candidate('IV') == 4\n    assert candidate('IX') == 9\n    assert candidate('LVIII') == 58\n    assert candidate('MCMXCIV') == 1994",
     "roman_to_int"),
    ("写函数 longest_common_prefix(strs)：返回字符串列表的最长公共前缀，空列表返回 ''。",
     None,
     "def check(candidate):\n    assert candidate(['flower','flow','flight']) == 'fl'\n    assert candidate(['dog','racecar','car']) == ''\n    assert candidate(['interspecies','interstellar','interstate']) == 'inters'",
     "longest_common_prefix"),
    ("写函数 rotate_matrix(m)：把 n×n 矩阵顺时针旋转 90 度，返回新矩阵（不改原矩阵）。",
     None,
     "def check(candidate):\n    m = [[1,2,3],[4,5,6],[7,8,9]]\n    assert candidate(m) == [[7,4,1],[8,5,2],[9,6,3]]\n    assert m == [[1,2,3],[4,5,6],[7,8,9]]",
     "rotate_matrix"),
    ("写函数 parse_access_log(line)：解析 Nginx 访问日志行 'IP - - [time] \"METHOD path HTTP/1.1\" status size'，返回 dict(ip, method, path, status)（status 为 int）。",
     None,
     "def check(candidate):\n    r = candidate('1.2.3.4 - - [10/Oct/2026:08:00:00] \"GET /index.html HTTP/1.1\" 200 1234')\n    assert r == {'ip':'1.2.3.4','method':'GET','path':'/index.html','status':200}",
     "parse_access_log"),
    ("实现函数 moving_average(window_size)：返回一个对象有 next(val) 方法，输出最近 window_size 个值的均值（不足时用已有均值）。",
     None,
     "def check(candidate):\n    ma = candidate(3)\n    assert ma.next(1) == 1.0\n    assert ma.next(2) == 1.5\n    assert ma.next(3) == 2.0\n    assert ma.next(4) == 3.0",
     "moving_average"),
]
for i, (task, ref, test, entry) in enumerate(REAL_EXEC):
    rows.append({
        "section": "real_exec",
        "id": f"real_exec/{i+1:02d}",
        "prompt": task + "\n\n只输出完整可运行的 Python 代码（含必要 import），不要解释。",
        "entry_point": entry,
        "test": test,
        "reference": ref,
    })
print(f"[exam] real_exec: {len(REAL_EXEC)} 题（自写 gig 风格，执行验证）")

# ── 3. 真实开放任务 10 道（rubric 评审，答案留存）───────────────────────
REAL_OPEN = [
    "下面这段 Python 有一个隐藏 bug，请指出根因并给出修复：\ndef avg(nums):\n    return sum(nums) / len(nums)\n\nprint(avg([]))",
    "为一个'用户上传头像'的 REST 接口设计完整方案：方法、路径、请求/响应格式、错误码、并发与安全注意点。简明分点。",
    "这条 SQL 很慢：SELECT * FROM orders WHERE user_id=123 AND status='paid' ORDER BY created_at DESC LIMIT 10; 表有 5000 万行。给出排查与优化步骤。",
    "为一个纯函数 calculate_discount(price, tier) 写单元测试策略：列出要覆盖的边界与异常路径（不用写全部代码，给用例清单）。",
    "重构建议：一个 800 行的函数 handle_order() 里混着参数校验、库存扣减、支付调用、发短信。给出拆分方案与职责边界。",
    "用三句话讲清并发与并行的区别，以及写爬虫、写计算密集型图像处理各该用哪个（Python 语境）。",
    "为一个支付回调接口写异常处理规范：哪些异常要重试、哪些要告警、哪些要幂等兜底。",
    "设计一个把 MySQL 单表 2 亿行迁移到 PostgreSQL 的方案：停机窗口、双写、校验、回滚各阶段怎么做。",
    "线上服务 CPU 突然 100%，描述你的排查顺序（工具、看什么、先排除什么）。",
    "给这个 PR 写 review 意见：\n- 新增函数 delete_user(id) 直接物理删除用户行\n- 没有任何日志\n- 测试只测了删除成功\n分点列出问题与改进建议。",
]
for i, task in enumerate(REAL_OPEN):
    rows.append({
        "section": "real_open",
        "id": f"real_open/{i+1:02d}",
        "prompt": task + "\n\n请分点作答，简明扼要。",
    })
print(f"[exam] real_open: {len(REAL_OPEN)} 题（rubric 评审）")

# ── 4. 白泽风格/格式贴合 10 道（三段式：结论/依据/代码）──────────────────
STYLE_QS = [
    "Python 的 list 和 tuple 有什么区别？",
    "HTTP 401 和 403 状态码分别是什么含义？",
    "git merge 和 git rebase 的区别？",
    "什么是数据库索引？什么时候不该建索引？",
    "Python GIL 是什么？对多线程有什么影响？",
    "TCP 和 UDP 的核心区别？",
    "什么是幂等性？为什么支付接口需要它？",
    "深拷贝和浅拷贝的区别？",
    "什么是 RESTful API？",
    "虚拟环境和全局环境有什么区别？",
]
for i, q in enumerate(STYLE_QS):
    rows.append({
        "section": "style",
        "id": f"style/{i+1:02d}",
        "prompt": q + "\n\n请用三段式回答：先一行结论，再分点依据，最后如适用给一小段代码。",
    })
print(f"[exam] style: {len(STYLE_QS)} 题（三段式格式贴合）")

# ── 5. 简单 QA 对照组 10 道（预期两臂打平）────────────────────────────
CONTROL = [
    "用CSS把所有 class 为 main-heading 的元素文字颜色改为红色。",
    "写一个 SQL：查询 orders 表里 status='paid' 的订单总数。",
    "numpy 里怎么做矩阵乘法？给一个例子。",
    "Python 里怎么对 list 去重并保持顺序？",
    "怎么反转一个字符串？",
    "怎么按 value 对 dict 排序？",
    "Python 里怎么读整个文本文件到字符串？",
    "HTTP 200/404/500 分别代表什么？",
    "git 里怎么撤销上一次 commit（未推送）？",
    "正则里 \\d 和 \\w 分别匹配什么？",
]
for i, q in enumerate(CONTROL):
    rows.append({
        "section": "control",
        "id": f"control/{i+1:02d}",
        "prompt": q,
    })
print(f"[exam] control: {len(CONTROL)} 题（对照组，预期打平）")

with open(OUT, "w", encoding="utf-8") as f:
    for r in rows:
        f.write(json.dumps(r, ensure_ascii=False) + "\n")

from collections import Counter
c = Counter(r["section"] for r in rows)
print(f"[exam] 总计 {len(rows)} 题 -> {dict(c)}")
print(f"[exam] saved {OUT}")
print("EXAM_BUILT")
