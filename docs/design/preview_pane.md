# 网页预览面板与 browser 工具双后端(UI2-0926 #8)

- 身份: live_design
- 来源:用户 issue #8「网页渲染的工具有吗？对标GPT的」;对标 ChatGPT / Codex 桌面端内置浏览器、Browser Use、HTML 文件预览与批注模式。
- 范围:本文件是后端(Rust 与代理工具)的设计与契约真源;前端章节在文末「## 前端」,由前端实现补充。
- B0 技术验证:一次性 Tauri 小程序(与 kanzei 同版本 tauri 2.11.5 / wry 0.55.1 / webview2-com 0.38.2)在本机全自动跑完,结论与计划调整见 §9,未能自动验证的项目见 §10。
- 独立复核的 3 条 major 与 11 条代码 / 文档类 minor 已全部落地,正文里标「复核修复」;事件次序的实测依据见 §9.1,新增的人工验收是 §10 的 10–17 条。复核的另一条是合并衔接提示:本分支必须与前端分支在同一次集成里合入(单独合入时 ipc-event-smoke 与 ui-runtime-smoke 必然因前端订阅缺席而失败),本文保留后端正文到「## 前端」之前,由前端一节接在文末。

## 0. 目标与不做

四件事:

1. 用户可见、可操作的应用内网页面板(子 webview,聊天区右侧停靠)。
2. 代理的 `browser` 工具能驱动这个面板,截图与 console 回给模型(用户看得见代理在做什么)。
3. 模型产出的 HTML(文件或代码块)一键渲染:本地文件与片段经 127.0.0.1 静态服务提供。
4. 批注模式:在页面上点选元素,选择器、文字与局部截图作为附件交给代理。

不做:React/JSX 片段在线编译(React 项目直接预览开发服务);多标签页;代填表单与登录外站;手机端 PWA 里的预览。

## 1. 技术选型

- **子 webview,不用 iframe**:外站普遍带 X-Frame-Options / CSP frame-ancestors;跨源 iframe 读不到 console、DOM,截不了图;file:// 不能嵌进 http 源。子 webview 是顶层浏览上下文,没有这些限制。
- `Window::add_child` 需要 tauri 的 `"unstable"` feature(`crates/kanzei-app/Cargo.toml`)。调用集中在 `src/preview/pane.rs` 一处;Cargo.lock 锁在 2.11.5,升级 tauri 小版本时重跑 §9 的检查清单。开了 unstable 之后主 webview 也按 `WindowChild` 创建、按比例自动铺满窗口(tauri-runtime-wry 在没有显式边界时给 0/0/1/1 的 auto-resize),尺寸行为与之前一致。
- **unstable 的副作用(复核发现,对所有用户生效,与是否开过面板无关)**:wry 只对**非** child 的 webview 给父窗口挂子类化(wry 0.55.1 `webview2/mod.rs:539 if !is_child`),主 webview 变成 child 后丢了两件事——`WM_SETFOCUS → MoveFocus`(Alt+Tab / 点任务栏切回后键盘焦点停在顶层窗口,要先点一下页面才能打字;tauri 在第一次 add_child 之前连 Focused 事件都不往外发)与 `WM_MOVE / WM_MOVING → NotifyParentWindowPositionChanged`(移动窗口后原生 `<select>`、右键菜单可能按旧位置弹出)。`src/preview/host.rs` 在主窗口上补挂同样的子类化(`windows-sys` 的 SetWindowSubclass,启动装配 `preview::install` 里挂上):焦点还给上次拿到焦点的 webview(默认主界面;用户点进过、仍可见的面板优先,两边的 GotFocus 维护),且**只在主窗口就是前台窗口时**才转交(B0 实测 MoveFocus(PROGRAMMATIC) 会抢前台);移动时对主界面与面板都发通知;比 wry 原版少了 WM_ENTERSIZEMOVE 时抢焦点那一条。守卫测试钉住这几处调用与「先查前台再转交」的次序。
- **进程内 CDP**:`webview2-com` 的 `CallDevToolsProtocolMethod` / `GetDevToolsProtocolEventReceiver`,不开 `--remote-debugging-port`,生产环境不留调试口(D-319 的端口问题与此无关)。版本与 wry 已拉入的一致(webview2-com 0.38.2、windows 0.61.3),Cargo.toml 里注明这是「只用 windows-sys」约定的唯一例外。
- 线程模型:async 命令 → `with_webview` 把闭包送到 UI 线程 → UI 线程发起 COM 调用 → WebView2 在 UI 线程回调 → oneshot 回到 async,带超时(默认 10 秒,截图 5 秒)。UI 线程从不同步等待;超时后才到的完成回调发给已丢弃的接收端,静默作废。
- **DevTools 只对面板**:不开 tauri 的 `devtools` feature(release 下会让主界面也带 Inspect),也**不**调 `SetAreDevToolsEnabled(true)`(F12 / Inspect 保持关闭)。「打开 DevTools」只经 `cdp::open_devtools(&PaneWebview)`——`PaneWebview` 只有 label 形如 `preview-<代次>` 才造得出来,调用前再按 label 复核一次。守卫测试:全仓代码行里 `OpenDevToolsWindow(` 只出现一次且只在这个函数里。

