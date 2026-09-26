# 对话背景:星座渲染器

- 身份: live_design
- 状态: 设计基线(2026-09-26 起草,随 release/2026-09-26-ui2 分支 ui2/bg 实施;本文描述的是已落地的实现)
- 日期: 2026-09-26
- 上游文档: [ui_color_semantics.md](ui_color_semantics.md)(token 与颜色语义门禁)、[app_icon.md](app_icon.md)(标志的设计概念)、[ui_surface_stack.md](ui_surface_stack.md)
- 关联需求: 无(用户 2026-09-26 第二轮 UI 问题清单第 10 条「主对话背景的动画现在很杂乱无章……默认是星座图或者 kanzei 的矢量图转换的类似星座的效果,支持输入图片如何?」;tracker 条目待登记)
- 一句话: 主对话背景从随机「神经场」换成 Canvas2D 星座——默认是 kanzei 标志的笔画转成的星座(边按标志含义分主干 / 记忆 / 行动),另有北斗七星、猎户座、仙后座(真实星表投影)和用户上传图片导出的点集;只画在空白处,窄窗口退成按正文对比度夹过 alpha 的水印;运行时光点按事件语义沿连线流动,空闲只缓慢闪烁,隐藏 / 别的视图 / 关闭时零定时器。

## 1. 旧背景为什么乱

`22-neural-flow.js` 的对话区变体(已删除):

- **没有形状**:38 个点用种子随机数撒在右半边,每点连最近 2~3 个,连线是随机弯的二次贝塞尔;10% 的边空闲时也常驻流光。
- **每次都不一样**:画布宽高变化超过 1px 就整张重建拓扑,随机数接着往下走,开合侧栏或活动面板就换一张网。
- **位置不对**:画布铺满对话区,只靠一张右上的径向蒙版压淡,线会穿过「开始一段新对话」和正文;亮色下几乎看不见;节点 0 被拽到 OC 脸上,拉出一条突兀的长线。
- **动画没有含义**:脉冲随机挑边,只靠事件驱动的 energy 衰减——长时间跑 bash 时反而回到静息。
- **有开销**:空闲时 rAF 每个 vsync 都唤醒,几乎每笔都开 shadowBlur;减少动态效果只在启动时读一次;没有开关。

## 2. 三类图案

模型一律归一化到单位框(长边 = 1 − 2·pad,短边居中,记下 aspect),渲染器只做「模型 → 目标框」的仿射映射(X = cx + (x − .5)·S,S = 框长边)。窗口尺寸变化只换目标框,不重建拓扑。

**(a) kanzei 标志(默认)**。数据与 index.html 空态 `.logo-mark`、15-views-misc.js 的 EMPTY_STATE_LOGO 是同一组路径(viewBox 64)。`strokesToConstellation`:

- 端点是亮星;端点离别的笔画 ≤ 0.14·viewBox 时在对方笔画上插一颗 T 字交汇星并连桥;**近平行(< 20°)的笔画不桥接**,否则三条电路臂的臂尖会被连成锯齿。
- 笔画内部按 0.15·viewBox / 密度补暗星(±20% 抖动、垂直抖动 1.8%);距离 < 0.06·viewBox 的星合并;剩余分量用最短跨分量边桥接。
- 边带角色:竖笔 = 项目执行主干(trunk)、三条平行臂 = 记忆层(memory)、右下粗笔 = 行动(action),桥接边无角色;hub = 离 (21, 33) 最近的星,即三层记忆汇入的决策点(见 app_icon.md「设计概念」)。实测 21 星 20 边、单连通。

**(b) 真实星座**:北斗七星(8 星含辅星、7 边)、猎户座(11 星、10 边)、仙后座(5 星、4 边)。J2000 赤经 / 赤纬 / 视星等取 Yale BSC / Hipparcos 公开值;以各星单位向量均值为中心做切平面投影,x = −ξ(东在左)、y = −η(北在上)。冒烟用球面几何核对:天枢—天璇 5.37°、天枢—北极星 28.7°、天璇→天枢大圆离北极星 < 2.5°,猎户腰带三星近似等距共线、参宿四在参宿七左上。

**(c) 我的图片**:只在本机转换,**只存点集**。File → objectURL → Image.decode → 长边 192 的画布 → getImageData → `imageToConstellation` → 立刻释放像素与 objectURL。

