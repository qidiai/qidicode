# -*- coding: utf-8 -*-
"""记忆检索精度基准：向量 vs FTS5 vs RRF 融合，语料=真实记忆库 619 块
用法：在 qidicode-portable 工作区根目录运行（需 8800 服务在线）：
    $env:PYTHONIOENCODING='utf-8'; python .qidi/services/baize-embed/bench_retrieval.py
换任何 embedding 后端后复跑同一脚本即可横向对比。评估结论见
.qidi/docs/s5-retrieval-eval.md（2026-08-27：S5 hit@1/5=3/5, TF-IDF基线 8/12）"""
import sqlite3, json, time, math, urllib.request

DB = r'.qidi/memory/qidicode-portable-b70e26d9/index.sqlite'
API = 'http://127.0.0.1:8800/v1/embeddings'

# (类别, 查询, gold子串) —— gold 由运行时 LIKE 验证，语料无 gold 则跳过
QUERIES = [
    # A 语义改写（换说法，低字面重叠）
    ('A改写', '服务重启后进程又不见了怎么办', 'Job Object'),
    ('A改写', '接口全被限流了怎么排查', '429'),
    ('A改写', '下载的模型缓存不完整缺权重文件', 'safetensors'),
    ('A改写', '标书技能里怎么把命令列表挖出来', '斜杠命令'),
    ('A改写', '门派的配色在哪里改', 'SectColors'),
    ('A改写', '远程只能排队执行一条命令新的会覆盖旧的', '单槽'),
    ('A改写', '那批截图是不是机器生成的', '合成'),
    ('A改写', '比赛环境模型目录都不存在', 'NPU'),
    ('A改写', '水墨风格着色器报错游戏启动不了', '水墨'),
    ('A改写', 'water ink shader for the game', '水墨'),   # 跨语言
    # B 错别字（S5 宣称强项）
    ('B错字', '虎王运地图标注项目收尾了吗', '地图标注'),   # 云→运
    ('B错字', '白择s5是多少维的向量', '白泽'),             # 泽→择
    ('B错字', '阿童木扣的签名无效报四零三', 'ATOMCODE_SIG_INVALID'),  # 全意译+谐音
    # C 精确关键词（控制组，FTS 主场）
    ('C精确', 'ATOMCODE_SIG_INVALID', 'ATOMCODE_SIG_INVALID'),
    ('C精确', 'WMI 拉起进程', 'WMI'),
    ('C精确', 'sqlite-vec', 'sqlite-vec'),
]

def embed(texts, with_dim=False):
    body = {"model": "baize-s5", "input": texts}
    if with_dim:
        body["dimensions"] = 1024
    req = urllib.request.Request(API, data=json.dumps(body).encode(),
                                 headers={'Content-Type': 'application/json'})
    with urllib.request.urlopen(req, timeout=120) as r:
        data = json.loads(r.read())
    return [d['embedding'] for d in sorted(data['data'], key=lambda x: x['index'])]

def cos(a, b):
    d = sum(x*y for x, y in zip(a, b))
    na = math.sqrt(sum(x*x for x in a)); nb = math.sqrt(sum(y*y for y in b))
    return d/(na*nb) if na and nb else 0.0

# ---- 载入库 ----
c = sqlite3.connect(DB)
chunks = c.execute('SELECT rowid, path, text FROM chunks').fetchall()
texts = [t for _, _, t in chunks]
print(f'corpus: {len(chunks)} chunks')

# ---- 语料向量化（走真实 API，计吞吐）----
t0 = time.time()
vecs = []
B = 32
for i in range(0, len(texts), B):
    vecs.extend(embed(texts[i:i+B]))
dt = time.time() - t0
print(f'corpus embedded: {len(vecs)} vecs in {dt:.1f}s = {len(vecs)/dt:.0f} items/s')

# ---- 逐查询评测 ----
def fts_query(q):
    toks = [t for t in q.split() if t]
    qs = ' OR '.join('"' + t.replace('"', '""') + '"' for t in toks) or '""'
    try:
        rows = c.execute('SELECT rowid FROM chunks_fts WHERE chunks_fts MATCH ? '
                         'ORDER BY bm25(chunks_fts) LIMIT 20', (qs,)).fetchall()
        return [r[0] for r in rows]
    except Exception:
        return []

