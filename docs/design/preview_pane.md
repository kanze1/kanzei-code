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
  工具栏里出现「对话 | 预览」;rail 开关在「对话」页签时切回预览。因布局变化(窗口变窄、侧栏展开)进入窄屏时停在「对话」页签
  (用户没要求看预览,对话不该被整个藏起来);用户主动打开(rail、地址、各处「在预览中打开」)时停在「预览」;回到宽屏复位成并排。
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
- **关闭与收起是两件事**:面板上的 ✕ 与「更多 → 关闭页面」= 关闭,先 `preview_close` 释放页面(渲染进程退出,声音、定时器、HMR 轮询都停;
  WebView2 被 `SetIsVisible(false)` 时这些都不停),✕ 再收起面板;rail 开关、Ctrl+Shift+B、命令面板 = 收起,只隐藏原生面板、页面保留,再打开直接恢复。
  焦点在面板里时收起,焦点还给 rail 开关(面板 `display:none` 后焦点会掉到 body)。
- 启动时按 `ui_layout.preview.open` 恢复面板开合(只恢复到起始页,不自动加载上次的页面:开发服务可能已经不在了)。本机 localStorage 重启即丢(D-404),
  真值是稍后到达的后端偏好:用户本次启动还没动过开关之前,每次偏好到达都按它对齐。
- **启动先无条件 `preview_close`**:主界面可以被重载(F5 / Ctrl+R,wry 的浏览器加速键默认开着),Rust 侧的子 webview 却还活着、还可见;
  前端状态机假定自己是面板的唯一真源(`alive` 从 false 起步),所以 defer 里、恢复偏好之前先收掉可能留下的孤儿面板。
  **请后端补双保险**:主 webview 的 `PageLoadEvent::Started` 时顺手收掉面板(见 §10)。
- **`preview_open` 的可见性约定**:前端不假设 preview_open 对可见性做了什么——成功后清掉上报缓存,下一帧 evaluate 明确上报一次;
  此刻处于冻结(抽屉、模态、菜单还盖着)时当场补一次 `set_visible(false)`。后端现状是 preview_open 顺手 `show()`,所以这一步不能省。
  契约写死为:preview_open 之后面板的可见性以前端随后的 `preview_set_visible` 为准。

### 3. 遮挡冻结

原生子 webview 永远画在 HTML 之上,z-index 管不着。浮层只有两条活路:

1. **让开**:常驻浮层 `.k-card` / `.k-chip-float` / `.k-toast-region` 的 inset 右值加 `var(--surface-safe-right, 0px)`(卡片、芯片与 `--kz-dock-right` 取大者)。
   24-preview.js 把它写在 `<html>` 上 = 视口右缘到面板左缘的距离(预览 + 其右侧停靠的后台任务侧栏);面板关闭、非对话视图、窄屏占满时为 0。
2. **冻结**:订阅 00-surface 的 `onSurfaceChange`(模态激活、任何关闭、锚定弹层/卡片打开、toast 区域显隐、提示显隐各通知一次)。
   栈里有模态,或任一弹层矩形与 `#preview-host` 相交,或后台任务侧栏的抽屉盖过来,或正在拖动分隔条/框体(`html[data-kz-frame-drag]`),
   就先 `preview_capture` 截一帧放进 `#preview-freeze`,再 `set_visible(false)`;不再遮挡时去抖 120ms 恢复。
   截图只在面板可见时发(隐藏态截图会挂起,B0 实测),截图失败照样隐藏(露出舞台底色)。
   **冻结用的截图只等 300ms**(`PREVIEW_FREEZE_CAPTURE_MS`;B0 实测可见态截图 12–25ms):被预览的页面死循环、渲染进程忙时截图迟迟不回,
   到点先隐藏、打开菜单,冻结帧留空;截图迟到且同一次冻结(`freezeGen`)仍在时再补上。5s 的截图超时只留给「截图放进输入框」这类显式截图——
   页面卡死时「更多 → 关闭页面」是逃生口,不能等 6 秒才弹出来。
   自家工具栏的菜单(视口与配色、更多)先冻结再开,不会先被原生面板盖一帧。