- 墨迹:有透明就用 alpha,否则取与边框中位亮度之差(深底浅字、浅底深字都成立),按 p95 归一。
- 判型:倒角距离变换估笔画宽(4 × 墨迹平均距离)。小于短边 14% 且墨迹 < 50% → **中轴线**(标志、文字、线稿:一条笔画一串星);否则 → **轮廓**(照片、剪影:只取 Sobel 边缘,内部不出星)。
- 选点:显著度 × 随机扰动降序的贪心泊松盘,二分半径让点数 ≈ 64。
- 连图:MST → 剪掉 > 2.6 × 中位长的边 → 不足 3 颗的碎簇降为散星 → 剪挂在交汇点上的短毛刺 → 近共线(转角 < 24°)的度 2 节点降为散星 → 轮廓模式再少量补不交叉的近邻边。
- 纯色、均匀半透明、极低对比的图返回 null,界面提示「换一张试试」;大于 20MB 拒绝。实测应用图标 → 中轴线 K(64 星 42 边,落盘 1.7KB)。

## 3. 构图:只画在空白处

`layoutBackdrop` 输入都是实测矩形(相对画布),返回 `{box, alpha, placement, avoid, capped}`。测量(≤ 2Hz 轮询 + ResizeObserver + 视图 / 语音 / OC / 偏好事件):

- 区域 = 对话区减去可见的 #bg-panel / #agent-panel(兼容浮层与停靠两种面板)。
- 正文列 = 活动 pane 最近 16 个子元素的**横向并集**,纵向铺满(消息会滚过整列)。不假设列宽:1080 旧列宽与 chat 组的 768 居中列都按实测走。
- 空态 = art 槽(`.empty-art`)与文案(`.empty-copy`);语音模式 = `.voice-art` 与 `.voice-stage-copy`。

| 模式 | 放哪 | alpha |
|---|---|---|
| 空态,槽 > 120×120 | 槽内 88%×92% 按 aspect 居中;OC 开着时退到人物身后的上 3/4、右对齐 | 1(人物身后 .7) |
| 空态,槽不可见(容器 < 760) | 文案右侧空白 ≥ 160 就放那里 | .9 |
| 对话态 | 右沟槽宽 ≥ 160、高 ≥ 160 就放右沟槽(有 OC 伴侣时止于人物上方 24px),其次左沟槽;框边长上限 360 | .85 |
| 都放不下 | 右上角水印,边长 clamp(160, .3·宽, 320) | 空态 .6 / 对话 .5 |

**正文保护两道**:

1. `avoid` = 正文列(对话态)或文案(空态 / 语音)外扩 12px。星座框不压正文时,绘制前用 evenodd 把 avoid 从剪裁区挖掉——星尘铺开 1.7 倍框也画不进正文,正文底下一个像素都不画。
2. 星座框本身压在正文上(只剩右上角水印)时 `capped = true`:不剪,改为**每一层 alpha 经 `layerAlpha` 夹到上限**。上限由 token 现算:`maxOverlayAlpha` 二分求出「chat-bg 上叠这种颜色后,--fg / --fg-strong / --dim / --accent-text / --ok / --err / --warn 里原本 ≥ 4.5 的每一种都仍 ≥ 4.5」的最大不透明度;再按同一像素最多两层叠加换算成单层上限 1 − (1 − safe)^(1/2)。

| 主题 | 星点 --backdrop-star | 连线 --backdrop-line | 光点 --accent | 涟漪 --err | 卡住上限的字色 |
|---|---|---|---|---|---|
| 暗色 safe / 单层 | .142 / .074 | .232 / .124 | .304 / .165 | .226 / .120 | --dim(6.63) |
| 亮色 safe / 单层 | .091 / .047 | .125 / .065 | .146 / .076 | .115 / .059 | --ok(5.38) |

夹过之后,不透明度偏好开到 150%、两层叠加的最亮像素上,所有字色最低正好 4.50;不夹则星点核心处跌到 1.0~1.2(冒烟 ⑬ 两头都验)。代价是窄窗口的水印在亮色下若有若无——这是「不压正文」换来的,宽窗口放沟槽时不受此限。

空态布局:OC 关、背景开时保留 art 槽,左文案右星座两栏(与 OC 开时同一套 1fr 1.1fr);关掉背景回到单列居中。`html[data-backdrop]` 在 index.html 里静态写 `on`,首帧就是两栏、不闪。style.css 里原来那条 `html[data-oc-enabled="false"] :is(.empty-art, .voice-art, #oc-companion)` 拆成三条:`:is()` 取其中 #id 的特异度,又在 `@layer app` 里,合写会把 `:not([data-backdrop="on"])` 的放行压掉。

## 4. 动效:跟着运行真源走

活动态来自模块内独立的 `createOcStateStore`(与 OC 同一个运行真源:sessionStates + 事件),每帧读一次 `current()`,长工具期间没有事件也保持 executing。