## 2. 安全模型

- **Tauri IPC 的 ACL**:远程源(没有 remote capability)调不了任何 app 命令。B0 实测:127.0.0.1 页面里 `window.__TAURI__` / `__TAURI_INTERNALS__` 是存在的(withGlobalTauri 注入),但 `invoke('spike_secret' | 'spike_ping' | 'plugin:event|emit' | 'plugin:app|version')` 一律 `Command X not allowed by ACL`,spike_secret 没有执行。
- **坑:自注册的 custom protocol 是本地源**。Windows 上形如 `http://<proto>.localhost`,Tauri 的 `is_local_url` 把它当本地源;kanzei 没有 app ACL manifest,本地源能调全部命令。所以:
  - 本地文件与片段**一律**走 127.0.0.1 静态服务(远程源),不用 `register_uri_scheme_protocol`;
  - **导航闸必须有**(`preview::nav_allowed`,on_navigation / 地址栏 / 代理导航共用):http(s) 的任何 `*.localhost` 子域一律拒绝(tauri / ipc / asset 及其它协议;尾点先剥再判),`localhost` 本身放行;`about:blank`、`data:text/html` 放行;`file:`、`javascript:` 与其它 scheme 拒绝。
  - B0 **对照组**:一个不设闸的子 webview 导航到 `http://tauri.localhost/` 后调 `invoke('spike_secret')` 拿到了 `SECRET-LEAKED`。闸不是可选项。
  - 闸同样覆盖程序化的 `Webview::navigate(tauri.localhost)` 与程序化的 `file:`;页面发起的 `file:` 在 on_navigation 之前就被 Chromium 自己拦了。
  - **子 frame 同样过闸**(复核修复):on_navigation 对应 NavigationStarting,只管顶层;而 wry 注册 custom protocol 用 `SOURCE_KINDS_ALL`,子 frame 请求 `http://tauri.localhost/…` 同样被应用协议接管,tauri 的 `is_local_url` 又只按 scheme + domain 判、不看端口(`http://tauri.localhost:3000` 的内容来自本机 3000 端口却算本地源),远程页面里还注入了带 invoke key 的 `__TAURI_INTERNALS__`。所以建面板时在 ICoreWebView2 上挂 `FrameNavigationStarting`(`cdp::guard_frame_navigation`),按 `preview::frame_nav_allowed` 判定后 `SetCancel(true)`:http(s) 同样拒绝任何 `*.localhost` 子域;放宽的只有页面内部常见、又拿不到新源的 `about:`(blank / srcdoc)、`data:`、`javascript:` 与内层源过同一规则的 `blob:`;其余 scheme 拒绝。
  - 被拦的导航(页面里的 `mailto:`、`blob:` 链接、`*.localhost` 子域的开发地址……)只记一条 **warning** 级控制台条目(「已拦截导航」/「已拦截子框架导航」),**不**改页面级错误态——当前页还好好的,不能被错误页换掉。
  - `on_new_window` 一律 `Deny`(window.open 返回 null),放行的地址改在本面板里打开。
- **站点数据**:面板与主界面共用同一个 WebView2 profile。「清除本站数据」只做 CDP `Storage.clearDataForOrigin{origin: 当前源, storageTypes: "all"}`;清全部浏览数据的 API 会连 kanzei 自己存在 localStorage 里的东西一起清掉,守卫测试保证全仓代码不调用它。
- **browser 工具权限分级**(`kanzei-tools/src/base.rs`):资源由 `browser_tool::resources_for` 按**解析后的 host** 生成(`http://localhost:80@evil.com/` 的 host 是 evil.com):
  - `url:localhost[:*|/*]`、`url:127.0.0.1[:*|/*]`、`url:[::1][:*|/*]`、`path:*`、`html:*`、`page:none`(还没打开任何页面)→ Allow;
  - 其余外网 → Ask(与 webfetch 一致);只读档位的硬 deny 不变;
  - 不带目标的动作(click / dom / eval …)按「将要执行它的后端」的当前页取资源;
  - 用户自己在面板里导航不走权限。

## 3. 静态服务(`kanzei-tools/src/preview_server.rs`)