rowid2idx = {rowid: i for i, (rowid, _, _) in enumerate(chunks)}
agg = {}
detail = []
for cat, q, gold in QUERIES:
    gold_ids = {rowid for rowid, _, text in chunks if gold in text}
    if not gold_ids:
        detail.append((cat, q, gold, 'GOLD缺失-跳过', '', '', ''))
        continue
    t1 = time.time()
    qv = embed([q], with_dim=True)[0]
    qms = time.time() - t1
    sims = sorted(((cos(qv, vecs[i]), chunks[i][0]) for i in range(len(chunks))), reverse=True)
    v_top = [rid for _, rid in sims[:20]]
    f_top = fts_query(q)
    # RRF 融合（qidi hybrid 思路近似）
    rrf = {}
    for r, rid in enumerate(v_top): rrf[rid] = rrf.get(rid, 0) + 1/(60+r+1)
    for r, rid in enumerate(f_top): rrf[rid] = rrf.get(rid, 0) + 1/(60+r+1)
    r_top = [rid for rid, _ in sorted(rrf.items(), key=lambda x: -x[1])[:20]]

    def hit(top, k): return any(rid in gold_ids for rid in top[:k])
    r = (cat, q, gold,
         f"V1:{'Y' if hit(v_top,1) else 'n'} V5:{'Y' if hit(v_top,5) else 'n'}",
         f"F1:{'Y' if hit(f_top,1) else 'n'} F5:{'Y' if hit(f_top,5) else 'n'}",
         f"R1:{'Y' if hit(r_top,1) else 'n'} R5:{'Y' if hit(r_top,5) else 'n'}",
         f"{qms*1000:.0f}ms score={sims[0][0]:.3f}")
    detail.append(r)
    a = agg.setdefault(cat, [0,0,0,0,0,0,0])
    a[0] += 1
    if hit(v_top,1): a[1] += 1
    if hit(v_top,5): a[2] += 1
    if hit(f_top,1): a[3] += 1
    if hit(f_top,5): a[4] += 1
    if hit(r_top,1): a[5] += 1
    if hit(r_top,5): a[6] += 1

print('\n===== 每条查询 =====')
for cat, q, gold, v, f, r, extra in detail:
    print(f'[{cat}] {q}  (gold:{gold})')
    print(f'    {v} {f} {r} | {extra}')

print('\n===== 汇总（hit@1 / hit@5）=====')
print(f'{"类别":<8}{"n":>3}  {"向量":>9}  {"FTS":>9}  {"RRF融合":>9}')
for cat, a in agg.items():
    print(f'{cat:<8}{a[0]:>3}  {a[1]}/{a[0]} {a[2]}/{a[0]:<3}  {a[3]}/{a[0]} {a[4]}/{a[0]:<3}  {a[5]}/{a[0]} {a[6]}/{a[0]}')
tn = sum(a[0] for a in agg.values())
print(f'{"总计":<8}{tn:>3}  {sum(a[1] for a in agg.values())}/{tn} {sum(a[2] for a in agg.values())}/{tn}  '
      f'{sum(a[3] for a in agg.values())}/{tn} {sum(a[4] for a in agg.values())}/{tn}  '
      f'{sum(a[5] for a in agg.values())}/{tn} {sum(a[6] for a in agg.values())}/{tn}')

# ---- 抽 3 条展示 top-3 定性结果 ----
print('\n===== 定性抽查 =====')
for q, gold in [('服务重启后进程又不见了怎么办', 'Job Object'),
                ('白择s5是多少维的向量', '白泽'),
                ('water ink shader for the game', '水墨')]:
    qv = embed([q], with_dim=True)[0]
    sims = sorted(((cos(qv, vecs[i]), chunks[i]) for i in range(len(chunks))), reverse=True)
    print(f'\nQ: {q}  (gold: {gold})')
    for s, (rid, path, text) in sims[:3]:
        mark = ' <-HIT' if gold in text else ''
        print(f'  {s:.3f} [{rid}] {path.split(chr(92))[-1][:30]} | {text[:70]!r}{mark}')
