# 网页预览面板

> UI2-0926 #8(用户原话「网页渲染的工具有吗？对标GPT的」)。本文件以后端半边(ui2/webbe)为主体;下面「前端」一节由前端半边(ui2/webfe)写,
> 合并时整节接在后端正文之后。

## 前端

- 代码:`crates/kanzei-app/ui/24-preview.js`(停靠面板控制器与对话里的入口),接线点 `00-surface.js`(onSurfaceChange)、`04-markdown.js`(addMarkdownHook)、
  `05-chat-render.js`(fillToolBlock 截图缩略图)、`05-tool-summary.js`(browser 摘要)、`06-activity.js`(交付卡片)、`06-agent-panel.js`(停靠判据)、
  `07-events.js`(写文件后刷新)、`08-compose-runtime.js`(批注进附件)、`09-sessions.js`(切线)、`15-views-misc.js`(大图查看)、`17-files-editor.js`、`21-palette.js`、
  `index.html #preview-dock`、`style.css「分区:网页预览」`、`surface.css`(--surface-safe-right)。
- IPC 按两半约定的契约调用(命令、事件、字段名一字不改),前端没有偏离契约的地方。

### 1. 布局

- `<section id="preview-dock">` 是 `#view-chat` 的最后一个子元素,不另加包裹层。打开时 24-preview.js 写 `#view-chat[data-preview="open"]`,
  对话视图由 flex 列切成两列网格:左列 `#chat-area` / `#turn-activity` / `#composer`,右列面板占满整高。
- 右列宽 = `clamp(360px, var(--kz-split-preview, 48%), max(360px, 100% − 420px))`;左缘 `installSplit(id: "preview")` 可拖,宽度存 `ui_layout.splits.preview`。
  JS 侧同一口径的纯函数是 `previewColumnFor(viewWidth, splitPx)`。
- 对话列的居中:活动行与输入区的宽度原本按 `#view-chat` 的 `100cqi` 算,面板打开时 `#view-chat` 这个容器含着预览栏,
  所以改按网格格子的 `100%` 算;正文 pane 的 cqi 本来就取 `#chat-area`,三者照旧同宽同中轴(`ui-narrow-layout-smoke` 真量,带判据自检)。
- 窄屏:对话视图窄于 800px 时写 `data-preview-narrow="chat|preview"`,单列;「预览」页签面板占满,「对话」页签面板收起(原生面板同时隐藏)。
  工具栏里出现「对话 | 预览」;rail 开关在「对话」页签时切回预览。
- 分隔条手柄骑在面板左边 ±3px,原生面板永远盖在 HTML 之上,所以 `#preview-host` 左侧让出 4px(`inset: 0 0 0 4px`),手柄不被盖住。
- 设备模式(自适应 / 手机 390×844 / 平板 768×1024 / 桌面 1280×800):后端按「设备 × 缩放」居中摆放并 `set_zoom`,`#preview-stage` 露出舞台底色;
  冻结帧按同一口径摆位(`fitDevice(host, preset)`,scale = min(1, 宽比, 高比),居中)。

### 2. 位置与可见性

- `preview_set_bounds{x,y,w,h}`:`#preview-host` 的 `getBoundingClientRect`(CSS px = Tauri 逻辑像素,应用不缩放),保留两位小数。
  触发:ResizeObserver 观察 `#preview-host`、`#view-chat`、`#main`(侧栏开合、后台任务侧栏停靠都会改变它们),窗口 resize,分隔条拖动,
  后台任务侧栏形态变化(`kz:tasks-layout`)。按帧合并,矩形没变不重发。DPI 变化由后端在 `ScaleFactorChanged` 上自己重放。
- `preview_set_visible{visible, processId}`:唯一决策点 `evaluate()`(按帧合并)。露出来的条件:面板开着、页面活着(preview_open 成功过)、
  没有错误页、当前是对话视图、窄屏时停在「预览」页签、没有被遮挡。每次上报都带当前活动线的 `processId`,
  后端据此路由代理的 browser(面板可见且绑定本线才走面板);切线(09-sessions 的两处切换)调 `previewLineSync()` 重报。
  还没有页面时不上报(后端还没有面板)。preview_open 之后不假设可见性,下一帧明确上报一次。
- 收起停靠面板(rail、Ctrl+Shift+B、✕)只隐藏原生面板、页面保留,再打开直接恢复;「更多 → 关闭页面」才 `preview_close` 释放页面进程。
- 启动时按 `ui_layout.preview.open` 恢复面板开合(只恢复到起始页,不自动加载上次的页面:开发服务可能已经不在了)。本机 localStorage 重启即丢(D-404),
  真值是稍后到达的后端偏好:用户本次启动还没动过开关之前,每次偏好到达都按它对齐。

### 3. 遮挡冻结

原生子 webview 永远画在 HTML 之上,z-index 管不着。浮层只有两条活路:

1. **让开**:常驻浮层 `.k-card` / `.k-chip-float` / `.k-toast-region` 的 inset 右值加 `var(--surface-safe-right, 0px)`(卡片、芯片与 `--kz-dock-right` 取大者)。
   24-preview.js 把它写在 `<html>` 上 = 视口右缘到面板左缘的距离(预览 + 其右侧停靠的后台任务侧栏);面板关闭、非对话视图、窄屏占满时为 0。
2. **冻结**:订阅 00-surface 的 `onSurfaceChange`(模态激活、任何关闭、锚定弹层/卡片打开、toast 区域显隐、提示显隐各通知一次)。
   栈里有模态,或任一弹层矩形与 `#preview-host` 相交,或后台任务侧栏的抽屉盖过来,就先 `preview_capture` 截一帧放进 `#preview-freeze`,
   再 `set_visible(false)`;不再遮挡时去抖 120ms 恢复。截图只在面板可见时发(隐藏态截图会挂起,B0 实测),截图失败照样隐藏(露出舞台底色)。
   自家工具栏的菜单(视口与配色、更多)先冻结再开,不会先被原生面板盖一帧。

后果:冻结期间可见性上报为 false,代理的 browser 按路由规则暂时走无头。提示(tooltip)也在栈变化钩子里,工具栏按钮的提示落在面板上时同样会短暂冻结。
门禁:`ui-surface-rules` P 组——三个常驻浮层宿主必须引用 `--surface-safe-right`;`#preview-host` 必须是空元素、style.css 不得给它背景;
`#preview-dock` 里不得出现 `.k-card/.k-chip-float/.k-toast-region/.k-panel/.k-scrim`(面板里的菜单用 openMenu 现造)。

### 4. 地址栏

`normalizeAddress(input)` → `{ kind: "url"|"path"|"invalid", target, display }`,后端再校验一遍:

| 输入 | 结果 |
|---|---|
| `5173` | `http://localhost:5173/` |
| `:3000/x` | `http://localhost:3000/x` |
| `localhost:8080`、`127.0.0.1:4173/app`、`[::1]:8000` | 补 `http://`;`0.0.0.0` 换成 `localhost` |
| `https://…`、`http://…` | 原样(没有路径补 `/`) |
| `example.com/docs` | `https://example.com/docs` |
| `docs/index.html`、`./out/a.svg`、`C:\…\页面.html`、`file:///C:/x/a.html` | path(后端换成 127.0.0.1 静态服务地址) |
| `javascript:`、`tauri:`、`ftp:`、带空格的其它文本 | invalid(toast「无法识别的地址」) |

地址栏与控制台里显示静态服务地址时去掉每次启动都变的 token,只留项目相对路径(`displayUrl`)。最近 5 个打开过的地址存 `ui_layout.preview.recent`。

### 5. 起始页、错误页、控制台

- 起始页(页面还没打开时):说明、`preview_dev_urls{projectDir}` 检测到的本地开发服务(地址 + 启动命令,点一下打开)、最近打开。
- 错误页:`kz:preview-state.error.kind` 为 `connection_refused` 时「服务没在跑？」+ 地址 + `net::ERR_*` 原文 + [重试](preview_nav reload)
  [看后台进程](打开后台任务侧栏,终端条目在那里);`unsafe_port` / `blocked` / `other` 各有说明。错误页显示期间原生面板隐藏。
- 控制台:`kz:preview-console{entries}` 按 seq 去重、最多 500 条;级别筛选 全部 / 错误 / 警告 / 网络;每条带来源(列里 `文件名:行:列`,悬停是完整地址);
  错误数角标挂在控制台按钮上(读屏名称带错误数)。主框架换页时清空并按 `preview_console{sinceSeq: 0}` 重取(后端的环在导航时已清),
  「保留日志」开着时不清(纯前端开关,存 `ui_layout.preview.preserve_log`)。条目文本一律 textContent,不解析 HTML。
  控制台高度可拖(`installSplit(id: "preview-console")`)。

### 6. 对话里的入口

- **工具截图**:后端把模型看到的截图落到 `.kanzei/artifacts/tool-images/<sha>.png`,正文末尾追加 `[tool-image] <路径>`。`fillToolBlock` 摘下标记行
  (摘要器与展开区只看去掉标记的正文),在工具行头之后、折叠详情之前画缩略图(经 `tool_image{projectDir, rel}` 取图,进视口才取,点开进查看器看大图)。
  实时与历史回放同一条路径。折叠的工具组里最后一张截图所在的行常驻可见(与失败行同理)。
- **browser 摘要**:结果首行 `backend: pane（用户可见）` / `backend: headless`,⎿ 行 = 动作 · 主机(或文件名、代码片段)· 面板/无头;
  认新动作 screenshot / press / scroll / wait / eval,参数摘要覆盖 html / key / expression。旧版无头结果(没有 backend 行)照旧「截图 主机」。
  走无头的结果给「在预览中打开」(优先入参 path / html,其次 url——静态服务地址带 token,历史回放时早已失效)。