- 只绑 `127.0.0.1:0`,进程内单例,std TcpListener 手写,每连接一个线程;桌面与 CLI 共用。启动失败不缓存,下次重试;失败时 browser 的 `path` 回落 file:// 并在结果里说明。
- 每次运行生成 128 位随机 token(OS 随机键的 SipHash + 时间 + 进程号,SHA-256 压成 32 hex)作路径前缀:
  - `/t/{token}/r/{root_id}/{rel}`:登记过的根;文件落在哪个候选根(代码树 cwd、项目主根)里就以它为根,都不在时以文件所在目录为根;
  - `/t/{token}/s/{id}.html`:内存片段,LRU 32 个,单个 ≤ 2 MB,同内容去重。
- **root_id 是随机的**(复核修复):所有根与片段同源(127.0.0.1:port)、共用 token,页面自己的 URL 里就带着 token;此前 root_id 取路径哈希、可以推算,经本服务打开的任何页面(项目里的第三方 HTML、聊天里的片段)都能拼出别的已登记根的 URL 去读 `.env` 之类的文件再发出去。现在登记表存「(目录, 单文件) → 随机 16 hex」,同一登记重复登记拿回同一 id,不同登记、不同运行各不相同;页面拿不到自己根以外的 id。同一个根内的文件仍然互相可读(与在该目录里起一个 `python -m http.server` 相同),这是有意的:多文件页面要能取到自己的资源。
- **兜底根不能太宽**(复核修复):目录是盘符根、用户主目录本身或其上级(`C:\Users`)时,不整个当根,只登记**单文件根**(只提供这一个文件,同目录其它文件与 Referer 逐级查找一律 404);候选根本身太宽时同样跳过。打开桌面上的 HTML 仍以桌面目录为根(桌面在主目录之下,不算太宽)。
- 只接受 GET / HEAD(其余 405);百分号解码(中文路径、含空格的 `kanzei code`);逐段拒绝 `.`、`..`、含 `\`、`:`、NUL 的段;canonicalize 之后必须仍在根内(符号链接越界同样 404)。
- 目录不带尾斜杠 → 301 补斜杠;目录没有 index.html → 404,不列目录;`Cache-Control: no-store`、`X-Content-Type-Options: nosniff`,不加 CORS 头。
- 根路径引用(Vite 构建产物的 `/assets/x.js`):只在 Referer 指向本服务某个根内页面时,从引用页所在目录逐级向上找同名文件,仍限定在同一个根内;没有图标时 `/favicon.ico` 回 204,不在控制台留 404 噪声。

## 4. 面板生命周期(`src/preview/pane.rs`)

- label 每次创建用 `preview-<代次>`:关掉的面板在 tauri 的 webview 表里注销是异步的,同名立刻重建会撞「label 已存在」。
- 创建在 async 命令里:`spawn_blocking(|| window.add_child(builder, 逻辑位置, 逻辑尺寸))`,10 秒超时。builder:`.focused(false)`、`.disable_drag_drop_handler()`、`.with_environment(主 webview 的 ICoreWebView2Environment)`、`on_navigation`(导航闸)、`on_new_window`(Deny + 本面板打开)、`on_page_load`、`on_document_title_changed`;不设 additional_browser_args / data_directory / incognito / proxy。
- 活性:add_child 的 Ok 与 `window.webviews()` 都不可信(B0);以 `with_webview` 回环兼 CDP 订阅作为活性检查,失败就关掉空壳并如实报错。先订阅事件、再挂子 frame 导航闸、再 `Runtime/Log/Page/Network.enable`、再 `Page.getFrameTree` 记下主 frame id、再登记焦点归还用的 controller,最后才导航(B0 的顺序)。初始化任何一步失败:先经 `run_on_main_thread` 释放这一代在 UI 线程上登记的事件接收器与 controller,再关空壳(复核修复:此前 enable 失败只关 webview,接收器泄漏)。
- 创建中被关闭(复核修复):创建要几百毫秒,期间用户点关闭时 slot 还是空的。`PreviewState.close_epoch` 在每次 `close` 时递增;ensure 建好后在同一把 slot 锁里比对,变了就当场丢弃新面板(释放 + 关闭),不写进 slot、不显示,preview_open 返回「在创建过程中被关闭了」。
- 边界:前端上报 `#preview-host` 的矩形(CSS px = Tauri 逻辑像素,应用没用 zoom)。设备模式用 `preview::fit_device(host, preset) → (边界, 缩放)`:缩放 = min(1, host 宽 / 设备宽, host 高 / 设备高),下限 0.25(WebView2 ZoomFactor 的下限,超出部分裁到 host 内),边界 = 设备 × 缩放、在 host 里居中,再 `Webview::set_zoom(缩放)`;Fill 铺满 host、缩放设回 1(ZoomFactor 跨导航、跨源保持,不设回会一直缩着)。手机 / 平板另开 `Emulation.setTouchEmulationEnabled`;深浅色用 `Emulation.setEmulatedMedia` 的 `prefers-color-scheme`;每次主 frame 导航后重放一次。
- DPI:`WindowEvent::ScaleFactorChanged` 时按记住的 host 重放边界(DOM 的 ResizeObserver 在纯 DPI 变化时不触发)。
- 可见性:`preview_set_visible{visible, processId}`,每次上报都带当前线。**显示时按上报值覆盖绑定**(`processId: null` 即解绑:用户此刻看的上下文没有线,就不该让上一条线的代理驱动这块面板);隐藏时保留绑定并记下隐藏时刻(路由据此区分「刚被遮住」与「早就收起」,见 §5)。preview_open 视同显示,同一规则。主窗口 `CloseRequested` / `Destroyed` 时关面板;关面板释放 UI 线程上的 CDP 接收器与焦点登记。
- 导航失败(「服务没在跑?」)**只认主 frame**(复核修复):
  - `Network.loadingFailed{type: Document}`(非取消、非 ERR_ABORTED)**不分帧**——外站的广告 / 跟踪 iframe 被 WebView2 跟踪防护拦下(ERR_BLOCKED_BY_CLIENT)、iframe 目标带 `X-Frame-Options: DENY`(ERR_BLOCKED_BY_RESPONSE)、iframe 指向死端口(ERR_UNSAFE_PORT),都会来一条。此前直接据此进错误态,前端会把完好的页面藏起来换成错误页,代理 open 也会提前以失败结束,外站几乎必现。
  - 现在失败只**暂存**(requestId → errorText,最多 16 条);只有主 frame 真的提交了错误页(`Page.frameNavigated` 无 parentId、`url = chrome-error://chromewebdata/`、带 `unreachableUrl`)才进入错误态、`fail_seq + 1`、清控制台后记一条 network 级「页面加载失败」。错误页的 `frame.loaderId` 就是那次失败请求的 `requestId`(Edge 实测,主帧与子帧同一规律,见 §9.1),据此取具体错误码;取不到按 `net::ERR_FAILED`(kind=other),错误页先于 loadingFailed 到达时,后到的同 id 失败再补上具体码。
  - **错误态只在导航开始时复位**:on_navigation 放行分支(含后退 / 前进 / 刷新)、preview_open、代理导航。wry 把 `PageLoadEvent::Started` 挂在 ContentLoading 上,而 WebView2 对 chrome-error 错误页同样触发 ContentLoading(B0 的死地址有一条 started),所以 Started 只更新 url 与 loading、**不碰** error——否则两种事件次序下 connection_refused 都会被抹掉。主 frame 正常提交时清错误。
  - 纯状态机在 `PaneMeta::apply` / `begin_navigation` / `content_loading`,次序单测喂的是 Edge 实测的事件序列。
