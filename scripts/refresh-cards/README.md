# 更新卡面

游戏更新后，从游戏自己的文件里重新导出战斗条卡牌的卡面图，不用手工截图、不用手工裁。

## 为什么需要工具

卡面不是独立图片。CoC 的卡面是 `assets/sc/ui.sc` 里图集页上的一块矩形，而且**游戏不存这块矩形** ——
它存的是矢量形状：符号名 → 对象 → 绘制命令 → 顶点，每个顶点带一对 16 位归一化 uv 坐标，
卡面矩形是这些 uv 的包围盒乘上页尺寸算出来的。所以想要卡面，必须复现引擎这套推导。

`.sc` 的结构（v6 容器）：

```
SCFILE 容器 = "SC" + 版本 + FlatBuffers 描述头长度 + 描述头 + zstd 帧
描述头      = 包含符号元数据（当前 ui.sc 为 3043 条，每条有名字和 16 字节哈希）
正文        = 连续的 [u32 长度][flatbuffers 分块]
              Resources / Exports / TextFields / Shapes / MovieClips / Modifiers / TextureSets
```

当前 `ui.sc` 的外层头部：偏移 `0..1` 是 `SC`，`2..5` 是小端版本号 `6`，`6..7` 是 `0`；
偏移 `8..11` 的 `196476` 是**描述头长度**。描述头位于 `12..196487`，本身也是
FlatBuffers，3043 条符号元数据在其中；zstd 帧从偏移 `196488` 开始。

推导链：`FBExports` 给出「符号名 ↔ 对象 id」，id 解析到 `FBShape` 的绘制命令
`{页号, 顶点数, 起始顶点下标}`，再回 `FBResources.shape_points` 取顶点
`{float x, float y, u16 u, u16 v}`，uv 包围盒 ÷ 65536 × 页宽高就是像素矩形。

这套格式有公开参考实现（`sc-workshop/SupercellFlash`、`danila-schelkov/supercell-swf`），
schema 直接可用，不需要逆向。

## 准备环境

```bash
pip install pillow numpy zstandard texture2ddecoder
```

## 拉取文件

从设备拉取 APK

```bash
adb pull "$(adb shell pm path com.supercell.clashofclans | tr -d '\r' | sed 's/^package://' | grep /split_install_time_asset_pack.apk)" var/coc-unpack/apk/
```

从 APK 里取出指定的 `.sc` 文件：

```bash
unzip -o -j var/coc-unpack/apk/split_install_time_asset_pack.apk assets/sc/ui.sc -d var/coc-unpack/sc
```

## 导出全部卡面

```bash
scripts/refresh-cards/sccards.py var/coc-unpack/sc/ui.sc
```

约 2 秒，输出 `assets/image/cards/<符号名>.png`，当前共 236 张。
两个仅大小写不同的符号在大小写不敏感的文件系统上会冲突，脚本会给它们加稳定后缀。
卡牌类型和卡牌背景由后续维护的「卡牌 ↔ SC 符号名」对应表决定，这里只按符号名导出图片。

常用参数：

| 参数 | 作用 |
|---|---|
| `--prefix icon_unit_` | 只要某类前缀，可重复（默认 `icon_unit_` 和 `icon_spell_`） |
| `--out /tmp/cards` | 换输出目录，不碰仓库里的 |

导出的 PNG **不入库**，随时可以重新生成。重复导出会覆盖同名文件，
但不会自动删除输出目录里本次未生成的旧 PNG。

## 查看通用 SC 的推导结果

```bash
scripts/refresh-cards/scframes.py var/coc-unpack/sc/ui.sc -o ui.frames.json
scripts/refresh-cards/scframes.py var/coc-unpack/sc/ui.sc --png-out var/coc-unpack/frames
scripts/refresh-cards/scframes.py var/coc-unpack/sc/ui.sc --apng-out var/coc-unpack/animations
scripts/refresh-cards/scframes.py var/coc-unpack/sc/ui.sc --textures-out var/coc-unpack/textures
```