3. **提示不冻结、改走侧向**:提示(tooltip)短暂且小,`isOccluded` 跳过它——为它冻结会让被预览的页面每次悬停都收到 visibilitychange
   (暂停视频、上报埋点的开发页跟着抖),代理的 browser 也会中途被切到无头。面板里的提示改走侧向摆位:`#preview-dock[data-kz-tip-side="inline-start"]`,
   00-surface `showTipNow` 查锚点祖先的该属性、抄到提示的 `data-side`,surface.css 给 `position-area: center inline-start`(放不下 flip-inline),
   与按钮同一行,不进占位框。
4. **拖动分隔条**:拖动期间(`html[data-kz-frame-drag]` 存在)按遮挡冻结;pointerdown / pointerup / pointercancel / lostpointercapture 触发按帧重判。
   指针划过原生面板时依赖跨窗口的鼠标捕获,B0 没有测过,冻结后原生面板不在,这条路径不再依赖它。安装版手工验收仍要拖一次(见 §10)。

后果:冻结期间可见性上报为 false,代理的 browser 按路由规则暂时走无头。
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
| `example.com/docs`、`example.com/page.html`、`docs.example.com/index.html` | 补 `https://`(首段像主机:含点且顶级域是字母) |
| `192.168.1.5:3000`、`example.com:8080/x` | 补 `http://`(IP 字面量或带端口的非 localhost 主机多半是局域网开发服务) |
| `docs/index.html`、`index.html?x=1`、`./out/a.svg`、`out.v2/index.html`、`C:\…\页面.html`、`file:///C:/x/a.html` | path(后端换成 127.0.0.1 静态服务地址);扩展名按去掉 `?#` 之后判 |
| `file://server/share/a.html` | path `//server/share/a.html`(UNC,主机名保留);`file://localhost/…` 同本机 |
| `data:text/html,<h1>x</h1>` | 原样(只剥成对出现的 `<…>` / 引号,内容里的尖括号不动) |
| `javascript:`、`tauri:`、`ftp:`、带空格的其它文本 | invalid(toast「无法识别的地址」,地址栏留在编辑态) |

地址栏与控制台里显示静态服务地址时去掉每次启动都变的 token,只留项目相对路径(`displayUrl`)。最近 5 个打开过的地址存 `ui_layout.preview.recent`;
data: 地址、代码片段地址、超过 2048 字的地址不记(一条 64KB 的 data URL 会让 `apply_ui_layout` 按 `UI_LAYOUT_MAX_BYTES` 拒绝整个补丁,
同一批去抖里的宽度一起丢),读取时也过滤一遍。

编辑态看显式标记,不看 `document.activeElement`(焦点点进原生子 webview 时主文档的 activeElement 不变):`input` 置位;Enter(地址认得出时)、
Esc、blur 清零;Enter 读完值交出焦点(与浏览器一致),blur 时按当前页面地址回填。编辑态之外,`kz:preview-state` 带来的地址(规范化后的、
页内跳转的)随时写回地址栏。

### 5. 起始页、错误页、控制台

- 起始页(页面还没打开时):说明、`preview_dev_urls{projectDir}` 检测到的本地开发服务(地址 + 启动命令,点一下打开)、最近打开。
- 错误页:`kz:preview-state.error.kind` 为 `connection_refused` 时「服务没在跑？」+ 地址 + `net::ERR_*` 原文 + [重试](preview_nav reload)
  [看后台进程](打开后台任务侧栏,终端条目在那里);`unsafe_port` / `blocked` / `other` 各有说明。错误页显示期间原生面板隐藏。