- SPA 路由(复核修复):订阅 `Page.navigatedWithinDocument`,只认主 frame(创建时 Page.getFrameTree 取 id,之后每次主 frame 提交刷新;id 未知时一律不认),更新 url 并刷新前进后退——否则 pushState 之后地址栏、代理结果里的 url、「在系统浏览器打开」用的都是旧地址。
- 截图:只在可见时截(隐藏时 `Page.captureScreenshot` 永不返回),刚被遮住时先等最多 1.5 秒让它露出来,5 秒超时;整页截图高度上限 16384 CSS px。设备模式下截的是面板显示尺寸(缩放后的画面),不临时改 device metrics——那会让用户眼前的画面闪一下(B0 计划调整 5 的二选一,取后者)。
  - **clip 按缩放换算**(复核修复):captureScreenshot 的 clip 单位是 DIP,元素矩形是 CSS px;ZoomFactor = z 时 1 CSS px = z DIP,四个量都乘 z(`pane::clip_params`),否则设备模式下元素截图与批注裁图会裁偏。
  - 设备模式(ZoomFactor ≠ 1)下**不做整页截图**:缩放下的整页几何(cssContentSize × z、captureBeyondViewport)没有在 WebView2 上验证过。preview_capture 明确报错;代理的 screenshot 退回可视区并在结果里写明原因。
- 批注:`preview_pick{on}` → `DOM.enable`、`Overlay.enable`、`Overlay.setInspectMode{searchForNode}`;`Overlay.inspectNodeRequested` → `DOM.resolveNode` → `Runtime.callFunctionOn` 取选择器 / 文字(≤200 字)/ 标签 / 视口矩形 → 退出选择模式 → 按元素矩形外扩 8px(换成文档坐标,再按缩放换成 DIP)截图 → 发 `kz:preview-pick`。

## 5. 代理路由与 browser 工具

