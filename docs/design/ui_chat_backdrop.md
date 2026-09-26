# 对话背景:星座渲染器

> **2026-09-26 后续修订：本页构图与 K 图标部分转为历史记录。** 当前消息对话不再绘制任何背景装饰；默认品牌采用三模块 SVG，仅欢迎页 / 语音舞台保留装饰。当前约定与验证入口见 [品牌与界面质感](brand_refresh.md)，不再使用下文的消息沟槽或水印布局。

- 身份: live_design
- 状态: 设计基线(2026-09-26 起草,随 release/2026-09-26-ui2 分支 ui2/bg 实施;同日按独立复核意见修订,本文描述的是修订后已落地的实现)
- 日期: 2026-09-26
- 上游文档: [ui_color_semantics.md](ui_color_semantics.md)(token 与颜色语义门禁)、[app_icon.md](app_icon.md)(标志的设计概念)、[ui_surface_stack.md](ui_surface_stack.md)
- 关联需求: 无(用户 2026-09-26 第二轮 UI 问题清单第 10 条「主对话背景的动画现在很杂乱无章……默认是星座图或者 kanzei 的矢量图转换的类似星座的效果,支持输入图片如何?」;tracker 条目待登记)
- 一句话: 主对话背景从随机「神经场」换成 Canvas2D 星座——默认是 kanzei 标志的笔画转成的星座(边按标志含义分主干 / 记忆 / 行动),另有北斗七星、猎户座、仙后座(真实星表投影)和用户上传图片导出的点集;画在正文两侧的沟槽里(768 列两侧只剩约 120px 时缩成窄沟小徽记),正文列 evenodd 剪掉;只有连窄沟都放不下时才退成水印,水印整张画进离屏层后以单一不透明度合成,正文对比度与叠了几层无关;运行时光点按事件语义沿连线流动,空闲只缓慢闪烁,失败会话静止,隐藏 / 别的视图 / 关闭时零定时器。

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

**(c) 我的图片**:只在本机转换,**只存点集**。File → objectURL → Image 的 load(只解析头部拿尺寸,Chromium 延迟解码)→ **像素数 > 4000 万直接拒绝**(文件体积 20MB 以内的 PNG 也可能是 2 万 × 2 万,RGBA 约 1.6GB,按原始分辨率解码会撑爆渲染进程)→ `createImageBitmap(file, {resizeWidth, resizeHeight})` 直接解码到长边 192(SVG 等不支持时退回 drawImage)→ getImageData → `imageToConstellation` → 立刻释放像素、位图与 objectURL。

- 墨迹:有透明就用 alpha,否则取与边框中位亮度之差(深底浅字、浅底深字都成立),按 p95 归一。
- 判型:倒角距离变换估笔画宽(4 × 墨迹平均距离)。小于短边 14% 且墨迹 < 50% → **中轴线**(标志、文字、线稿:一条笔画一串星);否则 → **轮廓**(照片、剪影:只取 Sobel 边缘,内部不出星)。
- 选点:显著度 × 随机扰动降序的贪心泊松盘,二分半径让点数 ≈ 64。
- 连图:MST → 剪掉 > 2.6 × 中位长的边 → 不足 3 颗的碎簇降为散星 → 剪挂在交汇点上的短毛刺 → 近共线(转角 < 24°)的度 2 节点降为散星 → 轮廓模式再少量补不交叉的近邻边。
- 纯色、均匀半透明、极低对比的图返回 null,界面提示「换一张试试」;大于 20MB、分辨率过高分别提示。实测应用图标 → 中轴线 K(64 星 42 边,落盘 1.7KB)。

## 3. 构图:画在空白处

`layoutBackdrop` 输入都是实测矩形(相对画布),返回 `{box, alpha, placement, avoid, capped}`。测量(≤ 2Hz 轮询 + ResizeObserver + 视图 / 语音 / OC / 偏好事件):

- 区域 = 对话区减去可见的侧面板:旧的 #bg-panel / #agent-panel,以及 panels 组合并后的 #tasks-panel(停靠在对话区外时左缘在画布之外,自然不扣;窄窗口下变成叠在对话区上的抽屉时按左缘扣掉)。
- 正文列 = 活动 pane 最近 16 个子元素的**横向并集**,纵向铺满(消息会滚过整列)。不假设列宽:1080 旧列宽与 chat 组的 768 居中列都按实测走。
- 空态 = art 槽(`.empty-art`)与文案(`.empty-copy`);语音模式 = `.voice-art` 与 `.voice-stage-copy`。

