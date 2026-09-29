# 更新卡面

游戏更新后，从 `.sc` 重新合成家乡战斗条的 Soldier、Hero、Spell 卡牌模板。

## 为什么需要工具

卡底和人物图分存在 `ui.sc` 的不同符号里。脚本将两者的网格映射到同一画布，
按顶点与 UV 直接绘制成卡牌：
普通兵用 `unit_slot` 图层 1，编号 100 起的活动兵用图层 2；`icon_unit_elite_*` 超级兵
暂沿用 `capacity_slot` 的红底图层 1。英雄用 `hero_slot` 图层 2，法术用 `spell_slot` 图层 1。
数量、等级及卡底 alpha 低于 50% 的边角填成纯绿，供 MAAFW 的
`green_mask` 跳过。
每类卡牌共用一套遮罩位置；咏王另遮住右下角的模式切换开关。
兵种图标按 SC 顶点所在的方形画布定位；可见网格缺少一侧时保留透明留白，避免拉伸后偏移。

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
`{float x, float y, u16 u, u16 v}`；顶点的 `(x, y)` 决定画布位置，UV 决定纹理采样。

这套格式有公开参考实现（`sc-workshop/SupercellFlash`、`danila-schelkov/supercell-swf`），
schema 直接可用，不需要逆向。

## 准备环境

```bash
pip install pillow zstandard texture2ddecoder
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

## 合成卡牌模板

```bash
scripts/refresh-cards/export_cards.py var/coc-unpack/sc/ui.sc
scripts/refresh-cards/export_cards.py var/coc-unpack/sc/ui.sc --out /tmp/cards
```

默认输出到 `assets/image/Cards/{Soldier,Hero,Spell}/<卡牌名>.png`，与旧模板目录分开；
流水线中的 `FindSoldier`、`FindHero`、`FindSpell` 从这三个目录读取模板。
当前内置 47 张 Soldier、5 张 Hero、2 张 Spell 的元数据；以后可在 `export_cards.py` 的
三个数组中追加卡牌及其 SC 符号，不扫描旧模板目录，也不导出建筑大师卡牌。
Soldier 文件名沿用原模板编号（如 `0_Barbarian.png`、`13_ElectroDragon.png`），
超级兵仍用原来的无编号名称，方便在文件夹中浏览。
输出倍率由 `export_cards.py` 顶部的 `SOLDIER_CARD_SCALE`、
`HERO_SPELL_CARD_SCALE` 和 `SUPER_CARD_SCALE` 分别控制。缩放后宽高上限为 1000 像素。
卡面按各类 slot 的固定位置映射到卡底；英雄肖像使用统一的 SC 画布范围保留留白。
需要按 720 高的实机战斗画面对比倍率时，修改对应常量后重新导出。
卡牌用尽后会变灰，三个 `Find*` 节点均使用反向 `TM_SQDIFF_NORMED`
（`method: 10001`），按像素差异区分彩色可用卡和灰卡。
实机截图核对后的阈值为兵种 `0.8`、英雄 `0.85`、法术 `0.85`。

常用参数：

| 参数 | 作用 |
|---|---|
| `--out /tmp/cards` | 将三类模板写到独立输出根目录 |

重复导出会覆盖同名文件，但不会自动删除输出目录里本次未生成的旧 PNG。
发布资源包前应检查这三个目录是否包含最新模板；`--out /tmp/cards` 只用于单独预览，
不会更新流水线正在使用的模板。

## 查看通用 SC 的推导结果

```bash
scripts/refresh-cards/scframes.py var/coc-unpack/sc/ui.sc -o ui.frames.json
scripts/refresh-cards/scframes.py var/coc-unpack/sc/ui.sc --png-out var/coc-unpack/frames
scripts/refresh-cards/scframes.py var/coc-unpack/sc/ui.sc --web-out var/coc-unpack/web
scripts/refresh-cards/scframes.py var/coc-unpack/sc/ui.sc --web-out var/coc-unpack/web --filter capacity_slot
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
`--png-out` 取首帧。`--web-out` 生成 `index.html`，将静态图和动画都编码为 WebP（质量 75），适合网页浏览；
它需要系统安装 `img2webp`。`--png-out` 的 PNG 和浏览页仍可独立使用。
两种输出都会保留组合图，并把根 MovieClip 中可绘制的直接子对象另存为
`<组合图文件名去掉扩展名>+layer-序号.<扩展名>`。图层保留透明背景，与组合图使用同一画布，
可以按子对象顺序叠回去。根 MovieClip 只有一个图层时只保存组合图，不重复导出图层。
文字层在浏览页的组合图上居中叠加 `######`，并单独显示占位卡片；
文字由浏览器以系统默认字体绘制，图片文件不包含文字。
浏览页都把同一符号的组合图与图层放在一组；顶部可按符号名筛选整组，并显示匹配数量。
导出时可用 `--filter` 按符号名包含的文字筛选，不区分大小写；重复传入时匹配任一条件。
筛选只影响 PNG/WebP 导出及其浏览页，已有目录中其他文件不会因此删除。
在 `--web-out` 中，各图层按自己的时间轴长度导出，组合图取最长图层的帧数。
根 MovieClip 只有一帧、子对象有动画时也会导出动画 WebP。动画各帧共用一张固定大小的画布，
较短的时间轴结束后保留最后一帧。使用 `.sc` 记录的帧率；`--max-size` 同样控制画布宽高上限。
相邻画面相同时，编码器可能合并帧，但会保留总播放时长。渲染会应用子对象的 2D 矩阵；
嵌套 MovieClip 从首次出现时开始计帧，文本、颜色变换和裁剪遮罩尚未还原。
`--textures-out` 导出 TextureSets 的各纹理页原尺寸 PNG（`page-000.png` 等），
并生成 `textures.html` 浏览页。每页按 TextureSets 页号读取，优先使用 highres 槽。

## 验收要看什么

1. 检查新模板中的卡面、卡底是否对齐，普通兵为蓝底、超级兵为红底。
2. 检查数量、等级以及卡底 alpha 低于 50% 的边角是否为纯 `(0,255,0)`。
3. 按 MAAFW 的 720 高度匹配实际画面，必要时调整 `CARD_SCALE` 并验证 TemplateMatch 分数。

卡面位置和动态区域目前按 `ui.sc` 这份样本及现有模板确定；游戏更新后如果卡底布局改变，
需复查 `export_cards.py` 中的 `*_FACE_BOX` 与 `*_MASKS`。卡牌名与 SC 符号的对应关系
由脚本内元数据人工维护。
