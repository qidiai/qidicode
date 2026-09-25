# -*- coding: utf-8 -*-
"""amap_plot.py — 高德卫星底图 + PIL 本地标注层 = 项目区位图
------------------------------------------------------------------------
用法：
  1. 用 amap_test.py 核验项目坐标（KEY 要填 Web 服务 Key）
  2. 改下面的 LON / LAT / LABELS / BOX，运行即可出图

已实测结论（免费 Key）：
  - 卫星底图（location/zoom/size/scale）可用；请求间隔 >=1.5s（QPS≈1）
  - labels/paths 参数不稳定（INVALID_PARAMS / CUQPS 10021），故标注走本地 PIL
  - scale=2 输出 2048x1536，可满足打印
"""
import sys, io, os, math, urllib.request, time
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
from PIL import Image, ImageDraw, ImageFont

# ---------------- 用户配置区 ----------------
KEY = "在此填入你的高德Web服务Key"
LON, LAT = 117.296825, 31.865269          # 项目坐标（GCJ-02，先跑 amap_test.py 核验）
ZOOM, SCALE, W, H = 17, 2, 1024, 768
MAPTYPE = "satellite"
OUT = "项目区位图.png"

# 标注列表：(文字, 经度, 纬度, 字号, 锚点)  锚点 lt/rt/lb/rb = 文字相对锚点的方位
LABELS = [
    ("本项目（寿春路55号）",       117.2962, 31.86445, 34, "lt"),
    ("寿春路（城市主干道）",       117.2968, 31.86745, 34, "lt"),
    ("庙后街",                   117.29300, 31.86495, 34, "rt"),
    ("明教寺·淮河路步行街",       117.2970, 31.86280, 34, "lt"),
    ("逍遥津公园",               117.2997, 31.86955, 34, "lt"),
]
# 用地示意框（矩形，经纬度），None = 不画
BOX = {"lon0": 117.29565, "lat0": 31.86515, "lon1": 117.29805, "lat1": 31.86455}
# -------------------------------------------

IW, IH = W * SCALE, H * SCALE
BAR_M = 200  # 比例尺米数

def lonlat2px(lon, lat):
    n = 2 ** ZOOM
    xr = (lon + 180) / 360 * n * 256 * SCALE
    yr = (1 - math.log(math.tan(math.radians(lat)) + 1 / math.cos(math.radians(lat))) / math.pi) / 2 * n * 256 * SCALE
    return xr, yr

cx, cy = lonlat2px(LON, LAT)
def to_img(lon, lat):
    xr, yr = lonlat2px(lon, lat)
    return int(xr - cx + IW / 2), int(yr - cy + IH / 2)

url = ("https://restapi.amap.com/v3/staticmap?"
       f"key={KEY}&location={LON},{LAT}&zoom={ZOOM}&size={W}*{H}&scale={SCALE}&maptype={MAPTYPE}")
req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(req, timeout=60) as r:
    data = r.read()
assert data[:4] == b"\x89PNG", "静态图返回非PNG：%s" % data[:200]
open("_base.png", "wb").write(data)
print("[底图]", len(data) // 1024, "KB")
time.sleep(1.2)

img = Image.open("_base.png").convert("RGB")
dr = ImageDraw.Draw(img, "RGBA")

def font(sz, bold=True):
    for p in ("C:/Windows/Fonts/msyhbd.ttc" if bold else "C:/Windows/Fonts/msyh.ttc",
              "C:/Windows/Fonts/simhei.ttf"):
        try:
            return ImageFont.truetype(p, sz)
        except Exception:
            continue
    return ImageFont.load_default()

if BOX:
    x0, y0 = to_img(BOX["lon0"], BOX["lat0"])
    x1, y1 = to_img(BOX["lon1"], BOX["lat1"])
    dr.rectangle([x0, y0, x1, y1], fill=(231, 76, 60, 40), outline=(231, 76, 60, 255), width=5)

def label(text, lon, lat, fs, anchor="lt"):
    x, y = to_img(lon, lat)
    f = font(fs)
    tb = dr.textbbox((0, 0), text, font=f)
    w, h = tb[2] - tb[0], tb[3] - tb[1]
    if anchor == "rt": x -= w
    if anchor == "rb": x -= w; y -= h
    if anchor == "lb": y -= h
    dr.rectangle([x - 8, y - 6, x + w + 8, y + h + 10], fill=(44, 62, 80, 225))
    dr.text((x, y - 2), text, font=f, fill=(255, 255, 255, 255))

for t, lo, la, fs, an in LABELS:
    label(t, lo, la, fs, an)

mx, my = to_img(LON, LAT)
dr.ellipse([mx - 12, my - 12, mx + 12, my + 12], fill=(231, 76, 60, 255), outline=(255, 255, 255, 255), width=3)

nx, ny = IW - 90, 90
dr.polygon([(nx, ny - 46), (nx - 16, ny + 16), (nx, ny + 4), (nx + 16, ny + 16)], fill=(30, 30, 30, 255))
dr.text((nx - 8, ny + 22), "N", font=font(36), fill=(30, 30, 30, 255))

m_per_px = 156543.03392 * math.cos(math.radians(LAT)) / (2 ** ZOOM) / SCALE
bar_px = int(BAR_M / m_per_px)
sx0, sy0 = 60, IH - 70
dr.rectangle([sx0, sy0, sx0 + bar_px, sy0 + 16], fill=(255, 255, 255, 230), outline=(20, 20, 20, 255), width=3)
dr.text((sx0 + bar_px + 14, sy0 - 8), f"{BAR_M}m", font=font(30), fill=(20, 20, 20, 255))

f9 = font(26, False)
dr.rectangle([IW - 250, IH - 46, IW - 12, IH - 12], fill=(255, 255, 255, 190))
dr.text((IW - 242, IH - 42), "底图 © 高德地图", font=f9, fill=(60, 60, 60, 255))

img.save(OUT)
os.remove("_base.png")
print("[OK] 区位图已生成:", os.path.abspath(OUT), img.size)