- **一个工具名,两个后端**,不新增工具名(免得弱模型在两个相近工具之间选错)。kanzei-tools 的 `browser` 是无头版(CLI 与桌面兜底);桌面端在 FrontendToolsComponent 注册同名同 schema 的 DesktopBrowserTool,同名 insert 后注册者胜。
- **路由** `preview::route(meta, ctx.process_id)`:面板存在且活着、**正在显示**、绑定了线且就是这次调用的线 → 面板;其余一律无头。代理第一次调用 browser 不会自动弹出面板。
- **遮挡冻结不算收起**(复核修复):前端在菜单 / 模态压到面板上时冻结成截图并 `set_visible(false)`,此前同一条线的下一次 browser 调用就切到无头(另一张页面、另一份状态),正在进行的面板截图也会吃满 5 秒超时,open→click→截图 这样的序列可能中途换后端。现在执行前走 `route_waiting`:面板活着、绑定本线、没有错误页、只是**隐藏不到 10 秒**(`OCCLUSION_GRACE_MS`)时,最多等 1.5 秒(`VISIBLE_WAIT`)让它露出来再定后端;隐藏超过 10 秒视为用户收起了面板,直接无头,不给每次调用白加延迟。面板后端内部(agent::execute、截图前)同样最多等 1.5 秒。权限判定用的后端预估 `route_hint` 与之同一结论(刚被遮住的面板按面板的当前页判权限)。
- **子代理**:子代理复用父线的 ctx(含 process_id),ToolCtx 里没有子代理标记,而 localhost / path / html 现在是 Allow,所以子代理**同样可能**路由到父线绑定的面板并驱动它(用户看得见)。它与父线的调用经 `PreviewState.agent` 这把锁串行,一次动作(导航 + 等 load + 截图)不会与另一次交错;但父线与子代理交替调用时,面板停在谁最后打开的页面上——需要各自独立页面的并行子代理请让它们用无头(例如父线先收起面板)。
- **结果首行**写明后端:`backend: pane（用户可见）` 或 `backend: headless`;其余格式由 `kanzei_tools::browser` 的 `out_*` 函数统一生成,两端逐字同形。桌面端走无头时,open / screenshot 结果末尾加一行提示:用户可点卡片上的「在预览中打开」。
- 动作:open(导航并截图)、screenshot(不重新导航;full_page 或 selector)、dom(共用 `scripts/browser-dom-walker.mjs`:helper 侧 import,Rust 侧 include_str! 去掉 export 后包进 `Runtime.evaluate`;表单值也回显,密码框除外)、console(默认错误 / 警告,有即判工具错误;all=true 返回最近 200 条全部级别)、click(面板:scrollIntoView + 元素中心 `Input.dispatchMouseEvent`)、type(聚焦并清空后 `Input.insertText`)、press(`Input.dispatchKeyEvent`,支持 Control+ / Shift+ / Alt+ / Meta+ 修饰)、scroll(selector 或 dy)、wait(selector / text / ms,≤ 10 秒,100 ms 轮询)、eval(`Runtime.evaluate{returnByValue, awaitPromise}`,JSON ≤ 8000 字符)。未知动作报错并列出合法值(旧实现把未知动作当 open,会悄悄重新导航)。新参数:html、key、expression、dy、ms、full_page、all、color_scheme;viewport 增加 tablet-768x1024、desktop-1280x800(面板后端映射成面板的手机 / 平板 / 桌面设备)。
- 无头辅助脚本位置:`KANZEI_BROWSER_HELPER` → `<exe 目录>/scripts/browser-helper.mjs`(tauri bundle resource,见 tauri.conf.json)→ 构建时仓库路径;都没有时报错写全三处。安装目录旁没有 node_modules:helper 先按常规解析 playwright-core,失败再从 `KANZEI_PLAYWRIGHT_ROOT`(Rust 侧在构建时仓库有 playwright-core 时传入)解析;都没有时报错说明「面板开着时不需要它」。console 行列统一 1 起算。
- **截图回显**(`kanzei-core/src/runner/tool_images.rs`):统一出口 `materialize_tool_output` 把 `output.images` 按内容 SHA-256 写到 `.kanzei/artifacts/tool-images/<sha>.png`(同内容去重),并在外置 / 截断**之后**把 `[tool-image] .kanzei/artifacts/tool-images/<sha>.png` 追加为 content 末行——实时显示与历史回放是同一机制(回放读的就是消息里的 ToolResult content),模型也看得到路径、可以直接 deliver;大结果被外置时标记也不会丢。与 D-349 大结果外置同属一个 2 GiB 配额(tool-results + tool-images 合计、同一把配额锁);超额或拿不到锁时只是不落盘、不加标记。清理:每个项目根在本进程第一次落图时(即应用启动后第一次)按「最近 300 张、14 天内」清一次,之后每新写 50 张再清一次。browser、ui_screenshot、plot、latex 自动受益;provider 不支持图片时模型收不到图,但对话里照样回显。
  - **只落 PNG**(复核修复,取「方案一」):契约与前端(`24-preview.js` 的 `TOOL_IMAGE_REL`、`tool_image` 命令)只认 `<sha>.png`,此前 jpeg / webp / gif 也写出 `.jpg/.webp/.gif` 并加标记,合并后对话里会露出一行认不出的 `[tool-image] ….jpg`。截图类工具产出的都是 PNG;别的格式不落盘、不加标记(模型照常收到图)。
  - **`read` 不落盘**:read 读图片文件时源文件本来就在盘上,每读一张就复制一份只占配额。
  - **复用刷新修改时间**:清理按修改时间排序判龄;同内容再次被引用时把修改时间刷成现在,免得一张 15 天前写过、今天又出现在新消息里的截图在下一轮清理时被删、缩略图随即失效。