- **交付卡片**:图片(png/jpg/gif/webp/bmp/svg)显示缩略图(`delivered_image{projectDir, path}`),兑现 deliver 说明里的「图片可内联预览」;`.html/.htm` 加「预览」。
- **代码块**:`renderMarkdownInto` 跑完后经 `addMarkdownHook` 给闭合的 ```html / ```svg 代码块右上角加「预览」→ `preview_snippet{html}` → 在面板打开返回的地址;
  流式写到一半的围栏(data-open)不加。查看器(模态)里的代码块先关查看器再打开面板。
- **链接**:对话里 host 为 localhost / 127.0.0.1 / [::1] 的链接拦下,在面板打开;Ctrl/⌘/Shift+点击照旧交给系统,外网链接不变。
- **文件页**:.html/.htm/.xhtml/.svg 文件的头部出现「在预览中打开」(预览的是磁盘上的版本)。
- **写文件后自动刷新**:07-events 的 kz:tool-end 调 `previewNoteToolEnd`:面板开着本项目的静态页(`/t/<token>/r/…`)时,write / edit / insert / multiedit /
  apply_patch 成功后防抖 300ms 刷新一次;开发服务页靠它自己的 HMR,不刷新。
- 开关:rail `#preview-toggle`(与后台任务开关同组)、Ctrl/⌘+Shift+B(模态开着时让路)、命令面板「网页预览」「在预览中打开当前文件」。

### 7. 批注

「批注」按钮 → `preview_pick{on:true}`(后端进 DevTools 元素选择模式,页面里不注入 UI)。收到 `kz:preview-pick{selector,text,tag,rect,png}`:
局部截图作为 `preview-pick-N.png` 附件进输入框,输入框末尾写上

```
【网页批注】<地址,静态页为项目相对路径>
元素:`<selector>` ("<文本前 60 字>")
修改意见:
```

光标停在最后,不自动发送。再点一次按钮或焦点在面板工具栏上按 Esc 退出(焦点在页面里时由后端回传)。「更多 → 截图放进输入框」同一条附件路径。

### 8. 与后台任务侧栏并存

`#main` 网格第 2 列是后台任务侧栏,预览在对话视图(第 1 列)里,从左到右:对话列 | 预览 | 后台任务侧栏。侧栏的停靠判据
(停靠后对话列 ≥ 600px)算上预览栏:`sideDockMode({ mainWidth: 主区宽 − previewColumnWidth(主区宽 − 侧栏宽), panelWidth })`;
预览开着时多数窗口下侧栏改抽屉,抽屉盖到面板上时按 §3 冻结。预览开合、分隔条拖动时主动 `reconcileTasksPanel()`。

### 9. 门禁与验证

- `ui-runtime-smoke` 分区「网页预览前端」:纯函数(地址规范化 11 正 5 反、设备摆位、截图标记、预览栏宽、browser 目标与摘要、控制台分类)、
  打开面板收到占位框矩形、切视图/切线的可见性与 processId、相交菜单先截图再隐藏且关菜单恢复、不相交菜单不冻结、模态冻结、
  写文件恰好一次刷新(开发服务页不刷新)、批注进附件与输入框、缩略图与「在预览中打开」、交付卡片、侧栏停靠判据与抽屉冻结、
  控制台去重/上限/角标/来源、错误页、Ctrl+Shift+B、代码块、localhost 链接、onSurfaceChange 通知。
  变异守卫 previewHideOnViewSwitch / previewFreezeOnSurface / previewBindProcess / toolImageMarker / previewReloadOnWrite / previewTasksReserve,逐条实跑变红。
- `ui-narrow-layout-smoke` 分区「网页预览前端」:7 种布局真量(两列、面板宽口径、对话列居中不压面板、与停靠侧栏并存、窄屏两个页签)+ 1 条判据自检。
- `ui-surface-rules` P 组 + 4 条反例自测;`ui-preview` 场景 `preview`(`state=page|empty|error`,占位框用棋盘格,样式只注入预览页)。

### 10. 接缝与已知限制

- 事件两侧求差(`ipc-event-smoke`、`ui-runtime-smoke` 的 D-381)要等后端半边的 `kz:preview-*` emit 合入才平;前端半边单独跑时这两处只差这三个事件。
- 冻结 = 隐藏,冻结期间代理的 browser 走无头;工具栏提示落到面板上也会短暂冻结(截图替身,画面不跳)。
- 焦点在原生面板里时,应用快捷键(Esc、Ctrl+Shift+B、Ctrl+P)收不到,要后端经 Runtime.addBinding 回传。
- 100% 缩放、跨显示器、真实系统拖放与焦点抢占只能在安装版手工验收。
