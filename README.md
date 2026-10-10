# MaaCoC

基于 [MaaFramework](https://github.com/MaaXYZ/MaaFramework) 的《部落冲突》自动战斗客户端。
连接手机或模拟器，自动完成"匹配对手 → 放兵 → 结束回村"的循环。

当前状态：自动战斗已在真机（POCO F3 / Redmi K40）验证可用，带图形界面。
支持自定义配卡、卡组分享和通用投放策略。

## 环境要求

- **设备分辨率必须是 1080x1920**，否则所有模板都对不上。挂机页底部有「设为
  1080×1920」「恢复默认」两个按钮，走 MaaFramework 自己的 adb 通道执行；画面
  左下角会显示 `物理 · 当前` 两个尺寸供核对。之所以要这个尺寸：MaaFramework 把
  截图缩到短边 720 再识别，所有模板和 ROI 都写在 **1280x720 坐标空间**里。
- 已开启 USB 调试、用数据线连好的设备或模拟器。
- 输入使用 **MaaTouch**，所需 `MaaAgentBinary` 随 SDK 提供并随应用打包。缺失时连接失败，避免 AdbShell 把长按静默降级为单击。
- Rust stable（`rust-toolchain.toml` 会带上 clippy 和 rustfmt）、Node 22。

## 准备依赖

根目录没有 `package.json` —— 前端工程在 **`app/`** 子目录，所有 `npm` 命令都要在那儿跑。

MaaFramework 的 SDK 不进仓库，按平台现拉：

```bash
./scripts/fetch-sdk.sh          # 下载 v5.14.0-beta.1 到 vendor/
python3 scripts/fetch-ocr.py    # 下载并校验满仓检查使用的 OCR 模型
(cd app && npm ci)              # 前端依赖
./scripts/sync-libs.sh          # 把动态库放到 target/debug 旁边
```

## 图形界面

```bash
cd app
./node_modules/.bin/tauri dev    # 注意不要用 npx tauri，那会去装同名的另一个包
```

三页：**挂机**（实时画面 + 节点时间轴 + 连接/开始/停止）、**战果**（局数、命中、
未命中最多的节点）、**设置**（设备、任务入口、刷新间隔、外观等）。

没有设备也能开发：前端在开发模式下会自动接上一份 mock 引擎，可以直接在浏览器里
打开 `http://localhost:5173/?page=run&theme=dark&run=1` 驱动和检查界面。

## 命令行

只想要引擎和命令行工具、不碰界面，先 `cargo build --workspace`：

```bash
cargo run --bin maacoc-engine -- devices                     # 列设备
cargo run --bin maacoc-engine -- snap frame.png              # 截一帧，报告匹配空间尺寸
cargo run --bin maacoc-engine -- shell wm size               # 通过 MaaFramework 在设备上执行命令
cargo run --bin maacoc-engine -- load                        # 加载资源并列出节点
cargo run --bin maacoc-engine -- run --minutes 10            # 跑自动战斗并打印时间轴
cargo run --bin maacoc-engine -- run --strategy units-first # 先投兵种、战车、英雄，再投法术
cargo run --bin maacoc-engine -- reco FindUnit frame.png     # 在静态帧上试跑某个节点
cargo run --bin maacoc-engine -- regress                     # 对已录制的语料帧重跑识别
cargo run --bin maacoc-engine -- patch AttackStart threshold 0.9   # 只打印 diff
```

应用设置中的“投放策略”可选择法术优先（默认）或兵种优先，停止后切换，下一次运行生效。所有预设在收益筛选通过后先执行两次双指缩小，再向右拖动画面，然后开始投放。法术优先和兵种优先仍使用现有左侧投放位置与英雄技能流程。

“左侧分散投放”（CLI：`--strategy left-spread`）在确认对手画面后先执行两次双指缩小，再向右拖动画面，等待稳定后按兵种、战车、英雄、法术顺序投放。兵种在左侧三个点各长按 2 秒，战车和英雄从左侧中央进入；法术在进攻路径五个点各点击 3 次。每位英雄落地后等待 3 秒，再确认战斗仍在进行才点击技能；等待时间在 `SpreadHeroWait.post_delay` 调整。长按只控制节奏，不保证精确数量。

镜头手势、投放坐标、时长和间隔集中在 `assets/pipeline/left-spread.json`，坐标以 MAA 720 高画面为准，目前在 POCO F3 的 1280×720 画面校准。缩放达到游戏下限后继续收拢不会再改变视野；拖移也受地图边界限制。镜头准备只在进入/恢复战斗时执行，不在每个兵种投放前重复。类别顺序由 `engine/src/strategy.rs` 的流水线覆盖配置决定。此预设采用固定位置，没有阵型识别或部队跟随。

“气球探路 + 分散投放”（`--strategy air-probe`）复用相同镜头和主力投放流程，开局先寻找普通气球或火箭气球，在三个点各长按 400 毫秒，再投放主力；没有气球直接跳过。探路只执行一次，时长控制不是精确计数。相关参数在 `assets/pipeline/air-probe.json`。

挂机页展示实时卡牌栏和当前阶段，数量与可用状态直接来自设备画面。竖屏、寻找对手和返回村庄时隐藏卡牌栏，避免显示上一场战斗的数据。

`regress` 是防资产退化的关键工具：`tests/fixtures/frames/` 里存着真机录下的帧和
当时的识别结果，改模板或改阈值后跑一遍就知道有没有把别的地方弄坏。
索引支持 `expected_hit: false`，可验证灰掉的卡牌不应命中；省略时沿用原有的“应命中”规则。

`patch` 按字节定点改写 `assets/pipeline/main.json`，保留注释和原有排版，
所以一行改动就只有一行 diff。不加 `--write` 时只打印，不落盘。

## 自定义配卡与分享

“配卡”页可以点击“读取手机当前卡组”回填部队、法术和战车；读取不会增删部队，遇到相似或无法确认的卡牌会停止。
也可按英文名称选择卡牌，填写数量后点击“应用到手机”。
流程先检查游戏当前开放的卡牌和军队容量，再替换现有配置；配完核对数量及总人口／容量。
失败会停止，不会自动开始战斗。英雄、宠物和装备沿用手机当前配置。
目前最多配置 6 种兵、6 种法术和 3 种战车；未配置的类别会被清空。
活动卡列表包含往期素材，游戏未开放的卡牌会在预检时停止。

“生成分享码”导出 `MC2:` 字符串，包含卡组、当前投放策略和自定义战术。
导入只更新本地配置，点击应用后才操作手机。分享码不携带设备地址、账号或挂机筛选设置。
“自定义战术”可按卡牌排列释放步骤，配置点击次数、固定坐标、步骤前等待和点击后等待（毫秒），最多 32 步／600 次点击。
坐标以 1280×720 匹配空间为准，Y 限制在 40～560，避免点击顶部和底部控制区。
可选择开始前先缩小、向右拖动画面。不可用卡牌会跳过；步骤结束后沿用设置中的通用策略释放剩余卡牌。
点击次数不等同于成功投放数量，点位是否在可投放区域需实机校准。战术默认关闭，停止流程后编辑，下次开始生效。
分享码不兼容旧版格式。
这份格式参考 [MAA 作业协议](https://docs.maa.plus/zh-cn/protocol/copilot-schema.html) 中编队与动作一起存储的思路。

命令行也可使用 `cargo run --bin maacoc-engine -- army army.json --device <serial>`。
使用 `cargo run --bin maacoc-engine -- army-read --device <serial>` 可只读导出当前卡组 JSON。
增加 `--verify-only` 可只检查当前卡组，不移除或添加卡牌。
JSON 形式为 `{"entries":[{"id":"Unit/13_ElectroDragon","count":11}]}`，卡牌 ID 见 `assets/army.json`。
该命令同样会替换三类配置，数量为 0 的条目不要写入。

## 测试与检查

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd app && npx tsc --noEmit
```

## 打包发布

```bash
cd app
./node_modules/.bin/tauri build --bundles app,dmg     # macOS
./node_modules/.bin/tauri build --bundles nsis        # Windows
./node_modules/.bin/tauri build --bundles deb         # Linux
../scripts/verify-bundle.sh                           # 检查包里确实带了资源和动态库
```

产物在 `target/release/bundle/` 下按类型分目录：`macos/MaaCoC.app`、`dmg/*.dmg`、
`nsis/*.exe`、`deb/*.deb`。

构建前会自动把 `assets/` 和 MaaFramework 的运行库收集进包；可执行文件的 rpath
同时指向自身目录和资源目录，开发和发布两种布局都能加载到库。

CI（`.github/workflows/ci.yml`）在 ubuntu / windows / macos 三个平台上跑格式、
clippy、单元测试和离线识别回归。发布（`.github/workflows/release.yml`）由 tag
触发，产出 macOS `.app`/`.dmg`、Windows 安装包和 Linux `.deb`。

> 尚未做代码签名和公证，第一次打开需要右键 → 打开绕过 Gatekeeper。
> Linux 的 AppImage 暂时没出：runner 上没有 FUSE，`linuxdeploy` 只会报一句
> `failed to run linuxdeploy`，先只发 `.deb`。

## 目录

| 路径 | 内容 |
| --- | --- |
| `engine/` | 引擎库与命令行：设备连接、常驻任务、事件流、识别试跑、离线回归、pipeline 改写 |
| `app/` | Tauri 壳（`src-tauri/` Rust 命令层）+ Material Web 前端（`src/`） |
| `assets/` | 模板图片与 pipeline 定义，被引擎和界面共用 |
| `scripts/` | 拉 SDK、同步/收集动态库、校验产物、无头截图；`refresh-cards/` 见其 README，从游戏更新包重导卡面图 |
| `tests/fixtures/` | 离线回归用的真机语料帧 |
| `src/`、`cmake/`、`CMakeLists.txt` | 旧的 C++ 宿主，仍可用 `cmake -B build && cmake --build build` 构建，但已不是主路径 |

## 界面与配色

界面用 Material Design 3：颜色不是手挑的，而是用 Google 官方
`material-color-utilities` 从一颗种子色（`#F5A623`）按 HCT 算出深浅两套配色，
MWC 组件读同一批 CSS 变量，所以不会出现"这里深蓝那里浅蓝"的漂移。
图标字体和 Roboto 都随包分发，离线也能正常显示。

## 许可

见 `LICENSE`。本项目与 Supercell 无关联。自动化操作存在账号风险，请自行判断。

### 仓库满后停止

挂机刷资源默认在每次出击前检查仓库。只有金币、圣水、黑油全部达到游戏弹窗里的最大储量才停止，不写死容量。读取失败会停止并提示，避免继续刷。设置页可以关闭此检查；命令行使用 `--keep-farming` 关闭。

目前已验证中文界面、1280×720 匹配截图的容量弹窗识别。OCR 模型由 `scripts/fetch-ocr.py` 从 MaaCommonAssets 下载并校验 SHA-256，打包流程会自动拉取。

### 开战收益筛选

设置页可输入最低金币、圣水、黑油及金币＋圣水合计，并选择“任一条件”或“全部条件”。0 表示不限制该项；全部为 0 时任何对手都可开战。筛选默认关闭，运行中不能修改，重新开始后生效。

启用后，在搜索到对手且“下一个”按钮可见时读取可获得战利品：达标才进入原投放策略，不达标搜索下一个；读不清或连续跳过达到设置上限时停止。此条件仅衡量资源收益，不评估防御强度或胜率；已开始的战斗恢复时不会重新筛选。

命令行战术验证：`cargo run --bin maacoc-engine -- run --tactics tactics.json`。
配置示例：
```json
{"enabled":true,"prepareCamera":true,"steps":[{"card":"Unit/0_Barbarian","count":5,"x":150,"y":310,"delayMs":0,"intervalMs":150}]}
```