- 错误页是 `role=alert`:内容(类别、文案、原文、地址、语言)没变就不碰 DOM,加载状态、标题变化的状态事件不会让读屏重播。
- 控制台:`kz:preview-console{entries}` 按 seq 去重、最多 500 条;级别筛选 全部 / 错误 / 警告 / 网络;每条带来源(列里 `文件名:行:列`,悬停是完整地址);
  错误数角标挂在控制台按钮上(读屏名称带错误数)。**网络来源的条目 `level = "network"`**(后端 console.rs:Log.entryAdded source=network 的 4xx/5xx
  与 net::ERR_*、主文档加载失败),前端按级别判为先,文本启发式(net::ERR_、Failed to load resource、HTTP 4xx/5xx、「GET /x 500」)只给没标 network 的
  错误级条目兜底;网络条目都是加载失败,所以「错误」筛选、红色高亮与角标都把它算进去(同一个 `consoleMatches(entry, "error")`)。主框架换页时清空并按 `preview_console{sinceSeq: 0}` 重取(后端的环在导航时已清),
  「保留日志」开着时不清(纯前端开关,存 `ui_layout.preview.preserve_log`)。条目文本一律 textContent,不解析 HTML。
  控制台高度可拖(`installSplit(id: "preview-console")`)。

### 6. 对话里的入口

- **工具截图**:后端把模型看到的截图落到 `.kanzei/artifacts/tool-images/<sha>.png`,正文末尾追加 `[tool-image] <路径>`。`fillToolBlock` 摘下标记行
  (摘要器与展开区只看去掉标记的正文),在工具行头之后、折叠详情之前画缩略图(经 `tool_image{projectDir, rel}` 取图,进视口才取,点开进查看器看大图)。
  实时与历史回放同一条路径。折叠的工具组里最后一张截图所在的行常驻可见(与失败行同理)。同一个块被填第二次(停止后补发的 ToolEnd、
  孤儿结果回填)先摘掉旧的缩略图条,不成对重复。后台任务侧栏的条目(06-activity `bgEnd`)同样摘掉标记行再做摘要与失败详情。
- **browser 摘要**:结果首行 `backend: pane（用户可见）` / `backend: headless`,⎿ 行 = 动作 · 主机(或文件名、代码片段)· 面板/无头;
  认新动作 screenshot / press / scroll / wait / eval,参数摘要覆盖 html / key / expression。旧版无头结果(没有 backend 行)照旧「截图 主机」。
  走无头且成功的结果给「在预览中打开」(优先入参 path / html,其次 url——静态服务地址带 token,历史回放时早已失效);
  等待批准(needs_confirmation)、失败的块不给。
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
(停靠后对话列 ≥ 600px)算上预览栏,且预览栏按「停靠后仍要并排」取宽:
`sideDockMode({ mainWidth: 主区宽 − previewColumnWidth(max(主区宽 − 侧栏宽, 800)), panelWidth })`。不取下限时,1333×695 窗口
(主区 1005、侧栏 360)停靠后对话视图只剩 645 < 800,`previewColumnFor` 返回 0,判据误判「停靠」,预览随即切进窄屏、把整个对话列藏到「对话」页签后面;
停靠态会自动打开(子代理、跑满 3 秒的命令),活动结束 6 秒后又收起,对话被来回挤掉。取下限后这类停靠判抽屉,停靠永远不会把对话视图压到 800 以下。
预览开着时多数窗口下侧栏改抽屉,抽屉盖到面板上时按 §3 冻结。预览开合、分隔条拖动时主动 `reconcileTasksPanel()`。

### 9. 门禁与验证