| 活动 / 事件 | 画面 |
|---|---|
| idle | 静息:星按 3.2~7.5s 周期正弦闪烁(暗星幅度大),整体漂移 3px(97s / 131s),星尘反向漂移做视差;每 24~40s 随机一条边收回再重描 |
| thinking | 1 颗常驻光点沿主干自上而下 |
| replying | 同上 1.6 倍速,走过的边短暂提亮 1.2s |
| executing | 1 颗常驻光点从 hub 沿行动那一笔流出;每个 tool_started 追加一颗 action 光点(memory_search 工具追加 recall) |
| memory_search_started / memory_recall_* / research_source_retrieved | recall 光点:从某条记忆臂尖汇入 hub |
| tool_completed ok=false / run_failed / memory_*_failed | hub 上一圈 --err 涟漪,900ms |
| run_completed / context_compacted / memory_consolidation_completed / memory_candidate_promoted | 从 hub 按 BFS 层每层 80ms 荡开一圈强调色波 |
| 语音 speaking / listening | 亮星随音量提亮(≤ 1.5 倍) / 一颗 recall 光点(≥ 1.1s 一次) |

- 没有角色的星座(真实星座、图片)光点退回从最亮三颗星之一随机游走 4~6 条边。光点同时 ≤ 4 颗;活动态换了,旧常驻光点走完本程再退场。
- 后台会话的事件只写进 store(切回来活动态仍对),不在当前画面放光点。
- 减少动态效果(matchMedia 带 change 监听):只画静帧,运行中 hub 着 --accent;只在布局、主题、偏好、活动态变化时重画。
- 预览页(`html[data-kz-preview]`)固定 t = 2400 画静帧,运行态按确定位置画光点,截图可复现。

## 5. 调度与性能

- 帧:`frameDelay` → 空闲 125ms(≤ 8 帧/秒)、有光点 / 波 / 涟漪 / 补间 / 入场 / 重描 / 运行态时 33ms(≤ 30 帧/秒);setTimeout → rAF,不再每个 vsync 唤醒。窗口隐藏、不在对话视图、背景关闭时 null:零定时器、零 rAF。布局观察 `watchDelay` 同样条件下 500ms 一次。
- 画法:星尘 + 星 = 星光小图 drawImage(星、强调色、失败色三张 64px 径向渐变离屏画布,主题变化时重建);连线一条 path 一次 stroke;**零 shadowBlur**;DPR 上限 1.5;getComputedStyle 只在主题变化后读一次。
- 实测(headless Edge,1600×834@1.25):空闲 7.7 帧/秒、每帧 0.20ms(最大 0.30);运行中 26 帧/秒、0.20ms(最大 0.50);切到设置页 / 窗口隐藏 0 帧;减少动态效果 3 秒 0 帧。headless 不代表 WebView2 的 GPU 光栅,真机复核见 §9。

## 6. 偏好与设置页

`~/.kanzei/app.json` 的 `backdrop` 字段(prefs.rs `AppPrefs.backdrop`,`ui_prefs_set` 的独立参数,序列化超过 64KB 整条拒绝、原值不变):

```json
{"enabled": true, "preset": "kanzei|big-dipper|orion|cassiopeia|custom", "density": 1, "opacity": 1,
 "custom": {"v": 1, "kind": "image", "mode": "centerline|outline", "name": "logo.png", "aspect": 0.93,
            "points": [[x, y, mag], …], "edges": [[i, j, role], …], "hub": -1}}
```