## 6. IPC 契约

单位:所有矩形都是 `#preview-host` 的 CSS px(= Tauri 逻辑像素)。形状钉在 `scripts/ipc-contract.json`(键名即命令 / 事件名),由 `crates/kanzei-app/src/ipc_contract.rs` 的「分区:网页预览后端」用例对照真实构造比对。

命令(全部 async,参数按 Tauri 惯例 camelCase):

| 命令 | 参数 | 返回 |
|---|---|---|
| `preview_open` | `target, processId, bounds:{x,y,w,h}` | kz:preview-state 同形 |
| `preview_set_bounds` | `x, y, w, h` | — |
| `preview_set_visible` | `visible, processId`(显示时按它覆盖绑定,null 即解绑;隐藏时保留绑定) | kz:preview-state 同形(没有面板时为关闭态) |
| `preview_nav` | `action: back \| forward \| reload \| stop` | — |
| `preview_close` | — | — |
| `preview_capture` | `fullPage?, clip?:{x,y,w,h}`(clip 为页面文档坐标的 CSS px,后端按缩放换算;设备模式下 fullPage 报错) | `{png(base64), width, height}` |
| `preview_console` | `sinceSeq` | `{entries:[{seq,ts,level,text,url,line,col}]}` |
| `preview_console_clear` | — | — |
| `preview_device` | `preset: fill \| phone \| tablet \| desktop, scheme: auto \| light \| dark`(缺省的一项保持不变) | kz:preview-state 同形 |
| `preview_pick` | `on` | — |
| `preview_snippet` | `html` | `{url}` |
| `preview_dev_urls` | `projectDir` | `{urls:[{url, command, pid}]}` |
| `preview_clear_site_data` | — | — |
| `preview_open_devtools` | — | — |
| `preview_open_external` | — | —(当前 http(s) 地址交给系统默认浏览器) |
| `tool_image` | `projectDir, rel`(只收 `.kanzei/artifacts/tool-images/<64 hex>.png`) | `{png}`(base64,≤ 8 MB) |
| `delivered_image` | `projectDir, path`(沿用 open_delivered_path 的根内校验,只收图片扩展名) | `{png}` |