它会检查保留下来的非零面积矩形是否落在对应纹理页内；无效页号、顶点范围等结构错误会直接报错。
输出 `越界自检: 0 处异常` 说明这批矩形通过了边界检查，但不代表卡面朝向和选块都正确。
树状输出会列出各分块的已读字段和最多 6 个示例；`TextFields` 等未用于卡面推导的内容只统计数量。
`-o` 写出的 JSON 仍包含全部矩形。
`--png-out` 按顶点 `(x, y)` 和 UV 对三角网格做纹理映射，生成各符号的首帧 PNG 与
`index.html` 浏览页。顶点到像素的倍率在 `scrender.py` 的 `PIXELS_PER_UNIT` 中，当前为 1。
输出宽高默认各不超过 1000 像素；可用 `--max-size` 修改。超限时等比缩小，文件名会注明
缩放前后的尺寸（例如 `--2400x1200-to-1000x500.png`），浏览页也会列出两种尺寸。
普通 `--png-out` 取首帧。`--apng-out` 同样导出全部符号并生成 `index.html`：多帧
MovieClip 的组合图导出为循环播放的 APNG，单帧符号的组合图导出为普通 PNG。
两种输出都会保留组合图，并把根 MovieClip 中可绘制的直接子对象另存为
`<组合图文件名去掉.png>+layer-序号.png`。图层保留透明背景，与组合图使用同一画布，
可以按子对象顺序叠回去。只有一个可绘制图层时只保存组合图，不重复导出图层。
文字层会在浏览页标出，但暂不生成图片。
两种浏览页都把同一符号的组合图与图层放在一组；顶部可按符号名筛选整组，并显示匹配数量。
在 `--apng-out` 中，各图层按自己的时间轴长度导出，组合图取最长图层的帧数。
根 MovieClip 只有一帧、子对象有动画时也会导出 APNG。动画各帧共用一张固定大小的画布，
较短的时间轴结束后保留最后一帧。使用 `.sc` 记录的帧率；`--max-size` 同样控制画布宽高上限。
相邻画面相同时，编码器可能合并帧，但会保留总播放时长。渲染会应用子对象的 2D 矩阵；
嵌套 MovieClip 从首次出现时开始计帧，文本、颜色变换和裁剪遮罩尚未还原。
`--textures-out` 导出 TextureSets 的各纹理页原尺寸 PNG（`page-000.png` 等），
并生成 `textures.html` 浏览页。每页按 TextureSets 页号读取，优先使用 highres 槽。

## 验收要看什么

1. 输出里没有「裁不出 N 个」这一行。
2. 缩略图扫一遍有没有倒置、镜像或裁错。图集里有 80 张图块原本带旋转或镜像，
   导出时会自动校正朝向。

## 朝向为什么不能用简单规则

图块在图集里可能是 8 种存法之一（二面体群 D4）。判法是：把**每条绘制命令**自己的顶点
从局部 `(x,y)` 拟合到页内像素 `(u,v)`（**必须带常数项**，因为局部原点在形状中心而 uv 原点在页角），
取线性部分 K，再选让 `O·K` 成为「正数倍单位阵」的那个 O。残差是精确几何量，正常都在 1e-4 量级。

两个踩过的坑：

- 只区分「转 90 度 / 没转」不够 —— 有 8 张的 K 是 `diag(1,-1)`，那是**镜像**不是旋转，
  必须把 flip 也放进候选集。
- 朝向必须按命令单独算。一个卡面 MovieClip 的孩子混着遮罩、脸和背景块，各有各的局部坐标系，
  混在一起拟合会互相污染。

## 已知没解决的

- **中文名**。`assets/localization/{cn,cnt,de,...,texts}.csv` 23 个语言文件确实在 APK 里，
  但和 `assets/logic/*.csv` 同一套加密（整体熵 7.999 bits/byte，头部 `5d 00 00 04 00` + u32 明文长度，
  zstd/zlib/lzma/bz2 都解不开），拿不到明文，需要人工维护。`assets.scdb` 是明文 SQLite，
  但 `tags` 列 6,890 行全空，指望不上。
- **导出的卡面是干净的脸，不含卡框**。现有 `assets/image/Soldier/` 那 48 张模板是「脸 + 外框 +
  数量角标 + 等级角标」合成后的屏幕截图裁片，所以拿导出图和它做像素比对**必然有差**，
  这个差不能用来判对错 —— 真正的判据只有流水线的 TemplateMatch 分数。
- **绿块还没打**。`green_mask` 用的纯 `(0,255,0)` 角标遮罩需要后处理时加，
  从现有 26 张模板反推的相对比例中位数是 `(左 0.047, 上 0.656, 宽 0.295, 高 0.261)`，分量范围都很窄。
- **`assets/image/Soldier/` 那 48 张模板还是老的手起名**，且没有绿块。要让流水线用这批官方
  命名的图，得先定后处理规则（统一尺寸、装框方式、绿块位置），再把模板迁成符号名。