- 坐标 3 位小数、星等 1 位;坏数据过 `sanitizeModel` / `normalizeBackdropPrefs`(夹紧坐标、丢越界 / 自环边、role 只收 0-3、≤ 200 点 400 边),preset=custom 而无点集回落 kanzei,绝不抛。
- D-404 模式:本机 WebView2 的 localStorage 重启即丢,`kz-backdrop` 只作缓存;启动先用缓存 / 默认值渲染,`ui_prefs_get` 回来后覆盖;后端无记录而本地有旧值就迁一次;用户在后端值回来前已经动过设置,以用户为准。
- 设置页「对话背景」(#backdrop-settings,在「角色外观与动作」之前,即时保存):显示背景开关、图案卡片(role=radio 按钮组,缩略图用同一渲染器画静帧;←/↑/→/↓/Home/End 移动即选中,roving tabindex;选中态中性)、上传 / 移除图片、星点密度 0~200%(星尘数量与标志笔画内的暗星)、不透明度 20~150%。选图案或上传图片时背景若关着顺手打开。

## 7. 模块

| 文件 | 职责 |
|---|---|
| ui/22-constellation-data.js | 标志笔画(含角色与 hub)、三个星座的星表与连线、北极星(仅冒烟用)。零 import |
| ui/22-constellation-core.js | 纯函数:投影、MST、补边、矢量 / 图片 → 星座、彗星路线、偏好校验与序列化、构图、正文对比度上限、调度延迟。零 import、零 DOM |
| ui/22-constellation.js | Canvas2D 渲染器与调度;缩略图;图片转换。拿不到 2D 上下文(冒烟假 DOM)时降级为只维护活动态的空实现 |
| ui/22-constellation-prefs.js | 偏好读写与设置页绑定 |
| ui/22-neural-flow.js | 事件枢纽:`neuralFlowEmit` 在会话过滤之前扇出给 OC 与 `chatBackdrop`;NeuralField 只剩记忆页一个实例 |

## 8. 门禁

- `scripts/ui-constellation-smoke.mjs`(单独跑,也由 ui-runtime-smoke 末尾链式 import,失败时 import 本身 reject):① MST 最优 ② 补边不交叉 ③ 北斗星表球面核对 ④ 猎户腰带与预设坐标 ⑤ 标志星座(单连通、臂尖只连自己那条臂、角色边与 hub)⑥ 泊松间距 ⑦ 图片圆环与 null ⑦b 判型(圆盘内部不出星、L 笔画走中轴线)⑦c 彗星路线语义 ⑧ 偏好校验与往返 ⑨ 构图(1600@1.25 + 768 列进沟槽且不交正文、正文列登记为避让区;1280@1.5 水印 capped;空态进槽;文案右侧;OC 伴侣避让)⑩ BFS ⑪ 调度 ⑫ 两套主题 token 齐全 ⑬ 正文对比度(本文件独立复算亮度)⑭ 预设解析 ⑮ 静态契约(每个 globalAlpha 都经 `this.a` / `layerAlpha`、evenodd 剪裁来自 layout.avoid、零 shadowBlur、旧对话变体已删、记忆页神经场保留)。
- 变异(`KZ_SMOKE_MUTATE=<id> node scripts/ui-constellation-smoke.mjs`,期望失败):bgParallelBridge、bgOutlineInterior、bgHiddenPause、bgSanitizeBounds、bgTextCap、bgColumnAvoid。
- ui-runtime-smoke「分区:星座背景」:假 DOM 降级不抛;run_started → thinking、tool_started → executing、后台会话事件不改写活动态;设置页四张卡片、role=radio、点「猎户座」即时写 `ui_prefs_set.backdrop.preset`、aria-checked 与 roving tabindex、→ 移到仙后座、关开关后 `html[data-backdrop]=off` 且落盘。变异 bgEmitFanout、bgPrefsPersist、bgRadioKeys、bgDatasetSync。R-285 段的记忆流断言同步为只认记忆页(`const idleAlpha = 0.22;`)。
- prefs.rs `backdrop_tests`:往返、旧 app.json 无字段回落 None、超 64KB 拒绝且原值不变。
- 截图:`scripts/ui-preview/shoot.mjs` 新增 `--dpr`(1600@1.25、1280@1.5 按真机缩放)与 `--query`(如 `backdrop=orion`);夹具 `?backdrop=kanzei|big-dipper|orion|cassiopeia|off`;场景 `backdrop` 展开设置页分组。

## 9. 决策与未做

- **Canvas2D,不用 pixi**:pixi 只服务 OC 立绘;星座每帧 1 次 stroke + ≤ 300 次 drawImage,0.2ms 量级,不值得多一个 WebGL 上下文。
- **默认是标志星座,不是真实星座**:用户原话把「kanzei 的矢量图转换的类似星座的效果」与星座图并列;标志的三种笔画天然对应三类运行事件,动画因此有含义。
- **水印的单层上限按两层叠加算**:星核与星尘、光点与尾迹会在同一像素叠加;按一层算会在叠加处跌破 4.5。亮色下水印因此很淡(见 §3 表),可见性下限在冒烟里守 ≥ .04。
- **空态两栏**是用户可见的布局变化(OC 关、背景开时);关掉背景即恢复单列。
- 旁支(不在本任务内):`@container (min-width: 900px)` 里 `#messages:not(:has(… .empty-state)) { padding-right: 222px }` 的特异度压过 `html[data-oc-enabled="false"] #messages { padding-right: 24px }`,OC 关着时正文列仍给伴侣留 222px——星座按实测列宽走,正好落进这块空白;chat 组改列宽时一并处理即可。22-oc-preference.js 的 OC 设置仍只写 localStorage,按 D-404 本机重启会丢。
- 真机复核(不可自动化):装好的 kzapp 在对话页空闲 60 秒,任务管理器里 WebView2 渲染进程 CPU 明显低于旧版;切到别的视图或最小化后为 0;开启系统「减少动态效果」后画面静止、运行时 hub 着色。