`preview_open` 的 target 规范化(`preview::normalize_target`):`5173` → `http://localhost:5173/`;`:3000/x`;`localhost:8080` / `127.0.0.1:4173` / `[::1]:5000` 补 `http://`;带 scheme 的过导航闸(file:// 转本地文件);盘符 / UNC 绝对路径、`./ ../ /` 开头或在当前线的根(工作树线先工作树、再主根;默认线 `d|<root>`)里存在的相对路径 → 静态服务 URL;像域名的补 `https://`;其余报错。

事件(三个都没有会话归属,前端登记进 `01-core.js` 的 `SESSIONLESS_EVENTS` 并经 `on()` 订阅;`scripts/ipc-event-smoke.mjs` 的「分区:网页预览后端」核对后端 emit、前端订阅与登记三处齐全):

- `kz:preview-state`:`{url, title, loading, canBack, canForward, visible, boundProcessId, device, scheme, error?:{kind, text}}`,`kind ∈ connection_refused | unsafe_port | blocked | other`;没有错误时省略 `error`。`error` 只表示**主 frame** 提交了错误页(iframe 失败、导航闸拦下的链接都不算,后者记 warning 级控制台条目);下一次导航开始时清除。`url` 随 SPA 的 pushState 更新。面板关闭时发 `visible:false, url:""` 的关闭态。
- `kz:preview-console`:`{entries:[...]}`,≤ 每 250 ms 合并推送一次,只推新条目(seq 单调、清空后不回退)。`level ∈ log | info | debug | warning | error | network`;`network` = 资源加载失败(Log.entryAdded source=network 的 4xx / 5xx 与 net::ERR_*、主文档加载失败),属于失败一类;`line / col` 1 起算,未知时为 null;`url` 未知时为空串。主 frame 导航后后端缓冲清空,前端据 kz:preview-state 的 url 变化决定自己的列表清不清(「保留日志」是前端行为)。
- `kz:preview-pick`:`{selector, text, tag, rect:{x,y,w,h}, png}`(rect 为视口 CSS px;裁图失败时 png 为空串)。

工具结果:首行 `backend: pane（用户可见）` 或 `backend: headless`;截图落盘后末行 `[tool-image] .kanzei/artifacts/tool-images/<sha>.png`(可能有多行,每张一行)。

## 7. 开发服务发现(`kanzei-tools/src/dev_urls.rs`)

`dev_server_urls(project_root)` 遍历该项目还在运行的后台进程(bash 后台模式 / process),读输出末尾 64 KB,去 ANSI(CSI 与 OSC 超链接),匹配 `https?://(localhost|127.0.0.1|[::1]|0.0.0.0)(:端口)?(/路径)?`,0.0.0.0 映射为 localhost,端口必须 1–65535,去掉粘在末尾的标点,按 origin 去重、先到先得;已退出的进程不列。`background` 模块对外私有,这是它开给预览面板的唯一窄接口。

## 8. 主窗口句柄的行为变化

第一次 `add_child` 之后 `get_webview_window("main")` 返回 None(tauri 按「窗口里只有同名 webview」判断是不是 WebviewWindow),拖放事件改走 emit_to_window。全仓一律用 `get_webview("main")` / `get_window("main")`;守卫测试保证代码里不再出现 `get_webview_window(`。主界面的 IPC 与页面内 HTML5 拖放 B0 实测不受影响。主窗口的焦点归还与移动通知见 §1「unstable 的副作用」(`src/preview/host.rs`)。

## 9. B0 结论

| 检查 | 结果 | 落地 |
|---|---|---|
| add_child + 150% DPI 对齐 | 通过:BitBlt 与 PrintWindow 两种抓屏四边误差 0 px,set_bounds 重排后也是 0 | 边界按 CSS px 上报;DPI 变化重放边界 |
| 100% DPI / 跨显示器 | 未测(只有一块 150% 屏) | 列入 §10 |
| 127.0.0.1 静态服务(console / throw / 404 / ES module / 表单) | 通过:`window.__moduleOk = 42` | §3 |
| 进程内 CDP enable + 事件 + UI 线程桥接 | 通过:回调全在 UI 线程;1.5 秒的 awaitPromise 期间 UI 线程延迟 ≤ 0.43 ms;对象参数只有 preview.properties 才有内容 | cdp.rs、console.rs 按 preview 渲染 |
| 可见时截图 | 通过:12–25 ms;最小化时也能截 | 冻结帧流程可行 |
| 隐藏后截图 | 失败:永不返回,重新显示约 30 ms 后才补回 | 路由只在可见时走面板;截图 5 秒超时,迟到回调作废 |
| 点击 / insertText / 回车 / returnByValue | 通过,且都不抢系统焦点 | agent.rs |
| 设备模拟 | 部分:setDeviceMetricsOverride 的 scale 与 dsf 覆盖都会裁图;边界 × 缩放 + set_zoom(无 metrics 覆盖)正好完整显示 390 宽 | fit_device 改为边界 + set_zoom;触屏另开;离开设备模式缩放设回 1 |
| 安全(ACL、导航闸、window.open) | 通过;对照组无闸时 spike_secret 被执行 | §2 |
| 主 webview 在 add_child 之后 | 部分:IPC 与 HTML5 拖放正常;get_webview_window 变 None | §8;系统级拖文件列入 §10 |
| 共用 WebView2 环境 | 通过;环境不一致时 add_child 返回 Ok 但实际失败(HRESULT 0x8007139F,只在日志留一行) | with_environment(主环境)+ 活性回环 |
| DevTools 只给面板 | 通过;设置为 false 时 OpenDevToolsWindow 照样能开,主界面也能被打开 | 不开设置,只在 PaneWebview 上调用,grep 守卫 |
| 焦点 | 默认 focused=true 会抢系统焦点;false 时全程不抢 | builder `.focused(false)` |
| 死服务检测 | loadingFailed(Document, ERR_CONNECTION_REFUSED)+ chrome-error frameNavigated;on_page_load 照样 Finished | §4 |
| 面板内键盘焦点与应用快捷键 | 未测(要往用户会话里敲键) | 列入 §10 |

### 9.1 复核修复的实测依据(Edge headless + CDP,与 WebView2 同内核)

复核时在本机用 Edge headless 经 CDP 跑了一页「主帧正常 + 三个失败 iframe」、一个死端口主帧与一个 pushState 页面,事件序列(已写进 pure_tests 的次序用例):

- 主帧正常时,iframe 指向死端口 / 带 `X-Frame-Options: DENY` 同样产生 `Network.loadingFailed{type:"Document", canceled:false, errorText: ERR_UNSAFE_PORT / ERR_BLOCKED_BY_RESPONSE}`,随后是**带 parentId** 的 `frameNavigated(chrome-error, unreachableUrl)`;主帧没有任何错误事件。
- 主帧死地址:`loadingFailed(Document, requestId = R)` 先到,随后是**无 parentId** 的 `frameNavigated{url: chrome-error://chromewebdata/, unreachableUrl, loaderId = R}`——错误页的 loaderId 等于失败请求的 requestId,主帧与子帧都是这个规律;导航请求本身 `requestId == loaderId`。
- Chromium 随后可能自动重试一次同一地址,产生一条不跟错误页提交的 `loadingFailed` 与一条 `ERR_ABORTED / canceled:true`:只暂存、不进错误态,正好无害。
- pushState:`Page.navigatedWithinDocument{frameId: 主帧 id, url, navigationType: "historyApi"}`,不产生 frameNavigated。

## 10. 剩余人工验收(安装版)

1. DPI 100% 与跨显示器移动窗口时面板对齐;拖动面板宽度、折叠侧栏时对齐。
2. 从资源管理器真实拖文件进输入框(系统级 OLE 拖放)正常;ui_screenshot 正常。
3. 焦点在面板里时 Ctrl+P / Esc / Ctrl+Shift+B 是否还能到达应用(计划 B4 用 Runtime.addBinding 回传白名单按键)。
4. Vite 页面加 HMR 生效;「服务没在跑?」错误页在真实 dev server 停掉后出现。
5. 在绑定线里 browser open 驱动的是面板(首行 backend: pane),另一条线走 headless;面板隐藏后立即走 headless。
6. 设备模式下代理的 click 落点正确(缩放后的坐标);设备模式截图为面板显示尺寸。
7. 面板里的页面 `invoke(...)` 被拒;地址栏输入 `http://tauri.localhost` 被拒。
8. 关闭面板后预览渲染进程退出;KANZEI_E2E_CDP 模式下面板仍能创建。
9. 安装版(没有仓库的机器)无头模式的报错说明清楚;有仓库时经 KANZEI_PLAYWRIGHT_ROOT 正常。
10. (复核修复)**从没打开过面板**的情况下:Alt+Tab 或点任务栏切回 kanzei 后,不点页面直接打字能进输入框、快捷键可用;打开面板并点进面板后切走再切回,焦点回到面板;面板收起后切回,焦点回到主界面;kanzei 在后台时面板被收起,kanzei 不会被拉到前台。
11. (复核修复)拖动主窗口换个位置后,打开设置页的下拉框(原生 `<select>`)与右键菜单,弹出位置贴着控件;面板里的页面同样。
12. (复核修复)停掉 dev server 后在面板里刷新,出现「服务没在跑?」(kind = connection_refused)与「看后台进程」;重新起服务后刷新,错误页消失、面板恢复显示。
13. (复核修复)打开带广告 / 跟踪 iframe 的外站(或自建一页含 `X-Frame-Options: DENY` iframe 的本地页),面板照常显示页面、不出错误页;代理 open 同一地址返回正常结果。
14. (复核修复)手机 / 平板设备模式下:代理 `screenshot` 带 selector 的元素截图与批注附件的裁图不偏;`full_page` 退回可视区并在结果里说明;preview_capture 的 fullPage 明确报不支持。
15. (复核修复)Vite / React 路由页里点站内链接(pushState)后,地址栏、「在系统浏览器打开」与代理结果里的 url 都是新地址,后退按钮可用。
16. (复核修复)页面里 `<iframe src="http://tauri.localhost/">` 与 `http://tauri.localhost:3000/` 不加载,控制台出现一条「已拦截子框架导航」的 warning;页面里点 `mailto:` 链接只记 warning,当前页不被错误页替换。
17. (复核修复)代理连续 open → click → screenshot 期间用户打开一个压到面板上的菜单,整串仍走 backend: pane(不中途变成 headless)。

## 11. 已知限制

- 依赖 tauri "unstable":小版本升级时 add_child 相关 API 可能变,见 §1。
- 原生子 webview 永远盖在 HTML 之上:菜单、弹窗压到面板上时由前端冻结成截图(见「前端」)。
- 代理只能在面板可见时驱动它;用户切到别的视图时代理自动走无头,结果首行如实标注。
- 静态服务不支持 Range 请求(视频拖动进度条不可用),不列目录。同一个根内的文件互相可读(多文件页面需要);不同根、不同运行之间靠随机 root_id 隔开。
- 截图落盘按内容寻址,同一张图在多个对话里共用一个文件;清理按修改时间(再次引用时刷新),不看引用。只落 PNG。
- 设备模式(ZoomFactor ≠ 1)下不做整页截图;要整页请切回「自适应」。
- 代理在面板被遮住不到 10 秒时最多等 1.5 秒;面板收起超过 10 秒后走无头,两边页面状态不共享。

## 前端

(由前端实现补充:面板 UX、遮挡冻结、控制台面板、批注转附件、截图缩略图等。)