| 模式 | 放哪 | alpha |
|---|---|---|
| 空态 / 语音,槽 > 120×120(OC 开) | 槽内 88%×92% 按 aspect 居中;OC 开着时退到人物身后的上 3/4、右对齐 | 1(人物身后 .7) |
| 空态 / 语音,槽不可见(OC 关 / 容器 < 760) | 文案右侧空白 ≥ 160 就放那里(side) | .9 |
| 对话态,沟槽宽 ≥ 160 | 右沟槽(有 OC 伴侣时止于人物上方 24px),其次左沟槽;框边长上限 360;边距 24 | .85 |
| 对话态,沟槽宽 56~160(窄沟) | 边距收到 16,星座缩成贴着列外侧的小徽记(gutter-narrow);星光小图随框缩到 .55 倍 | .85 |
| 都放不下 | 右上角水印,边长 clamp(160, .3·宽, 320);压在正文上时 capped | 空态 .6 / 对话 .5;capped 见下 |

窄沟是为用户日常几何加的:1333×695@1.5、侧栏展开时对话区约 1005 CSS px,chat 组的 768 列两侧各约 118px。按原来「沟槽 ≥ 160 否则水印」的规则,对话态永远退成右上角水印,亮色上限只有 .047,运行光点几乎看不见(复核意见 4b)。现在这个宽度落在窄沟(browser-smoke P5 实测 `gutter-narrow`),不压正文,alpha 不受对比度上限约束。

**正文保护两道**:

1. `avoid` = 正文列(对话态)或文案(空态 / 语音)外扩 12px。星座框不压正文时,绘制前用 evenodd 把 avoid 从剪裁区挖掉——星尘铺开 1.7 倍框也画不进正文,正文底下一个像素都不画(browser-smoke ⑤ 对沟槽、窄沟、文案旁三种构图逐像素实测为 0)。
2. 星座框本身压在正文上(只剩右上角水印)时 `capped = true`:**整张星座先画进离屏层,再以单一不透明度 `watermarkAlpha` 合成到画布**。离屏层里再怎么叠(星尘、连线、星、尾迹、光点头、点亮的边、失败 hub),alpha 都 ≤ 1、颜色是若干 token 色的凸组合;合成后正文底下任一像素 = chat-bg 与某个混色按 ≤ 上限混合,和叠了几层无关。

原来的做法是逐层夹 alpha,并假设「同一像素最多两层同色」。复核逐像素实测推翻了这个前提:光点经过星点时同一像素叠星尘、星、尾迹、光点头至少 4 层,点亮的边还叠在底线上;1280@1.5 暗色、活动面板开着时合成 alpha 到 .376,--dim 跌到 3.91:1、--err 跌到 4.15:1,而冒烟只建模两层同色,始终是绿的。离屏合成让上限不再依赖层数假设。

上限 `watermarkCap`:先取四种颜色(--backdrop-star / --backdrop-line / --accent / --err)各自单独叠加时的安全上限(`maxOverlayAlpha`:chat-bg 上叠这种颜色后,--fg / --fg-strong / --dim / --accent-text / --ok / --err / --warn 里原本 ≥ 4.5 的每一种都仍 ≥ 4.5)的最小值;亮色主题里「亮度 ≥ 下限」对混色不是凸的,再在 4 色单纯形网格(步长 1/6)上逐个混色复核,不够就二分收紧;最后扣 .01 的 8 位量化余量。不透明度偏好只能往下调(开到 150% 也不越过上限)。

| 主题 | 星点 | 连线 | 光点 | 失败 | 水印上限(扣余量后) | 卡住上限的字色 |
|---|---|---|---|---|---|---|
| 暗色 | .142 | .232 | .304 | .226 | .132 | --dim |
| 亮色 | .091 | .125 | .146 | .115 | .081 | --ok |

离屏层用「水印配比」:最亮的星核为 1、连线提亮 2 倍(星座骨架在很低的总不透明度下仍读得出来)、星尘约 .43,总亮度交给合成那一步。比逐层夹的旧做法更亮(旧做法星点单层 .074 / .047,且星被夹得比连线还暗),层次也保住了。browser-smoke ⑥ 实测(1100×720@1.5 + 活动面板,运行中逐帧审计正文文本矩形下的像素):暗色最坏 --dim 5.0:1、亮色最坏 --ok 4.7:1,alpha ≤ 上限 + 1/255。

