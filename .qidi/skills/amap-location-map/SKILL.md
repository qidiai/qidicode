---
name: amap-location-map
description: >
  高德地图 API 区位图/卫星图生成与周边环境核验。当用户需要为投标/汇报/勘察
  生成项目区位图、获取卫星影像、核验项目坐标与周边道路地标关系时使用。
  触发词：高德API、高德地图、区位图、卫星地图、周边环境核验、项目定位、
  Amap、staticmap、静态图。涵盖 Key 申请口径、坐标核验、卫星底图下载、
  本地标注层渲染、合规提示。已实测：免费 Key 可跑通底图下载，labels/paths
  参数不稳定需走本地标注方案。
---

# 高德地图 API 区位图生成

## 一、Key 申请（选错平台类型是第一步坑）

高德开放平台（lbs.amap.com）→ 创建应用时：

| 用途 | 平台类型 |
|---|---|
| **本 skill（服务端 Python 调 API）** | ✅ **Web服务** |
| 手机 App | Android/iOS（要填 SHA1＋包名） |
| 网页内嵌 | Web端(JS API) |

选「Web服务」**不需要填** SHA1/PackageName（那是 Android 平台 Key 的字段）。

## 二、铁律：坐标必须 API 核验（踩过的坑）

**不要相信浏览器 POI 视口参数**——本次实战曾被 POI 详情页 geoobj 误导，卫星瓦片定位偏了约 12km。正确顺序：

1. `v3/geocode/geo`（门址编码）→ 拿门牌级坐标
2. `v3/place/text`（POI 检索）→ 交叉校验同一地点 ≥2 个 POI
3. 两个坐标一致后再下载卫星底图

```python
# 端点（key=你的Web服务Key）
GET https://restapi.amap.com/v3/geocode/geo?key=KEY&address=合肥市庐阳区XX路55号
GET https://restapi.amap.com/v3/place/text?key=KEY&keywords=易捷特大厦&city=合肥
GET https://restapi.amap.com/v3/staticmap?key=KEY&location=lon,lat&zoom=17&size=1024*768&scale=2&maptype=satellite
```

返回全部 JSON（除静态图成功时返回 PNG，`\x89PNG` 头）；`status=="1"` 才是成功。

## 三、免费 Key 的边界（实测）

| 能力 | 免费 Key |
|---|---|
| 卫星底图下载（location/zoom/size/scale） | ✅ 可用 |
| markers | ✅ 可用 |
| **labels（服务端文字标注）** | ⚠️ 不稳定，间歇 INVALID_PARAMS / CUQPS 10021（配额限流） |
| **paths（画线/框）** | ⚠️ 不稳，UNKNOWN_ERROR 20003 |
| QPS | 约 1 QPS，脚本内请求间隔 ≥1.5s |

**结论：labels/paths 一律放弃，走"官方底图 + PIL 本地标注层"方案**（scripts/amap_plot.py），效果相同且不受配额波动影响。

## 四、本地标注层要点（amap_plot.py）

- **Web 墨卡托换算**：`x_px = (lon+180)/360 × 2^zoom × 256 × scale`；y 经 log 变换；图片像素 = 世界 px − 中心 px + 图幅/2
- **比例尺**：`m_per_px = 156543.03392 × cos(lat) / 2^zoom / scale`，按目标米数反算条长
- 标注样式：深蓝底(44,62,80)白字粗体矩形，间距 8px；项目红点 (231,76,60)；用地框 (231,76,60) 半透明
- **必加 © 角标**："底图 © 高德地图"（白底，右下角）

## 五、合规红线（必须提示用户）

高德《开放平台服务协议》明确：**免费 Key 仅限短期测试**；"用于参与第三方投标/对外销售/上架公开/收费获益/长期大量调用"均需**购买技术服务许可**。

| 场景 | 建议 |
|---|---|
| 内部勘察/汇报/核验 | 直接用，留 © 角标 |
| 投标文件插图 | ⚠️ 提示用户补技术服务许可，或改用其他合规底图；**不得静默插入** |

## 六、脚本

- `scripts/amap_test.py` — Key 有效性 + 门址/POI 双坐标核验
- `scripts/amap_plot.py` — 卫星底图 + PIL 标注层 → 区位图（参数化标注列表，改 LON/LAT/标注文本即可复用）

```powershell
python scripts\amap_test.py    # 先核验坐标
python scripts\amap_plot.py    # 再出图
```

坐标系：高德为 **GCJ-02**（非 WGS-84、非 BD-09）；与 GPS 原始坐标混用会有偏移，修正需单独做转换（本 skill 不覆盖）。