- `ui-runtime-smoke` 分区「网页预览前端」:纯函数(地址规范化 11 正 5 反、设备摆位、截图标记、预览栏宽、browser 目标与摘要、控制台分类)、
  打开面板收到占位框矩形、切视图/切线的可见性与 processId、相交菜单先截图再隐藏且关菜单恢复、不相交菜单不冻结、模态冻结、
  写文件恰好一次刷新(开发服务页不刷新)、批注进附件与输入框、缩略图与「在预览中打开」、交付卡片、侧栏停靠判据与抽屉冻结、
  控制台去重/上限/角标/来源、错误页、Ctrl+Shift+B、代码块、localhost 链接、onSurfaceChange 通知。
  变异守卫 previewHideOnViewSwitch / previewFreezeOnSurface / previewBindProcess / toolImageMarker / previewReloadOnWrite / previewTasksReserve,逐条实跑变红。
  复核修复追加:previewDockNarrowFloor(主区 1005 判抽屉、预览不进窄屏)/ previewLayoutNarrowChat / previewAddressEditing(聚焦回车后地址写回、打字不被覆盖)/
  previewFreezeBudget(截图永不返回的桩:预算内 set_visible(false) 且菜单已开,迟到补帧)/ previewCloseReleases / previewCloseFocus / previewStartupClose /
  previewTooltipNoFreeze / previewTipSide / previewFrozenOpen / previewFrameDragFreeze / previewErrorNoRewrite / previewShotsDedupe /
  previewOpenInSuccessOnly / previewBgMarker / previewRecentSkip / previewConsoleBadgeNetwork,同样逐条实跑变红;地址规范化新增 10 条用例(8 条在旧实现上失败)。
- `ipc-event-smoke` 分区「网页预览前端」:命令名求差——前端 `invoke("x")` 的字面命令名 ⊆ `generate_handler!`(认 `rename`)。
  预览这一族在后端模块 `crates/kanzei-app/src/preview/` 合入前不判,合入后自动生效(不是常设豁免);`KNOWN_UNREGISTERED` 只收一条早于本特性的既有缺陷
  `run_tool_process_stop`(06-activity「停止后台进程」调用了从未注册的命令),条目过期(已注册或不再调用)会反过来报红。
- `ui-narrow-layout-smoke` 分区「网页预览前端」:7 种布局真量(两列、面板宽口径、对话列居中不压面板、与停靠侧栏并存、窄屏两个页签)+ 1 条判据自检。
- `ui-surface-rules` P 组 + 4 条反例自测;`ui-preview` 场景 `preview`(`state=page|empty|error`,占位框用棋盘格,样式只注入预览页)。

### 10. 接缝与已知限制

- 事件两侧求差(`ipc-event-smoke`、`ui-runtime-smoke` 的 D-381)要等后端半边的 `kz:preview-*` emit 合入才平;前端半边单独跑时这两处只差这三个事件。
  命令名求差在后端模块合入后自动生效;合入前已按后端分支核对过,17 个命令名全部在它的 `generate_handler!` 里。
- 冻结 = 隐藏,冻结期间代理的 browser 走无头(提示不再触发冻结)。
- 焦点在原生面板里时,应用快捷键(Esc、Ctrl+Shift+B、Ctrl+P)收不到,要后端经 Runtime.addBinding 回传。
- **请后端处理的接缝**(前端做不了,写在这里等后端半边确认):
  1. `/t/<token>/s/`(代码片段)的响应加 `Content-Security-Policy: sandbox allow-scripts allow-forms allow-modals`,让片段落到不透明源。
     片段与项目文件地址 `/t/<token>/r/` 同源,片段里的脚本能从自己的 URL 拿到 token、进而读项目文件;而代码块「预览」挂在全局的
     `renderMarkdownInto` 上,文档页、研究页(可能含抓来的网页内容)里的 html 代码块也有这个按钮(要用户点一下才会执行)。
  2. 发出 `kz:preview-pick` 之后把焦点还给主 webview(`get_webview("main")` 上 `set_focus`):点选完成时系统焦点还在子 webview 里,
     `addPickAttachment` 里的 `promptBox.focus()` 接不到键盘输入。
  3. 主 webview 的 `PageLoadEvent::Started` 时收掉面板,作为前端启动 `preview_close` 的双保险(§2)。
- 100% 缩放、跨显示器、真实系统拖放与焦点抢占只能在安装版手工验收;另加一条:拖动预览分隔条、控制台分隔条经过原生面板(§3 第 4 条),
  松手后面板位置与宽度正确、没有卡在拖动态。