空态布局:**OC 关时空态照旧单列,星座放文案右侧空白(side)**,style.css 里 `html[data-oc-enabled="false"] :is(.empty-art, .voice-art, #oc-companion)` 与 `.empty-welcome` 两条规则与基线一字不差。首版为了「左文案、右星座」两栏,把这两条拆成了挂 `html[data-backdrop]` 的三条;复核指出它和 chat 组(kz-ui2-chat 98383b26)对同一处的改法文本上必冲突、语义上也相反(chat 组要「OC 关时空态与列同轴居中」),两条规则同时生效时 .empty-art 在单列网格里成为宽 0、高 300 的第二行,把居中文案顶高约 125px。见 §9 决策。`html[data-backdrop]` 现在只管画布显隐(`off` 时 display:none)。

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
| blocked(本轮失败,停在失败会话上) | 涟漪散完后静止:hub 着淡 --err(`GAIN.hubError` .5),不动、不算忙 |
| run_completed / context_compacted / memory_consolidation_completed / memory_candidate_promoted | 从 hub 按 BFS 层每层 80ms 荡开一圈强调色波 |
| 语音 speaking / listening | 亮星随音量提亮(≤ 1.5 倍) / 一颗 recall 光点(≥ 1.1s 一次) |

- **忙态只认有常驻光点的 thinking / executing / replying**(core `activityBusy`)。首版把「活动态不是 idle」一律算忙,而 `createOcStateStore.current()` 对 runtime.phase=failed 一直返回 blocked,直到下一轮开始:停在失败会话上一直按 26 帧/秒重画、光点 0 个,违反「空闲 ≤ 8 帧/秒」(复核意见 2)。complete 同理。
- hub 着色(core `hubTone`):减少动态效果画静帧时,运行中 hub 着 --accent 表达「在跑」;本轮失败任何模式都着淡 --err;其余不着色。强调色只表示运行中(ui_color_semantics),首版在减少动态效果时把 blocked 也画成强调色,等于把失败画成「在跑」。
- 没有角色的星座(真实星座、图片)光点退回从最亮三颗星之一随机游走 4~6 条边。光点同时 ≤ 4 颗;活动态换了,旧常驻光点走完本程再退场。
- 后台会话的事件只写进 store(切回来活动态仍对),不在当前画面放光点。
- 减少动态效果(matchMedia 带 change 监听):只画静帧;只在布局、主题、偏好、活动态变化时重画。
- 预览页(`html[data-kz-preview]`)固定 t = 2400 画静帧,运行态按确定位置画光点,截图可复现。

## 5. 调度与性能

- 帧间隔:`frameDelay` → 空闲 125ms(≤ 8 帧/秒)、有光点 / 波 / 涟漪 / 补间 / 入场 / 重描 / 忙态时 33ms(≤ 30 帧/秒);setTimeout → rAF,不每个 vsync 唤醒。定时器从**上一帧的 rAF 时间戳**起算、提前 2ms:从 draw 结束才起算时 33ms 常常刚好错过第 2 个 vsync,忙时实际只有 20~24 帧/秒。窗口隐藏、不在对话视图、背景关闭时 null:零定时器、零 rAF。布局观察 `watchDelay` 同样条件下 500ms 一次。
- **已排好的帧不被新事件取消**(core `shouldHurry`):有新东西要画时,只有「还在等定时器、离触发还比忙帧间隔更久、且现在忙」才把定时器提前;已排好的 rAF 与一个忙帧内就会触发的定时器都不动。首版每次唤醒都 cancelFrame 再重排,而 01-core 把 text_delta 按 rAF(约 16ms)合并后逐帧转发 assistant_streaming、kz:text 每个分片也转发:16ms / 25ms 连发时 0 帧/秒,模型流式回复时光点和点亮的边都不动(复核意见 1)。
- 流式分片只写活动态;唤醒渲染器每 33ms(`WAKE_THROTTLE_MS`)至多一次,真有新东西(活动态变了、放了光点 / 涟漪 / 波)立即唤醒。语音 speaking 电平同样节流。
- 以上三道(shouldHurry、唤醒节流、定时器从上一帧起算)任一道单独都扛得住 16ms 连发;browser-smoke 的 bgBrowserStream 变异三道一起拆回首版写法,实测 0 帧/秒。
- **画布换尺寸时同步补画一帧**:换尺寸会清空画布;拖动改窗口尺寸(或 panels 组的分隔条)时 ResizeObserver 每帧都来,首版清空后等不到下一帧,拖动期间画布一直空白。
- 画法:星尘 + 星 = 星光小图 drawImage(星、强调色、失败色三张 64px 径向渐变离屏画布,主题变化时重建);连线一条 path 一次 stroke;**零 shadowBlur**;DPR 上限 1.5;getComputedStyle 只在主题变化后读一次。水印离屏层只覆盖星座周围(框外扩 0.85·S + 28px),与画布同分辨率、对齐设备像素;离开水印构图即释放。
- 实测(headless Edge,browser-smoke):空闲 7.5 帧/秒;每 16ms 一次流式事件 30 帧/秒;失败会话 8 帧/秒;切到设置页 / 窗口隐藏 / 背景关闭 / 减少动态效果 0 帧;60 次改尺寸期间画布始终非空。每帧 0.2ms 量级(首版实测,画法未变)。headless 不代表 WebView2 的 GPU 光栅,真机复核见 §9。

