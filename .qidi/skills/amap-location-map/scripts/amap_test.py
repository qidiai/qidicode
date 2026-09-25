# -*- coding: utf-8 -*-
"""amap_test.py — 高德 Key 有效性 + 门址/POI 双坐标核验
用法：改 KEY / ADDRESS / POI_KEYWORD 后运行
"""
import sys, io, json, urllib.request, urllib.parse, time
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")

KEY = "在此填入你的高德Web服务Key"
ADDRESS = "合肥市庐阳区寿春路55号"
POI_KEYWORD = "易捷特大厦"
CITY = "合肥"

def get(url):
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
    with urllib.request.urlopen(req, timeout=20) as r:
        return json.loads(r.read().decode("utf-8"))

d = get("https://restapi.amap.com/v3/geocode/geo?" + urllib.parse.urlencode({"key": KEY, "address": ADDRESS}))
print("== 门址编码 ==", d.get("status"), d.get("info"), d.get("infocode"))
geo = d.get("geocodes") or []
for g in geo:
    print("  ", g.get("formatted_address"), "|", g.get("location"), "|", g.get("level"))
if not geo:
    print("  [FAIL] 无结果——检查Key或地址写法（省市区+路+门牌）")
    sys.exit(1)

time.sleep(1.5)
d2 = get("https://restapi.amap.com/v3/place/text?" + urllib.parse.urlencode(
    {"key": KEY, "keywords": POI_KEYWORD, "city": CITY, "offset": 5}))
print("== POI 检索 ==", d2.get("status"), d2.get("info"), d2.get("infocode"))
pois = d2.get("pois") or []
for p in pois[:5]:
    print("  ", p.get("name"), "|", p.get("address"), "|", p.get("location"))
if not pois:
    print("  [WARN] POI 未命中——可用门址坐标继续，但建议换关键词复核")

time.sleep(1.5)
lon, lat = [float(x) for x in geo[0]["location"].split(",")]
print("\n== 建议主坐标（门址） ==", lon, lat)
if pois:
    plon, plat = [float(x) for x in pois[0]["location"].split(",")]
    dist = ((lon-plon)**2+(lat-plat)**2)**0.5
    print("== POI 主坐标 ==", plon, plat, "| 与门址偏差 %.4f 度 ≈ %.0f m" % (dist, dist*97000))
    if dist > 0.002:
        print("  [WARN] 两坐标偏差>200m，请人工确认目标点（换 POI 关键词）")
print("[OK] 坐标核验完成——把主坐标填入 amap_plot.py 的 LON/LAT")