## 6. 偏好与设置页

`~/.kanzei/app.json` 的 `backdrop` 字段(prefs.rs `AppPrefs.backdrop`,`ui_prefs_set` 的独立参数,序列化超过 64KB 整条拒绝、原值不变):

```json
{"enabled": true, "preset": "kanzei|big-dipper|orion|cassiopeia|custom", "density": 1, "opacity": 1,
 "custom": {"v": 1, "kind": "image", "mode": "centerline|outline", "name": "logo.png", "aspect": 0.93,
            "points": [[x, y, mag], …], "edges": [[i, j, role], …], "hub": -1}}
```

- 坐标 3 位小数、星等 1 位;坏数据过 `sanitizeModel` / `normalizeBackdropPrefs`(夹紧坐标、丢越界 / 自环边、role 只收 0-3、≤ 200 点 400 边),preset=custom 而无点集回落 kanzei,绝不抛。
- D-404 模式:本机 WebView2 的 localStorage 重启即丢,`kz-backdrop` 只作缓存。**启动时等后端值(最多 250ms)再第一次发布**,渲染器(`waitForPrefs`)等这次发布才开始画:关掉背景或换了图案的用户每次启动不再先闪一帧默认星座(首版先按默认值发布,193ms 画 kanzei、225ms 才换成 orion)。超时就先用缓存 / 默认值,后端值晚到再覆盖;后端无记录而本地有旧值就迁一次;用户在后端值回来前已经动过设置,以用户为准。状态机是 `createBackdropPrefsStore` 工厂,runtime-smoke 另建实例注入假的 load / publish 断言时序。
- 设置页「对话背景」(#backdrop-settings,在「角色外观与动作」之前):显示背景开关、图案卡片(role=radio 按钮组,缩略图用同一渲染器画静帧;←/↑/→/↓/Home/End 移动即选中,roving tabindex;选中态中性)、上传 / 移除图片、星点密度 0~200%(星尘数量与标志笔画内的暗星)、不透明度 20~150%。开关、图案、图片即时保存;**滑杆拖动中(input)只改内存与画面,松手(change)落盘一次**——app.json 每次都是整文件读写,首版拖一下写十几到二十次盘。选图案或上传图片时背景若关着顺手打开。

## 7. 模块

| 文件 | 职责 |
|---|---|
| ui/22-constellation-data.js | 标志笔画(含角色与 hub)、三个星座的星表与连线、北极星(仅冒烟用)。零 import |
| ui/22-constellation-core.js | 纯函数:投影、MST、补边、矢量 / 图片 → 星座、彗星路线、偏好校验与序列化、构图(含窄沟)、水印上限与合成不透明度、活动态(activityBusy / hubTone)、调度(frameDelay / watchDelay / shouldHurry)。零 import、零 DOM |
| ui/22-constellation.js | Canvas2D 渲染器与调度;水印离屏层;缩略图;图片转换。拿不到 2D 上下文(冒烟假 DOM)时降级为只维护活动态的空实现 |
| ui/22-constellation-prefs.js | 偏好状态机(createBackdropPrefsStore:启动等后端值、滑杆只在松手时落盘)与设置页绑定 |
| ui/22-neural-flow.js | 事件枢纽:`neuralFlowEmit` 在会话过滤之前扇出给 OC 与 `chatBackdrop`(waitForPrefs);NeuralField 只剩记忆页一个实例 |

## 8. 门禁

- `scripts/ui-constellation-smoke.mjs`(纯函数 + 静态契约;单独跑,也由 ui-runtime-smoke 末尾链式 import,失败时 import 本身 reject):① MST 最优 ② 补边不交叉 ③ 北斗星表球面核对 ④ 猎户腰带与预设坐标 ⑤ 标志星座(单连通、臂尖只连自己那条臂、角色边与 hub)⑥ 泊松间距 ⑦ 图片圆环与 null ⑦b 判型(圆盘内部不出星、L 笔画走中轴线)⑦c 彗星路线语义 ⑧ 偏好校验与往返 ⑨ 构图(1600@1.25 + 768 列进沟槽且不交正文、正文列登记为避让区;1280@1.5 水印 capped;**1005 宽 + 768 列进窄沟**;空态进槽;文案右侧;**语音 OC 关放文案右侧**;OC 伴侣避让)⑩ BFS ⑪ 调度 ⑫ 两套主题 token 齐全 ⑬ **水印合成**:按真实的离屏合成逐层复算(具名最坏叠放「光点压在星上」「失败 hub 叠星」「点亮的边叠底线」+ 4000 组随机 1~7 层叠放),原本 ≥ 4.5 的字色全部 ≥ 4.5、不透明度偏好 150% 不越限、上限 ≥ .06;自检:不走离屏逐层直接叠必须有像素跌破 4.5 ⑭ 预设解析 ⑮ 静态契约(每个 globalAlpha 都经 `this.a` / `layerAlpha` / `watermarkAlpha`、capped 必走离屏且只在一处合成、evenodd 剪裁来自 layout.avoid、零 shadowBlur、旧对话变体已删、记忆页神经场保留)⑯ **活动态**(blocked / complete 不算忙,失败 hub 着 --err 绝不着强调色)⑰ **帧不被饿死**(shouldHurry 各分支 + 16ms 连发模拟 ≥ 20 帧/秒)。
  - 变异(`KZ_SMOKE_MUTATE=<id> node scripts/ui-constellation-smoke.mjs`,期望失败):bgParallelBridge、bgOutlineInterior、bgHiddenPause、bgSanitizeBounds、bgTextCap(不透明度偏好放开到 150%)、bgWatermarkMargin(量化余量改成加)、bgBusyActivity、bgHubTone、bgHurryRaf、bgColumnAvoid。
- `scripts/ui-constellation-browser-smoke.mjs`(**新增**;无头 Edge + scripts/ui-preview 预览页,插桩 clearRect 数帧、逐像素读画布;由 ui-lint-smoke 在弹层样例冒烟之后调用,verify 的 ui_lint 步因此覆盖它,检查键集合不变):① 空闲 ≤ 9 帧/秒、失败会话 ≤ 9 帧/秒 ② 其它视图 / 窗口隐藏 / 背景关闭 / 减少动态效果 0 帧 ③ 每 16ms 一次 assistant_streaming ≥ 20 帧/秒 ④ 60 次改尺寸期间画布非空 ⑤ 空态文案旁、对话态沟槽、1333×695@1.5 + 768 列窄沟三种构图下正文文本矩形与避让区下画布 alpha 全为 0 ⑥ 水印(1100×720@1.5,暗 / 亮)运行中逐帧:正文下像素合成到 --chat-bg 后原本 ≥ 4.5 的字色仍 ≥ 4.5、alpha ≤ watermarkAlpha ⑦ 启动时后端存「关闭」:第一次偏好发布之前一笔不画,第一次发布就是关闭。
  - 变异(`KZ_SMOKE_MUTATE=<id> node scripts/ui-constellation-browser-smoke.mjs`,经 page.route 改写被守护的源码,期望失败):bgBrowserIdle、bgBrowserFailed、bgBrowserView、bgBrowserHidden、bgBrowserOff、bgBrowserReduced、bgBrowserStream、bgBrowserResize、bgBrowserClip、bgBrowserWatermark、bgBrowserBoot。
- ui-runtime-smoke「分区:星座背景」:假 DOM 降级不抛;run_started → thinking、tool_started → executing、后台会话事件不改写活动态;设置页四张卡片、role=radio、点「猎户座」即时写 `ui_prefs_set.backdrop.preset`、aria-checked 与 roving tabindex、→ 移到仙后座、关开关后 `html[data-backdrop]=off` 且落盘;**滑杆 input 不落盘、change 落盘一次且带最终值;启动时序(后端「关闭 + 猎户座」时第一次发布就是它、后端回来之前不发布;后端超时先发默认、晚到再覆盖、不回写)**。变异 bgEmitFanout、bgPrefsPersist、bgRadioKeys、bgDatasetSync、bgBootWait、bgSliderPersist。R-285 段的记忆流断言同步为只认记忆页(`const idleAlpha = 0.22;`)。
- prefs.rs `backdrop_tests`:往返、旧 app.json 无字段回落 None、超 64KB 拒绝且原值不变。
- 截图:`scripts/ui-preview/shoot.mjs` 的 `--dpr`(1600@1.25、1280@1.5 按真机缩放)与 `--query`(如 `backdrop=orion`);夹具 `?backdrop=kanzei|big-dipper|orion|cassiopeia|off`;场景 `backdrop` 展开设置页分组,**场景 `voice` 经语音控制器自己的 onState 入口进入语音布局**。

## 9. 决策与未做

- **Canvas2D,不用 pixi**:pixi 只服务 OC 立绘;星座每帧 1 次 stroke + ≤ 300 次 drawImage,0.2ms 量级,不值得多一个 WebGL 上下文。
- **默认是标志星座,不是真实星座**:用户原话把「kanzei 的矢量图转换的类似星座的效果」与星座图并列;标志的三种笔画天然对应三类运行事件,动画因此有含义。
- **水印走离屏层 + 单一不透明度**,不再逐层夹:逐层上限依赖「同一像素最多叠几层」的假设,而光点、尾迹、点亮的边、星、星尘的叠法随几何变化,复核实测已经推翻两层假设;离屏合成把保证变成与层数无关的一个数,同时让水印比逐层夹更亮、层次更对。代价:水印构图下每帧多一次离屏清屏与一次 drawImage(只覆盖星座周围)。
- **窄沟小徽记**,而不是只避让实测文本行的水印:行间空白随消息内容变化,剪裁区每帧都变,且仍要逐像素算对比度;窄沟在列外,剪裁区稳定、alpha 不受限。
- **OC 关时的空态构图(采用复核给的方案 1,需用户确认)**:首版是「左文案、右星座」两栏;chat 组是「与列同轴居中」。两份已批准的方案给出了相反构图,改一边就得改另一边。本分支退回基线规则(OC 关即隐藏 .empty-art、单列),让 chat 组的居中空态生效,星座走已有的「文案右侧空白」(side)分支——复核实测用户几何 1333×695@1.5 叠上 chat 组 CSS 时,框在 x=651、w=324,位于居中文案右侧,不压字;集成时两边文本上不再冲突。若用户更想要两栏,方案 2 是把 chat 组那三条规则限定在 `:not([data-backdrop="on"])` 下、本分支恢复两栏规则。本分支只能改本文档;集成时请把这条取舍同步进 chat 组的设计文档,并按用户意见定稿。
- 集成接缝(给集成方):① prefs.rs 的 `apply_backdrop` 与 panels 组的 `merge_ui_layout` 都插在 `apply_ui_prefs` 之后,是同一个 hunk,两边都保留即可;`ui_prefs_set` 的参数各自独立。② panels 组把 #agent-panel / #bg-panel 合并成 aside#tasks-panel,measure() 已同时识别三个 id。③ chat 组的 `.neural-flow-chat` 列区域 CSS mask 与本渲染器的 evenodd 剪裁是两道独立保护,合并后可并存;若保留 mask,水印构图被挖掉的是列内部分,不影响对比度判据。
- 旁支(不在本任务内):`@container (min-width: 900px)` 里 `#messages:not(:has(… .empty-state)) { padding-right: 222px }` 的特异度压过 `html[data-oc-enabled="false"] #messages { padding-right: 24px }`,OC 关着时正文列仍给伴侣留 222px——chat 组改列宽时一并处理。22-oc-preference.js 的 OC 设置仍只写 localStorage,按 D-404 本机重启会丢。
- 真机复核(不可自动化):装好的 kzapp 在对话页空闲 60 秒,任务管理器里 WebView2 渲染进程 CPU 明显低于旧版;模型流式回复时光点持续流动;切到别的视图或最小化后为 0;开启系统「减少动态效果」后画面静止、运行时 hub 着色、失败会话 hub 淡红。
