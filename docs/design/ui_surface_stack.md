# 弹层技术栈:一种写法、一处外观、一道门禁

- 身份: live_design
- 状态: 设计基线(2026-09-26 起草并随 release/2026-09-26-ui 分支实施,本文描述的是已落地的实现);同日 UI2-0926 #4「很多的弹出窗口不可以修改拖拽，不是可变的」补 §4.6 可调框与分隔条,#14 把两块常驻浮层合成停靠的后台任务侧栏
- 日期: 2026-09-26
- 上游文档: [chat_presentation_contract.md](chat_presentation_contract.md);下游: [subagent_presentation.md](subagent_presentation.md) §5.6(后台任务侧栏)
- 关联需求: 无(用户 2026-09-26 的 UI 问题清单第 9 条「弹层技术栈」,另有截图 6 的白色模型下拉;tracker 条目待登记)
- 关联缺陷: 无
- 关联决策: 无
- 一句话: 弹层各写各的外观,原生控件不归 CSS 管,门禁只认 hex,所以每加一个就坏一个。现在弹层收敛到「平台原生顶层原语(dialog / popover / 锚点定位 / base-select)+ 组件层 token + 唯一的 00-surface.js + 门禁 + 样例页」,只有一种写法。

## 1. 症状与直接根因:模型下拉为什么是白的

截图 6:输入区的模型下拉在暗色界面里弹出一块白底菜单。成因链路:

1. `#model-select` 当时是原生 `<select class="ctx-select">`(UI-0926 #3 起,输入框上方的模型与思考强度已换成自绘芯片 `#model-picker`/`#reasoning-picker`,菜单走 00-surface 的 `openMenu`,不再是原生 select;其余原生 select 按 §5 用 `appearance: base-select`)。
2. `.ctx-select` 写了 `background: transparent`,只有 `:hover` 时才给 `var(--panel2)`。
3. Chromium(WebView2)在 Windows 上画经典下拉用的是一张内部弹出页:Blink 把 select 的计算背景色与文字色序列化进去,弹出页 `body` 固定 `background-color: white`。背景透明时透出来的就是这块白。
4. 弹出期间 select 样式一变就重新序列化:鼠标从 select 移进列表后 `:hover` 消失,背景回到透明,文字回到 `--dim`,于是成了白底浅灰字。
5. `:root` 上的 `color-scheme: dark` 只改变浏览器默认色,管不到作者显式写的 `transparent`。

同病的还有思考强度、交付方式、线路模型等所有透明按钮态的下拉。只修一个选择器能止血,但下一次还会以别的形态出现,原因见 §2。

## 2. 为什么总是「加一个坏一个」

| # | 根因 |
|---|---|
| R1 | 没有共享的弹层原语:十几个弹层各自声明外观,开关有 4 套机制(`.hidden` 类、`<details open>`、`position:fixed` + 脚本定位、手工遍历 `inert`)。同一行的「任务设置」与「更多」菜单底色、边框、阴影三处都不同。 |
| R2 | 原生控件不归页面 CSS 管:几十个 `<select>` 的弹出列表、约 200 处 `title` 提示都由浏览器或系统绘制。 |
| R3 | token 只有语义一层,没有「弹层该用哪几个 token」这一层;阴影与遮罩写成字面量。 |
| R4 | 门禁只认 hex:主题块外的 25 处 `rgba()`(其中 10 处在弹层上)全部漏过;圆角、阴影、层级、新造 fixed 浮层都没有门禁。 |
| R5 | 层级与 Esc 没有归属:z 档位互相矛盾;多个 document 级 Esc 监听互不让位——权限卡在场时,在确认框里按 Esc 会先把那条权限请求拒掉。模态期间全局快捷键照常生效。 |
| R6 | 看不到:弹层只在各自场景里出现;运行时冒烟是假 DOM,没有顶层、样式与颜色。 |

## 3. 选型

本机 WebView2 为 153(常青运行时)。用到的平台能力及首发版本:`<dialog>.showModal()`(37)、`::backdrop` 继承自定义属性(122)、`dialog[closedby]`(134)、Popover API(114)/`popover="hint"`(133)、CSS 锚点定位 `anchor-name`/`position-anchor`/`position-area`/`position-try-fallbacks`(125~129)、`appearance: base-select` / `::picker(select)` / `option::checkmark`(135)、`@layer`/`@starting-style`(99/117)。全部 ≤ 153;样例冒烟启动时再探测一次,运行时回退会直接失败而不是静默放行。

候选:A 保持原生 ESM,加组件层 token + 唯一 surface 模块 + `@layer` 层序 + 门禁 + 样例页;B Lit Web Components(Shadow DOM 挡住 i18n 的 `[data-i18n-*]` 遍历与假 DOM 冒烟,且顶层与定位最终仍靠 dialog/popover);C 换框架 + 无头组件库(要接构建、重写约 2.2 万行命令式 JS,焦点陷阱/顶层/轻关闭/定位平台已原生提供);D Open Props 等 token 库(只给数值,不解决「外观归谁画」,也不带门禁)。**采用 A**:缺的只是「只准这样写」的约束,一个零依赖模块、层序与门禁就能补上,不用改现有代码的写法。

## 4. 方案结构

### 4.1 token:语义层 + 组件层

```
语义层  --bg --panel --panel2 --fg --border --accent --elev-* --scrim --surface-overlay/-hover/-selected …
        即 style.css 顶部 :root(暗)与 [data-theme="light"](亮)两块;hex 只准出现在这两块里
  ↓ 引用
组件层  --surface-*(:root 里「/* 组件层」注释开头的一段)——只在 :root 定义一次、只引用语义层、只准 surface.css 使用
```

- **不引入 `--c-*` 原始层**:主题切换只发生在语义层,组件层不在亮色块里重定义。注意 `--surface-overlay`、`--surface-raised`、`--surface-hover`、`--surface-selected` 名字带 surface 但属于语义层(两块主题里各有字面量值),组件层是「/* 组件层」那一段里的名字。
- 现行组件层(值随语义层变):`--surface-bg`(= `--surface-overlay`)、`--surface-bg-raised`(tooltip)、`--surface-fg`/`-fg-strong`/`-muted`、`--surface-border`/`-divider`、`--surface-attention`(权限卡与提问胶囊描边 = `--warn`,「需要你」,见 [ui_color_semantics.md](ui_color_semantics.md))、`--surface-item-hover`/`-item-active`(中性半透明)、`--surface-radius-sm/md/lg`、`--surface-shadow-1/2/3`(= `--elev-*`,亮色另有一套浅阴影)、`--surface-backdrop`(模态遮罩 = `--scrim`)、`--surface-backdrop-soft`(命令面板:跳转器不是确认框,背后界面要看得见;后台任务侧栏抽屉的遮罩 `.k-scrim` 同用)、`--surface-panel-docked`(停靠侧栏底色 = `--sidebar-bg`,与左侧栏对称)、`--surface-offset`、`--surface-motion`。
- 运行时变量(不在 CSS 里定义,由脚本写,门禁 T1 豁免):`--kz-frame-l/r/t/b/w/h`(00-frame.js 写在框上,§4.6)、`--kz-split-<id>`(分隔条,写在 `<html>`)、`--kz-dock-right`(后台任务侧栏停靠时占去的右侧宽度,06-agent-panel.js 写在 `<html>`,`:root` 给默认 `0px`;右下停靠的卡片与芯片据此让开)。
- 高位 z 档位 `--z-float/--z-overlay/--z-dialog/--z-toast` 已删:进入顶层的元素不需要 z-index。`--z-drawer` 留给后台任务侧栏的抽屉态(停靠态在 `#main` 网格里,不需要 z-index)。
- **嵌套主题区块的坑**:组件层在 `:root` 上以 `var()` 引用语义层,计算值在 `:root` 就定死了;某个子元素上再挂 `[data-theme="light"]` 只换语义层,组件层仍是 `:root` 的计算值。应用里主题只挂在 `<html>` 上,不受影响;样例页的静态矩阵要同屏两套主题,由 gallery.js 在亮色区块上按样式表原文把 `:root` 里所有以 `var()` 定义的 token 重声明一遍(组件层 `--surface-*` 之外,`--danger`/`--alert`/`--muted`/`--dot-idle` 这类语义别名同样在 `:root` 求值;只重声明组件层时,亮色矩阵的危险菜单项拿到暗色 `--danger` #ff7b72,白底上 2.52:1)。亮色块自己给了字面量的名字不重声明。ui-surface-gallery-smoke 逐个比对区块上全部主题 token 的计算值与 `html[data-theme=light]` 上的计算值。

### 4.2 CSS 层序与文件

```
ui/app.css(index.html 与 gallery.html 唯一引用的样式入口)
  @layer tokens, base, app, surface;
  @import url("style.css") layer(app);
  @import url("surface.css") layer(surface);
```

- `app` 层就是 style.css 全文(含 token 块),写法不变。`surface` 层排在它之后:**层序优先于选择器特异性**,视图里就算写了 `#confirm-overlay { background: … }` 也压不过弹层外观——这是「一劳永逸」的机制,不只是约定。
- 反过来,surface 层写了的属性视图也改不动,所以 surface.css 只写外观、定位与少量通用内边距(菜单 4px、浮层 10px 12px、卡片、模态);宽度等尺寸留给 style.css(`.task-options-panel { width: 340px }` 这类)。
- `!important` 会反转层序:`.hidden { display:none !important }` 与减少动效的 `!important` 继续生效,这正是期望的;门禁禁止在外观属性上写 `!important`。
- 顶层元素关闭态一律 `display: none`(surface 层 `dialog.k-surface:not([open]), .k-surface[popover]:not(:popover-open)`):否则 style.css 里任何 `display: flex` 都会把关着的弹层画进文档流(作者样式压过 UA 的 `display: none`)。
- 未分层样式(内联 style、Monaco 注入的 CSS、样例页自己的 `<style>`)高于一切分层样式。style.css 里没有 Monaco 规则,不存在交集;今后覆盖 Monaco 时要注意。
- index.html 头部 `<meta name="color-scheme" content="dark light">`。

### 4.3 surface.css(@layer surface)

`.k-surface` 共同外观(底色/文字/边框/圆角/阴影);`.k-dialog`(+ `data-size="lg"|"palette"`,`::backdrop`);`.k-menu`/`.k-popover`(锚点定位,默认宽 ≤420px、高 ≤min(65vh, 480px);补全列表 `.k-popover[role="listbox"]` 放开宽度上限、高 220px、左右不留偏移,宽度由 style.css 的 `anchor-size(width)` 跟输入框走;`data-placement="top-end|top-start|bottom-end|bottom-start"` 映射到 `position-area`,`position-try-fallbacks: flip-block, flip-inline`);`.k-menu-item`/`-sep`/`-heading`,`.k-menu .menu-row:hover`;`.k-card`(停靠右下、按 `--kz-dock-right` 让开停靠侧栏,`data-tone="attention"` 描边;权限卡 `data-kz-anchor="composer"` 在对话视图里锚在输入区正上方、与输入区同宽)与 `.k-chip-float`;`.k-toast-region` 与 `.k-toast[data-kind=ok|warn|err]`(按 kind 浅底 + 1px 软边,不用彩色左竖条;info 中性);`.k-tooltip`;`select, ::picker(select) { appearance: base-select }` 与 `::picker(select)`/`option`/`option::checkmark`;`.k-panel` 与 `.k-panel[data-dock="side"|"drawer"]`(停靠:直角、只留左分隔线、无阴影;抽屉:加 3 档阴影)、`.k-scrim`(抽屉遮罩);§10 可调框落位与手柄(§4.6);`@starting-style` 入场动效(减少动效的全局 `!important` 自动压平);`.k-static`(样例页把顶层弹层按普通块画出来)。

- 悬停/选中只叠半透明层(`background-image: linear-gradient(…)`),不换掉不透明底色:浮在内容上的芯片、下拉选项若底色变半透明就看不清。
- `option` 的 `background-color` 是兜底:即使 base-select 不可用,Blink 也会把 option 的底色抄进经典弹出页,列表仍跟主题。

### 4.4 ui/00-surface.js(零 import)

- **零 import**:样例页与假 DOM 冒烟都要能单独加载它,也免得卷进 ESM 循环依赖。翻译函数由 01-core.js 启动时 `setSurfaceTranslator((key) => t(key))` 注入。index.html 的脚本清单里它排在 01-core 前。
- **状态唯一**:`const stack = []`,元素 `{ el, type: "modal"|"menu"|"popover"|"card", escape, onClose, anchor, lightDismiss, returnFocus, … }`。打开摘 `.hidden`、关闭加回(镜像):旧代码与冒烟读 `classList.contains("hidden")` 仍然成立;**除本模块外任何代码都不得直接切换弹层的 `.hidden`**(门禁 J1)。
- **Esc 只有一个入口**,固定形态(变异守卫按这一行定位):

```js
function onKeydown(event) {
  if (event.key !== "Escape" || event.isComposing) return;
  if (nativePickerOpen(event)) return;   // 页面内下拉列表开着:Esc 归浏览器关列表
  const top = topEscapable(event.target ?? activeElement());
  if (!top) return;
  event.preventDefault();
  event.stopImmediatePropagation();
  top.escape();
}
document.addEventListener("keydown", onKeydown, true);
```

  `topEscapable(target)` 返回栈顶第一个可 Esc 的句柄;模态开着时,之后才弹出的停靠卡片在模态背后是惰性的,不参与。弹层内输入框自己的 Esc(如「更多」菜单里的搜索框)因此改为关闭弹层,这是有意的行为变化。
  **停靠卡片让位局部 Esc**:按键目标是卡片外的文字输入框(text 类 input、textarea、contenteditable)或位于 `.monaco-editor` 内时跳过卡片,不 `preventDefault`、不截断传播——#prompt、想法/缺陷速记表单的 Esc 照常取消输入,Monaco 查找/补全小部件照常收起,权限请求不被顺手拒掉(与卡片 `focus: "auto"`「用户在别处打字时不打扰」同一条理由)。勾选框/按钮/下拉上没有局部 Esc 含义,仍按卡片的 `onEscape` 处理。
- **点外关闭由模块做,不交给 `popover="auto"`**:所有菜单/浮层都是 `popover="manual"`,模块在 document 捕获阶段的 `pointerdown` 里自顶向下关掉不含点击目标的轻关闭弹层,**锚点(触发器)除外**。原因:浏览器的 auto 轻关闭发生在 pointerdown 分发之前,「点触发器收起菜单」会先被轻关闭、再被触发器的 click 重新打开,触发器永远关不掉它;自己做还让假 DOM 能测。打开一个轻关闭弹层时,不以它为祖先的其它轻关闭弹层先关(与 auto 语义一致);打开模态时轻关闭弹层全部先关。
- **模态**:`openDialog` 用 `showModal`(原生惰性化背景、焦点关在里面);记住并归还焦点;聚焦 `initialFocus`(或 `[autofocus]`/第一个可聚焦元素);`closedby="any"` 点外关闭,不支持时退化为「点在 dialog 自身且坐标在内容框外」。同一个 dialog 被并发打开时排队(`queue: true`,确认框/输入框用),内容在轮到它时才写入(`prepare`),不再互相覆盖。
- **原生事件是异步的**:`close`/`toggle` 在任务里派发。排队的第二个确认框在同一任务里已经 `showModal`,迟到的 `close` 不能把它当成被关掉——只在元素确实已关闭时才同步栈(浏览器冒烟有专门用例)。
- **卡片焦点**:`showCard(el, { focus: "auto" })` 在焦点位于别处的可编辑元素(用户正在打字)时不抢焦点,否则聚焦 `initialFocus`/第一个按钮;抢过焦点的卡片关闭时把焦点还回去。修掉「正在打字时焦点被抢到『允许一次』,下一个空格就放行」。`onEscape` 缺省 = 不响应 Esc(收起后的「重新打开」芯片不能被 Esc 弄丢)。
- **锚点**:`anchorTo(el, anchor)` 分配 `--kz-anchor-N`,写 `anchor-name` 与 `position-anchor`。
- **挂载点**:JS 现造的弹层(openMenu 菜单、tooltip)放进 `<div id="kz-surface-root">`,首次使用时追加到 body 末尾(另存引用,不依赖 getElementById 查后加的节点)。**例外:锚点在开着的 `<dialog>` 里时,openMenu 把菜单挂进这个 dialog**——模态打开期间浏览器把 dialog 子树之外的一切(包括之后才弹出的顶层 popover)设为惰性,挂在 body 下的菜单弹得出来却点不动、拿不到焦点(无头 Edge 153 实测)。菜单显示后照样进顶层,不受 dialog 的 overflow 裁剪;关 dialog 时它作为嵌套弹层一起关。静态弹层同理必须写在 dialog 内(门禁 H 组查 index.html);模态开着时对 dialog 外的元素调 `openPopover` 会 `console.warn`。
- **程序化聚焦不弹提示**:模态初始焦点(含 `showModal` 自己的聚焦步骤)、卡片抢焦点、菜单首项、关闭后归还焦点都在 `quietly()` 里执行,tooltip 的 focusin 看到标记就跳过——否则键盘打开查看器时「关闭」提示立刻盖住弹窗角。菜单内方向键移动是用户的键盘导航,提示照常。

对外 API:

```js
setSurfaceTranslator(fn)
isModalOpen() → boolean;  stackDepth() → number;  isSurfaceOpen(el) → boolean
openDialog(dialogEl, { initialFocus, onEscape, onClose, cancelValue, prepare, queue }) → handle{ el, result: Promise, close(value) }
closeSurface(handleOrEl, value)
confirmDialog({ title, message, list, okText, safeText, danger }) → Promise<true|false|"safe">
inputDialog({ title, message, value, placeholder, okText }) → Promise<string|null>   // 组合输入中的 Enter 不提交
openPopover(anchorEl, el, { placement, manual, type, onEscape, onClose }) → handle     // 已开着则幂等返回;anchorEl 为空取 bindMenus 登记的触发器
openMenu(anchorEl, items, { placement, label, onClose }) → handle                       // items: { label, desc?, kbd?, checked?, disabled?, danger?, onSelect } | "separator" | { heading };同锚点再调 = 收起
bindMenus(root)          // 扫 [data-kz-menu="<弹层 id>"]:aria-haspopup/controls/expanded、click 切换、ArrowDown 打开并聚焦第一项、菜单内方向键
showCard(el, { onEscape, focus: "auto"|"none", initialFocus });  hideCard(el)
toast(message, { kind: "info"|"ok"|"warn"|"err", timeout }) → remove()                   // 最多 3 条;err 用 role=alert(默认 6s),其余 role=status(2.6s);过期只隐藏、留到下一条再清
installTooltips(root)    // 接管全局 title:悬停 450ms/键盘聚焦立即显示,title 暂挪进 data-kz-tip,离开还回;.monaco-editor 内不接管
onSurfaceChange(fn) → off()   // UI2-0926 #8:栈变化钩子,fn([{ el, type }]);模态激活、任何关闭、锚定弹层/卡片打开、toast 区域显隐、提示显隐各通知一次
surfaceElements() → [{ el, type }]  // 此刻浮着的全部弹层(栈 + 显示中的提示 + 有条目的 toast 区域),订阅方布局变化后主动重判用
```

**网页预览的遮挡(UI2-0926 #8)**:网页预览面板是 Tauri 子 webview(原生窗口),永远画在 HTML 之上,z-index 管不着。浮层只有两条活路:
① 常驻浮层让开它——`.k-card`、`.k-chip-float`、`.k-toast-region` 的 inset 右值加 `var(--surface-safe-right, 0px)`(卡片/芯片与 `--kz-dock-right` 取大者),
该变量由 24-preview.js 写在 `<html>` 上 = 视口右缘到预览面板左缘的距离,面板关闭、非对话视图、窄屏占满时为 0;
② 临时弹层压上去时冻结——24-preview.js 订阅 `onSurfaceChange`,栈里有模态、或任一弹层矩形与 `#preview-host` 相交,就先截一帧放进 `#preview-freeze`、
再隐藏原生面板,不再遮挡时(去抖 120ms)恢复。钩子零依赖、不做几何,相交判定归订阅方。设计与路由后果见 [preview_pane.md](preview_pane.md) §前端。

导入习惯不变:01-core.js 仍导出 `confirmDialog`/`inputDialog`(连同冒烟接缝 `setConfirmDialog`/`setInputDialog`),内部委托 00-surface;03-shell.js 的 `toast(text, { kind })` 负责本地化后交给 surface 的 toast,`toastError` 仍写日志面板(长错误不交给会自动消失的 toast)。

### 4.5 原生 select:为什么是 base-select 而不是 JS 替身

几十个 select 的 `.value`/`.options`/`change` 被业务代码直接使用,假 DOM 冒烟也专门模拟了 SELECT 的规范语义;JS 替身会把这些全部打断。`appearance: base-select` 保留原生语义、键盘首字母跳转与表单行为,同时让列表变成顶层弹层、由 surface.css 绘制;按钮态仍归视图(输入区的透明芯片是 `.kz-ctl`,见 §11)。门禁禁止 `multiple`/`size`(列表框模式外观规则不同)。

两处 base-select 的坑(UI2-0926 #11 补):

- base-select 的 UA 按钮盒在 flex 行里被块化成 `display: flex`,但 `align-items` 计算值是 `normal`(= stretch)。固定高度的下拉(输入区模式芯片 30px)里已选文字与 `::picker-icon` 被拉满后顶在上半截(实测墨迹偏离中线 4.31px)。**style.css 的基础 `select` 规则必须带 `align-items: center`**,全应用 51 个 select 一次修好;浏览器冒烟 §7.3 第 7 项逐个核对计算值。
- `::picker-icon` 的 UA 默认内容是 `counter(disclosure-open)` 的实心 ▼ 字形,字号随文本、基线不齐,与其它菜单触发器的箭头不是一个东西。surface.css §6 把它换成细 V 形遮罩 `var(--icon-chevron)`(与 `.kz-chev`、`.picker-btn::after` 共用,token 定义在 style.css `:root`),`select:open` 时翻转 180°。

### 4.6 ui/00-frame.js:可移动/可调尺寸的框与分隔条(零 import)

2026-09-26 用户原话:「很多的弹出窗口不可以修改拖拽，不是可变的」。拖动与调尺寸只有这一个入口,两种东西同一套规矩:

- **框(frame)**:模态弹窗(`<dialog class="k-surface k-dialog">`,命令面板除外)与停靠卡片(`.k-card`)。在 index.html 上写属性,启动时 `bindFrames(document)` 接线,调用点零改动:

  | 属性 | 含义 |
  |---|---|
  | `data-kz-frame="<id>"` | 必填、唯一;几何偏好的键 |
  | `data-kz-frame-move="<选择器>\|:scope"` | 拖动区(标题栏);`:scope` = 整框可拖(确认框/输入框),按钮、输入框、链接等交互元素上按下不开始移动 |
  | `data-kz-frame-edges="all\|n e s w ne se sw nw"` | 可拖的边/角;不写 = 只能移动 |
  | `data-kz-frame-min="<宽> <高>"` | 最小尺寸 |
  | `data-kz-frame-keep="w\|h"` | 窗口缩小夹紧时优先保住的维度 |
  | `data-kz-frame-persist="no"` | 不记几何:关闭即复位(确认框/输入框每次都在中间) |

  现有的框:查看器 `viewer`(八向)、项目模型 `project-models`(八向)、权限卡 `ask`(标题栏移动、左右调宽,锚在输入区上方,见 §4.3)、确认框 `confirm` 与输入框 `input`(整框可拖、不记几何)。**菜单、浮层、提示、toast、芯片不是框**——它们跟着锚点走(门禁 H)。
- **交互**:按下后移动超过 3px 才算拖动(之前不捕获指针,普通点击与双击的目标不变);手势监听挂在 document 捕获阶段,甩得再快也跟得上;拖边时对边不动;双击拖动区复位(清掉偏好,回到默认居中/停靠);框内 Alt+Shift+方向键每次调 24px,Alt+Shift+Home 复位;窗口缩小时整框夹进视口(留 8px),窗口恢复后回到用户摆的几何(存的是意图,落地值按当前视口夹紧,不改写存储)。拖动中 `<html data-kz-frame-drag>` 统一光标并禁止选中文字。
- **几何只写成变量与令牌**:`--kz-frame-l/r/t/b/w/h` 写在框自身,`data-kz-placed="pos w h"` 标出哪几项由用户定;落位规则只在 surface.css §10(选择器 `(0,3,0)` 排在文件最后,压过 `.k-dialog[data-size="lg"]` 与权限卡的锚定摆放)。不写内联 `width/left`:内联尺寸会压过 `.collapsed` 这类状态规则。可调尺寸的框本体 `overflow: visible`(手柄骑在边上、外露 6px),裁剪与滚动下放到唯一的内层容器(`#viewer-dialog`/`#project-models-dialog`/`#ask-dialog`,浏览器冒烟检查「只有一个内层容器」)。
- **分隔条(split)**:`installSplit(pane, { id, side, min, max, key, title, ariaLabel, titleKey, ariaKey, onChange })` → `{ set, reset, sync, reapply, value, cssVar, pane, handle }`。宽度/高度写成 `<html>` 上的 `--kz-split-<id>`,style.css 用 `var(--kz-split-<id>, 默认值)`。手柄是 `role="separator"` 的 `.resize-handle`(可 Tab,方向键 ±8px,Home/双击复位,带 `aria-valuenow/min/max`;窗格有 id 时 `aria-controls` 指向它)。`title`/`ariaLabel` 是调用方按当前语言译好的文案,`titleKey`/`ariaKey` 是词条键,记到手柄的 `data-i18n-title`/`data-i18n-aria-label` 上,运行中切语言由 `applyLanguage` 重译(只传译文的话,切到英文后读屏名还是中文)。`max` 可以是函数,每次夹紧与同步 `aria-valuemax` 时重算。现有:左侧栏 `sidebar`(220~460,沿用旧键 `kz-sidebar-width`)、运行日志高度 `log`、文件树 `files`(上限 = 文件页宽 − 360,给编辑器留位;读屏名「调整文件树宽度」,见 [files_editor.md](files_editor.md) §6)、记忆列表 `memory`、后台任务侧栏 `tasks`(左缘;停靠时 `[320, min(760, 主区宽 − 600)]`,抽屉时 `[320, 主区宽 − 96]`,subagent_presentation §5.6)。
- **持久化**:`setFrameStore(store)` 注入存储,接口 `get(kind, id, hint)` / `set(kind, id, value|null, hint)`,`kind` 为 `frames` 或 `splits`,读写一律吞异常。应用里由 `ui/03-layout.js` 接到 `ui_prefs` 的 `ui_layout`(`crates/kanzei-app/src/prefs.rs`:任意 JSON,两级合并写入——按分区、再按键整体替换,`null` 删除,单次负载超过 64 KiB 整次丢弃;本机 WebView2 的 localStorage 重启即丢,D-404):启动时读一次,改动 400ms 去抖后只写变化的键,页面隐藏时立即写;localStorage 只作 try/catch 包着的缓存。样例页与冒烟用默认的 localStorage 存储。`ui_layout` 形如 `{ frames: { <框 id>: {v,l|r,t|b,w?,h?,kw,kh} }, splits: { <id>: px }, side_panel: { auto_open, auto_close } }`,后者是后台任务侧栏的两个设置开关。
- **零 import**:样例页与假 DOM 冒烟要能单独加载;需要翻译的文案(手柄提示)由调用方传入。

## 5. 唯一写法(给人和弱模型的决策表)

| 需求 | 唯一写法 | 从哪里引入 |
|---|---|---|
| 问是或否 | `await confirmDialog({ title, message, list?, okText?, danger? })` | ./01-core.js |
| 要用户输入一行 | `await inputDialog({ title, message?, value?, placeholder? })` | ./01-core.js |
| 展示长文或报告 | `openRuntimeMarkdown(title, md)` / `openDocViewer(kind)` | ./15-views-misc.js |
| 静态按钮弹出一组开关或动作 | HTML:`<button data-kz-menu="x-menu">…</button>` + `<div id="x-menu" popover="manual" class="k-surface k-menu hidden" data-placement="top-end">…</div>` | 不需要 JS,启动时 bindMenus 自动接线 |
| JS 里临时弹出动作列表 | `openMenu(anchorEl, items, { placement })` | ./00-surface.js |
| 弹窗(`<dialog>`)里的菜单 | JS 菜单照用 `openMenu(anchorEl, …)`(自动挂进锚点所在的 dialog);静态菜单把 `data-kz-menu` 触发器和它的 popover 弹层**都写在同一个 `<dialog>` 里** | ./00-surface.js |
| 锚定在某元素旁的信息浮层 | `openPopover(anchorEl, el, { placement })` / `closeSurface(el)` | ./00-surface.js |
| 需要持续引起注意的停靠卡片 | `showCard(el, { onEscape })` / `hideCard(el)` | ./00-surface.js |
| 一句话反馈 | `toast(t("…"), { kind })`;长错误用 `toastError` | ./03-shell.js |
| 给控件加说明 | `title=` 加 `data-i18n-title`,tooltip 层自动接管 | 不需要 JS |
| 下拉选择 | 原生 `<select>`;视图 CSS 只写尺寸和按钮态(输入区里加 `.kz-ctl`,垂直居中已由基础 select 规则保证) | 不需要 JS |
| 输入区(composer)里的按钮/下拉/开关 | `class="kz-ctl"`(图标按钮再加 `kz-ctl--icon`,发送 `kz-ctl--round`);下拉箭头用 `<span class="kz-chev">` | 不需要 JS,见 §11 |
| 常驻侧面板 | `<aside class="k-surface k-panel" data-dock="side">`(地标,不写 role=dialog);视图 CSS 只写网格位置、宽度与内部排版;显隐/停靠形态只由它的唯一写入者切(后台任务侧栏:`reconcileTasksPanel`) | ./06-agent-panel.js |
| 让弹窗/卡片可拖动、可调尺寸 | index.html 上写 `data-kz-frame="<id>"` + `-move`/`-edges`/`-min`/`-keep`/`-persist`(§4.6),不写 JS | 启动时 bindFrames 自动接线 |
| 两栏之间可拖的分隔 | `installSplit(pane, { id, side, min, max })`,CSS 用 `var(--kz-split-<id>, 默认值)` | ./00-frame.js |
| 界面布局偏好(跨重启) | `layoutPref(section, key)` / `setLayoutPref(section, key, value)`(经 ui_prefs 的 ui_layout 落 app.json);不要直接用 localStorage | ./03-layout.js |

**禁止**:新增 `position:fixed` 浮层;自带遮罩(侧栏抽屉用 `.k-scrim`);自己 `setPointerCapture` 写拖动或调尺寸、写 `--kz-frame-*`/`data-kz-placed`(只准 00-frame.js);在视图 CSS 里给弹层写底色、边框、圆角、阴影、z-index、position;`window.alert/confirm/prompt`;把 `<details>` 当下拉用;document 或 window 级的 Esc 监听;直接切换弹层的 `.hidden`;直接调 `showModal/showPopover/hidePopover`。

## 6. 迁移清单(已全部落地)

| 现状 | 现在 |
|---|---|
| 原生 select 的弹出列表 | surface.css `appearance: base-select` + `::picker(select)`;调用点零改动 |
| `#confirm-overlay` / `#input-overlay` + 01-core 各自挂 document keydown | `<dialog class="k-surface k-dialog" closedby="any">`;逻辑在 00-surface,并发排队;内层 `#confirm-dialog`/`#input-dialog` 保留为纯排版容器(保 id) |
| `#viewer-overlay` + 遮罩 click + 07-events 的 Esc 分支 | `<dialog data-size="lg">`,openDialog;`#viewer-dialog` 为内部纵向排版 |
| `#palette` + 手写 inert + 遮罩 mousedown + window 级 Esc | `<dialog data-size="palette">`,showModal 原生惰性化;window 监听只剩 Ctrl/Cmd+P(其它模态开着时不叠加) |
| `#ask-overlay` 全宽 fixed 条 + document 级 Esc 拒绝 | `popover="manual"` 的 `.k-card[data-tone=attention]`,showCard/hideCard,`onEscape` 拒绝/取消;内容元素(`#ask-action`/`#ask-resource`/`#ask-remember`/问题区)未动 |
| `#ask-reopen` | `popover="manual"` 的 `.k-chip-float`(不响应 Esc) |
| `#toast` 单槽 | 区域 + 条目,最多 3 条,`kind` |
| `details#composer-more` / `#task-options` / `#autorun-more` | `<button data-kz-menu>` + `#composer-more-menu` / `#task-options-menu` / `#autorun-menu` 弹层菜单;删 `placeAutorunMenu` 手算定位;鞭挞数字快捷键挂在触发器与菜单上、以菜单开着为前提;搜索框宿主判断改为 `closest("[popover]")` + 经原语打开 |
| `#sop-picker-panel` / `.context-detail` / `.file-suggestions` / 语音设置 details | 锚定浮层(openPopover;文件补全为 manual、锚在输入框、Esc 经栈收起);删 07-events 的 document 级 click/Esc |
| `#bg-panel` / `#agent-panel` 逐字重复的外观 | 先收敛为 `.k-surface.k-panel`;UI2-0926 #14 起两块浮层合成停靠的 `<aside id="tasks-panel" class="k-surface k-panel" data-dock>`(`#main` 网格第 2 列;窄时抽屉 + `.k-scrim`),见 subagent_presentation §5.6 |
| 弹窗/卡片固定尺寸、拖不动;侧栏/文件树/日志各写一套拖拽(侧栏写内联宽度,收起后留空栏) | 00-frame.js 统一:框用 `data-kz-frame*`,分隔条用 `installSplit`,几何经 `ui_layout` 持久化(§4.6) |
| 约 200 处 `title` | installTooltips,调用点零改动 |
| 主题块外 14 处非弹层 `rgba()` | 语义 token(`--diff-*-bg`、`--danger-soft`、`--alert-soft` 等)或 `color-mix(in srgb, var(--token) N%, transparent)` |
| 模态期间全局快捷键照常生效 | 08-compose-runtime 全局快捷键开头 `if (isModalOpen()) return;` |

暂缓、待用户拍板:移动端 PWA 是否换成同一套 token(现为独立的亮底蓝调);WebView2 默认右键菜单是否在非文本区域屏蔽。

## 7. 门禁

全部挂在现有步骤里:`ui_a11y` 负责静态规则,`ui_lint` 负责 ESLint 与浏览器冒烟。verify.ps1、ci.yml 与 git.rs 守护测试的检查键集合不变。

### 7.1 ESLint(eslint.config.js)

`crates/kanzei-app/ui/*.js`(00-surface.js、gallery.js、oc-studio.js 除外)加 `no-restricted-syntax` 8 条,级别 error:原生 `alert/confirm/prompt`(含 `window.`/`globalThis.`)、`createElement("dialog")`、直接调 `showModal/showPopover/hidePopover/togglePopover`、`setAttribute("popover"|"popovertarget")`、给 `popover`/`popoverTargetElement` 赋值、`style.position = "fixed"`、document/window 级 keydown 监听里判断 `"Escape"`。报错信息写明应改用的写法。

### 7.2 静态规则(scripts/ui-surface-rules.mjs,由 ui-a11y-smoke 调用)

替换了只认 hex 的旧判据。解析:先把注释换成等长空白(行号不漂移),再取最内层规则;逗号分组与「主体」(最后一个复合选择器)的拆分跳过括号内的逗号与空格。

- **C1 字面量色**:style.css 主题块结束标记之后、surface.css 全文(PWA 样式表有了自己的 token 块之后才查)的声明值里不得出现 hex、颜色函数(`rgba/rgb/hsla/hsl/hwb/lab/lch/oklab/oklch/color(`,`color-mix(` 不算)、CSS 颜色名;`mask-image` 豁免。
- **T1 token 分层**:组件层名字只准 surface.css 使用;不在 `[data-theme="light"]` 里重定义;surface.css 引用的每个 token 在 style.css 或 surface.css 里有定义(00-frame.js 运行时写入的 `--kz-frame-l/r/t/b/w/h` 豁免);不出现 `--c-*`。
- **S1 外观归属**(style.css):选择器含 `dialog`/`::backdrop`/`::picker(`/`:popover-open`/`[popover`/`.k-*`/`option` 即违例;主体是弹层宿主(`#confirm-overlay`、`.autorun-menu` 等)时不得声明底色/边框/圆角/阴影/backdrop-filter/z-index/position/inset;`#tasks-panel` 不得声明底色/边框/阴影(停靠与抽屉的外观在 surface.css 的 `.k-panel[data-dock]`);不得按 `[data-kz-placed]`/`[data-kz-frame]` 选择、不得引用 `--kz-frame-*`(框的落位只在 surface.css §10);`position: fixed` 只许 `.resize-handle`;不得引用已删的高位 z;模糊 ≥16px 的大阴影只许 `#composer`、`#composer:focus-within`、`#sidebar:not(.collapsed)`;外观属性不得 `!important`;index.html 里 `<details>` 的 id/class 作主体时不得 `position: absolute|fixed`。
- **H 页面结构**(index.html):`role="dialog|alertdialog|menu|tooltip"` 的宿主须是 `<dialog>` 或带 `popover`(白名单已删:常驻侧栏 `#tasks-panel` 是 `<aside>` 地标);`data-kz-frame` 只准挂在 `<dialog class="k-dialog">`(命令面板除外)或 `.k-card` 上,id 非空且唯一,有 `data-kz-frame-*` 就必须有 `data-kz-frame`,`data-kz-frame-edges` 只能是 `all` 或 `n e s w ne se sw nw`;每个 `<dialog>` 与 `[popover]` 必带 `k-surface`;`<dialog>` 里的 `data-kz-menu` 触发器,对应弹层必须写在同一个 dialog 内;输入区菜单不得再是 `<details>`;`<select>` 不带 `multiple`/`size`;不写内联颜色样式。
- **P 预览遮挡**(UI2-0926 #8):surface.css 里选择器分支等于 `.k-card` / `.k-chip-float` / `.k-toast-region` 的规则至少一条声明 inset,且每条 inset 都引用 `--surface-safe-right`;index.html 的 `#preview-host` 是空元素(开标签后紧跟闭标签);style.css 不得给 `#preview-host` 背景;`<section id="preview-dock">` 里不得出现 `.k-card/.k-chip-float/.k-toast-region/.k-panel/.k-scrim`。
- **J 脚本**:J1 除 00-surface.js 外不得 `$("<弹层 id>").classList.add|remove|toggle("hidden")`,也不得先取进局部变量再切(`const detail = $("context-detail"); … detail.classList.remove("hidden")`,同一顶层函数体内配对);J2 `ui/[0-9]*.js`(22-neural-flow 与 22-oc-* 除外)不得给 `.style.background/color/…` 赋字面量;J3 00-surface.js 与 00-frame.js 零 import;J4 拖动/调尺寸只有一个入口:编号脚本(22-oc-* 角色工作室除外)不得出现 `setPointerCapture(`、`--kz-frame-`、`data-kz-placed`,只准 00-frame.js。

每条违例输出「文件:行、原文、改用什么」,同一改法只说一遍。模块自带反例自测(`selfTestSurfaceRules`),每条规则喂一条必须命中的样本,任何判据恒绿先在 a11y 冒烟里红。

### 7.3 浏览器样例冒烟(scripts/ui-surface-gallery-smoke.mjs,由 ui-lint-smoke 在 ESLint 之后调用)

playwright-core `channel: "msedge"` 无头模式;页面经 `scripts/ui-preview/server.mjs` 的本地服务打开(ES 模块不能从 file:// 加载)。

1. 特性探测:dialog、Popover API、`anchor-name`、`position-area`、`appearance: base-select` 任一缺失即失败(提示需要 WebView2/Edge ≥ 135);`closedby` 与 `popover="hint"` 只打印(有退化路径)。
2. gallery.html 暗/亮两套主题,1280×840,逐个演示:计算底色/圆角/阴影等于探针解析出的对应 token(模态 lg + shadow-3,卡片 md + shadow-3,tooltip bg-raised + sm + shadow-1,芯片 pill,其余 md + shadow-2);正文对比度 ≥ 7,`--surface-muted` 对底色 ≥ 4.5;包围盒在视口内(容差 1px);另在 800×500 只查几何。
3. 下拉:点开普通、透明芯片、30 项长列表与菜单内嵌的下拉,确认 `select:open`,截取列表内部一块求平均相对亮度:暗色 < 0.2、亮色 > 0.6。列表下面垫着一块反色「金丝雀」底块,列表没画进页面(base-select 失效)或画成白底都会红。
4. Esc 与叠放:卡片上再开确认框,一次 Esc 只关确认框、卡片仍在、栈深恰好减 1;菜单里开着下拉列表时一次 Esc 只关列表(菜单仍在、栈深不变),第二次才关菜单;同一确认框并发两次,确认第一个后第二个接着打开且保持打开。
   弹窗里的菜单:JS 菜单挂在 dialog 内,真实鼠标点击菜单项走到 onSelect 且只关菜单;静态 data-kz-menu 菜单里的勾选框勾得上;Esc 先关菜单再关弹窗。补全列表与宽锚点同宽且左缘对齐(锚点 >420px)。键盘打开查看器同构弹窗时初始焦点不弹 tooltip(新开一个鼠标没进过的页面测,另有键盘聚焦出提示的对照)。
5. index.html 运行时对照(预览服务注入模拟 IPC):所有 select 计算为 `base-select`;所有 `dialog, [popover]` 带 `k-surface`;除 `.resize-handle` 外没有顶层以外、正在显示的 `position: fixed` 元素。
6. 可调框(暗色一轮,`// ── 分区:后台任务侧栏与可调框 ──`):真实鼠标从右边框外 3px 拖 +100 只加宽、左缘不动、弹窗不被轻关闭;拖标题栏只移动;一步甩动到位;上边拖出窗外夹在 8px;关掉再开保留几何并写进存储;窗口缩到 800×500 夹进视口、恢复后回到原几何;双击标题复位并清存储;Alt+Shift+→ 加宽 24;点遮罩照常关闭;卡片拖标题移动、左缘加宽右缘不动、标题上的普通点击目标不变;菜单/浮层/提示/toast 里没有手柄。index.html 对照:框清单恰为 `ask,confirm,input,project-models,viewer`,可调尺寸的框只有一个内层容器。
   后台任务侧栏(同一分区,步骤 8,1600×900 暗色):侧栏停靠时权限卡左右边与 `#composer` 重合(±1px)、在输入区上方、不压侧栏;收起成「重新打开询问」芯片后芯片右缘 ≤ 侧栏左缘;「筛选与清理」菜单里原生下拉的列表开着时,第一次 Esc 只收列表(侧栏与菜单都在)、第二次只收菜单;2000 宽(侧栏 520)小表保留模型列,1280 宽(侧栏 352)隐去模型列、子代理列占行宽 ≥ 55%(容器查询阈值 440)。`KZ_SMOKE_MUTATE=askComposerAnchor|chipDockRight|escPopoverBrowser|tasksNarrowModel` 经 `page.route` 把被守护的源码改坏后再跑,四条均已实跑变红(变异没恰好命中一处直接报错)。
7. 输入区控件几何(UI2-0926 #11,`scripts/ui-composer-geometry.mjs`,见 §11.4):1100@1、1280@1.5、1600@1.25、2000@1 × 暗/亮 × 运行/空闲(另加超长项目名),每次运行附带三种注入回归的自检。
8. 截图写入 `dist/ui-gallery/<theme>-<demo>.png`、`matrix.png`、`<theme>-index.png`、`dark-frame-dialog.png`、`dark-frame-card.png`(dist 已在 .gitignore)。

已实测:删掉 base-select 那一行,或去掉 §4.4 的迟到 close 守卫,这份冒烟都会红。ui_lint 步骤因此增加约 20 秒并依赖本机 Edge(本机与 windows-latest 都有)。

## 8. 样例页

`ui/gallery.html` + `ui/gallery.js`:只引入 app.css、00-surface.js 与 00-frame.js(两者都零 import),不依赖应用其它模块与 Tauri。顶部主题切换、「逐个打开」「全部关闭」;演示:确认/危险/三键确认、输入框、大尺寸查看器、命令面板尺寸、静态 data-kz-menu 菜单(行式 + 数字快捷键)、JS 菜单(分隔线/选中/禁用/危险/快捷键)、**container-type: inline-size 容器里向上弹出的菜单**(复刻输入区环境)、菜单内嵌下拉、**弹窗内 JS/静态菜单**、**宽锚点补全列表**、锚定浮层、停靠卡片 + 浮动芯片(两者都是可调框:大尺寸查看器八向、卡片左右调宽)、四种 toast、短/多行长提示、普通/透明芯片/30 项(含禁用项)下拉、`.k-panel`;底部暗/亮两套 `.k-static` 静态矩阵同屏,矩阵里另有停靠态/抽屉态的 `.k-panel[data-dock]` 与 `.k-scrim`。暴露 `window.__gallery = { demos(), open(id), measure(id), tokens(), stackDepth(), closeAll(), setTheme(theme), picks(), frames(), resetFrames() }`。演示文案经恒等的 `t("…")`,复用应用已有词条(i18n 冒烟据此校验词条仍在资源表里)。

## 9. 运行时冒烟(假 DOM)

- 基础设施:`Element` 增加 `showModal/show/close/showPopover/hidePopover/togglePopover`(维护 `open/_modal/_popoverOpen`,派发 close/toggle);document 的 `addEventListener` 识别捕获阶段且先于冒泡执行,`dispatchEvent` 遵守 `stopImmediatePropagation`;HTML 解析把 `popover`、`data-kz-menu`、`data-placement` 等属性建到桩上。
- 用例(`// ===== 分区:弹层与外观 =====`):确认框(showModal、镜像、结果、栈清空、safe、并发排队);Esc 只关栈顶(权限卡 + 确认框:第一次 Esc 只关确认框、`answer_ask` 不变、权限卡仍在;第二次拒绝;收起后的芯片不响应 Esc);卡片让位局部 Esc(焦点在 #prompt、卡片外输入框、Monaco 内时不拒权限、不截断传播;勾选框上仍拒绝);输入框(组合中的 Enter 不提交、Enter 返回值、Esc 返回 null);openMenu(role、禁用项、先关再 onSelect 且恰好一次、同时只开一个、同锚点再调收起);弹窗里的菜单(挂进 dialog、点项不关弹窗、Esc 先关菜单、关弹窗连带关菜单、模态外静态弹层告警);bindMenus(四个触发器接线、aria-expanded、Esc、点外关闭、按下触发器不算点外);toast(role、kind、最多 3 条、过期后收起但文案留存);tooltip(title 挪进 data-kz-tip、#kz-tip、aria-describedby、移开恢复);静态护栏(全局快捷键的 isModalOpen 守卫、placeAutorunMenu 不得复活、Esc 唯一入口在捕获阶段、不得直接把焦点抢到 `#ask-allow`)。
- 改写的旧断言保留原意图:命令面板的 inert 断言改为「必须以模态打开」(`open && _modal`,关闭后镜像 `.hidden`)并加「不得再手写 inert」反证;搜索框宿主静态锁改为 `closest("[popover]")` + `openPopover`;任务设置结构断言改按 `#task-options-menu` 定位。
- 变异守卫 `KZ_SMOKE_MUTATE=surfaceEscTop`:把 `const top = topEscapable(…);` 换成取栈底,冒烟必须红(已实跑:第一次 Esc 没关确认框、权限请求被拒)。`surfaceCardYield`:删掉卡片让位判断(已实跑:焦点在 #prompt/输入框/Monaco 时权限请求被拒)。`surfaceMenuInDialog`:openMenu 退回一律挂 body(已实跑:弹窗里的菜单未挂进 dialog)。
- 可调框与侧栏(`// ── 分区:后台任务侧栏与可调框 ──`):纯几何(`dragRect`/`prefFromRect`/`placeFromPref` 的八向、夹紧、左右半屏锚定)、`bindFrames` 接线与手柄、阈值前不捕获、双击复位、Alt+Shift 键盘、不记几何的框关闭即复位、存储抛错不崩、分隔条写 `<html>` 变量、`ui_layout` 去抖写后端与远端偏好到达后重放;后台任务侧栏的策略场景、停靠/抽屉、唯一写入者、rail 徽标与可访问名。变异守卫 `frameClamp`/`frameGestureDoc`/`frameDblclickFlag`/`frameTransientReset`/`frameStorageGuard`/`splitCssVar`/`layoutPersist` 与侧栏的 8 条(subagent_presentation §11.3)均已实跑变红。
- a11y 冒烟:分隔条 `role=separator` 与 aria 值、`#tasks-panel` 是 `<aside>` 地标、`#tasks-toggle` 的 `aria-controls/expanded`、旧 `#bg-panel`/`#agent-panel`/`#agent-toggle`/`#activity-toggle` 不得复活、`#main` 网格与抽屉 absolute、失败徽标 `--bg` on `--err` ≥ 4.5;`--kz-frame-*` 列入运行时 token。窄窗口布局冒烟(并入 ui-lint 链)在 7 个视口 × 左侧栏开合 × 后台任务侧栏开合 × 日志开合下断言停靠/抽屉判据、对话列 ≥ 600、无横向滚动。

## 10. 风险与回退

- base-select 改变了 select 按钮的固有样式与 `::picker-icon` 箭头;ui-narrow-layout-smoke 在 5 个视口下复核通过。回退:删掉 surface.css 的 `select, ::picker(select) { appearance: base-select; }` 一行,`option` 的底色兜底让经典弹出列表仍保持暗色。
- style.css 被 `@layer(app)` 包裹后优先级低于任何未分层样式(见 §4.2)。
- 假 DOM 没有顶层,也把按 id 建的节点拍平在 body 下:菜单内部行(如鞭挞数字快捷键命中哪一行)只能静态锁接线点,真实行为由浏览器样例冒烟兜底。
- tooltip 接管 title:出现时机变为 450ms,长说明完整显示;scroll/keydown/pointerdown/失焦/页面隐藏都会隐藏,避免卡住。悬停期间元素的 title 暂时为空(依赖 title 作可访问名称的元素此时有 aria-describedby 指向提示)。
- 权限卡的聚焦策略变为「正在输入时不抢焦点」,属于行为变化,需写进发版说明。
- `closedby` 与 `popover="hint"` 不可用时有退化路径;base-select 与锚点定位不可用时浏览器冒烟直接失败。
- 可调框:用户拖过的几何压过默认摆放(含权限卡锚在输入区上方),窗口变化只夹紧不改写存储;双击标题或 Alt+Shift+Home 复位。回退:删掉 index.html 上的 `data-kz-frame*` 属性,框回到固定尺寸,其余不受影响。`ui_layout` 依赖新版 kzapp(Rust 侧 `ui_prefs_set` 多一个参数);旧后端收到时忽略,几何只剩本次会话。
- 后台任务侧栏停靠时对话列变窄:权限卡跟着输入区走,右下停靠的卡片与芯片按 `--kz-dock-right` 让开;对话列不足 600px 改抽屉,不自动弹出。

## 11. 输入区控件几何(UI2-0926 #11)

用户截图 13:「这里的渲染也有问题」——模式芯片文字贴在上半截、看上去「异常高」;鞭挞组两层描边错位。实测输入区控件有 7 种高度(15/16/19/24/26/27/28/30/32)、4 种字号,没有「控件高度」这个概念;旧控件多由高特异性的 ID 规则各写各的尺寸,任何统一类都压不过。

### 11.1 根因

| # | 根因 |
|---|---|
| A | base-select 的 `align-items: normal`(见 §4.5),固定 30px 高的模式芯片文字与 ▼ 顶在上半截;它又是那一排唯一有实底的控件,把错位放大成「异常高」。用户说的「盖住项目名和模型首字母」实为截图上红框线压字,芯片本体没有叠压邻居。 |
| B | `::picker-icon` 只上了色,形状仍是 UA 实心 ▼;其它触发器用文本「▾」,模型/思考芯片没有箭头——一行里三种下拉指示。 |
| D | 鞭挞组容器 `.autorun-bar` 在 armed/running 时自己画圆角边框与底色,左右内边距却是 0;内部「鞭挞」又是带描边的胶囊,两层描边相切叠成双线,「推进中」贴着右框。 |
| E | 鞭挞组四个成员四种高度(26/27/19/15)、三种字号;轮次徽标为一条常态宽度为 0 的进度条留了不对称内边距,数字上偏。 |
| F | 没有控件几何 token;每个控件各写尺寸,多由 ID 规则写死。 |
| G | 停机原因胶囊是 inline-flex,匿名文本画不出省略号;`#hint` 无写入方;`.ctx-project::after` 动画挂在已删的伪元素上。 |

### 11.2 结构(参照 Claude Code 桌面 / Codex)

```
#composer(卡片,container: composer / inline-size;宽度 = 对话列表达式,见 chat_presentation_contract.md §4.4)
├─ #composer-context 上下文带:项目名 · ⎇ 分支 ……………… [N 个文件 +a −d ⌄](#change-bar)   ← 卡片顶部通栏 + 下方细线
├─ #change-bar-files 展开的文件清单(带下方通栏)
├─ 附件条 / #prompt / 继续文案编辑区 / 排队条 / 语音面板
└─ #composer-bar 工具行(单行;放不下时右段整体换到第二行靠右,不隐藏任何控件)
   ├─ .composer-left :[＋ 附件] [模式 ⌄] [● 鞭挞] [N 轮 · 阶段 ⌄](= 鞭挞设置触发器) 停机原因… [继续鞭挞]
   └─ .composer-right:[模型 ⌄] [思考 ⌄] [⚙ 任务设置] [⋯ 更多] [继续](仅空闲)|[排队 ⌄](仅运行) [🎤] [停止](仅运行) [↑ 发送]
```

- SOP 是「更多」菜单首项(点开后收起菜单、以「更多」触发器为锚弹出 SOP 列表);继续文案是鞭挞菜单「刹车」分区末尾一行。
- 「继续」与「排队」占同一个位置,只读 `html[data-kz-activity]`:运行/停止中显示交付方式,其余显示「继续」(鞭挞轮间 pending 显示「继续」,与 sendText 在 running=false 时不看交付方式一致)。交付方式选项去掉英文尾巴(「排队」「插入」),完整说明在悬停提示。「排队」这个 key 已译作状态词 Queued(工作区卡片「排队 N 条」),交付方式要的是动词 Queue:选项 key 写「排队 queue」,中文显示走 `data-i18n-zh="排队"`(02-i18n.js `applyDataI18nKeys`:同字不同义时中文显示与 key 分开写)。
- 来源标签只标偏离默认的来源(全局/内置不占位,完整来源仍在 title / aria-label / 菜单里):临时 / 项目级 / Agent。项目级只在运行态收起(见 §11.3 窄宽),空闲态照常显示。
- 模式芯片不再有实底:自主推进是强调色文字(进行中家族),结伴与研究中性(语义色表见 ui_color_semantics.md)。
- 停机原因是纯文字(`inline-block` + `line-height: 28px` 才画得出省略号,上限 24ch);附件芯片 = 名字(省略号)+ 单独的 × 移除键。
- 语音键是麦克风图标:23-voice.js 只写 title / aria-label / data-i18n-*,不再写 textContent(会冲掉图标)。

### 11.3 几何(唯一真源 `.kz-ctl`)

- token(style.css `:root` 非颜色区):`--ctl-h: 28px`、`--ctl-h-lg: 32px`、`--ctl-px: 10px`、`--ctl-gap: 4px`、`--ctl-group-gap: 12px`;`--icon-chevron` 为 data:svg 遮罩(遮罩只取 alpha,SVG 里的描边色无关紧要,C1 不命中)。
- `.kz-ctl`:`inline-flex`、居中、高 28、左右 10、胶囊、透明底、无边框、12px、字重 400、单行。悬停 `--surface-hover` + `--fg-strong`;`[aria-expanded=true]` / `[aria-pressed=true]` / `:has(> input:checked)` 用 `--surface-selected`;焦点 2px `--focus-ring` 内描边。一律中性,不染强调色。
- 变体:`.kz-ctl--icon` 28×28(svg 16px);`.kz-ctl--round` 32 圆(发送,强调色填充归「发送键」规则);`#stop` 同高胶囊 + `--danger` 字 / `--danger-soft` 底,保留文字。
- 箭头:`.kz-chev`、`.picker-btn::after`、`select::picker-icon` 共用 10px 细 V 形,展开翻转。
- 鞭挞组:容器不画框不加底(`.autorun-bar` 的 border / background 只准 0 / none);开关的 `::before` 是 7px 圆点——关 = 1.5px 空心描边;开着待命 = 中性实心(`currentColor`,胶囊的 `--surface-selected` 底已说「开着」);推进中 / 等待下一轮 = `--accent` 实心(进行中家族,推进中呼吸)。绿只表示「一件工作成功收尾」,开关打开不是收尾(ui_color_semantics.md §3;复核前圆点是 `--ok`)。触发器未开时收成 28×28 纯箭头,开着显示「N 轮 · 阶段」,推进中不重复「推进中」(活动行已说);等待下一轮的阶段字 `--accent-text`(与活动行、kz-dot pending 一致),已暂停 `--warn`。触发器的读屏名由 08-auto.js 写(「鞭挞设置 · 鞭挞轮次 N · 阶段」),静态 data-i18n-* 摘掉以免切语言被冲回。
- 窄宽以 composer 自身为查询容器(列宽由对话列 token 决定,视口宽度不再代表 composer 宽度):≤860px 模型芯片上限 200px;≤640px 收全部来源标签、模型 150 / 思考 110;再窄右段整体换行。项目级来源标签只在运行态收起(`html[data-kz-activity=running|stopping]`):运行态右段多出 [排队 ⌄] 与 [停止],768 列宽里只剩约 30px 空位;空闲态约 127px,放得下。复核前它挂在 860 容器查询里,输入区最宽 768 → 任何宽度都看不到。768 列宽下中文空闲态与运行态都是单行。
- 英文界面:「Self-directed progress」「Auto-run」「0 rounds」偏长,1280@1.5、1600@1.25、2000@1 三档下 768 列宽都放不下一行(超出约 140px),按上面的设计回退——右段整体换到第二行靠右,不隐藏任何控件。这是有意的:用户主用中文,英文不为一行去截断模式名。
- 上下文带与文件清单用 `margin-inline: calc(-1 * var(--composer-px))` + `max-width: none` 通栏到卡片边缘,文字左缘与输入框正文对齐(`--composer-px + 6px`);选择器写成 `#composer > :is(#composer-context, #change-bar-files)`(2,0,0),压过 `#composer > *:where(…)` 的 auto 外边距与 `max-width: 100%`。
- 删除:`.ctx-select`、`.seg-btn` / `.seg-select`、`.composer-actions`、`.composer-secondary`、`#hint`、改动条里的分支与 ▸ 字形、`.auto-progress::after` 进度条与扫光、`.ctx-project::after` 死规则;线路页的模型下拉回到普通 select 外观。

### 11.4 门禁

- **浏览器几何**(`scripts/ui-composer-geometry.mjs`,由 ui-surface-gallery-smoke §6 调用,§7.3 第 7 项):① `#composer` 内可见 `.kz-ctl` 高 28±0.5、`.kz-ctl--round` 高 32±0.5;② 页面里全部 select 的 `align-items` 计算值为 center;③ 输入区可见控件(`.kz-ctl`、发送、项目名、分支、停机原因)包围盒两两不交且都在 `#composer` 内,超长项目名必须截断;④ 模式芯片元素截图在页内用 `createImageBitmap` + `OffscreenCanvas` 找墨迹纵向范围,中心偏离盒中线 ≤1.5 CSS px(只量中文:拉丁字母的升部/降部让墨迹天然不对称,居中的英文芯片也偏 1.7~2px);⑤ 输入区占满列宽(≥760)时工具行单行,英文界面改量「至多两行」;⑥ 交付方式「排队」选项中文显示「排队」、英文显示 Queue;⑦ 项目级来源标签空闲态显示、运行态收起。组合含英文 1280@1.5(运行 / 空闲)与 2000@1 亮色运行。测量期间收起 toast(只影响测量)。每次运行都注入四种回归(`select { align-items: normal }`、`#model-picker { height: 30px }`、`#profile-select { margin-left: -24px }`、项目级标签一律藏起[空闲态量]),任一没被判红即报「测量判据失效」;⑥ 另做过手工变异(去掉 `data-i18n-zh` 支持 / 选项 key 改回「排队」)实跑为红。基线(改前)在 1600@1.25 上 ② ④ 为红(51 个 select 为 normal,墨迹偏离 4.31px)。
- **颜色语义**(ui-a11y-smoke ⑥,ui_color_semantics.md §5):⑥d 中性前缀加 `#auto-continue-wrap`、`.auto-phase`(不带 `[data-phase]` 的静息态 = 待命,中性);⑥w 鞭挞组的圆点 / 轮次 / 阶段字不得引用 `--ok`,running / pending 的圆点背景必须是 `var(--accent)`,pending 阶段字必须是强调色家族。自带 4 个反例(待命绿点、删掉进行中圆点规则、pending 阶段字琥珀、待命圆点染强调色)。
- **静态**(ui-a11y-smoke「分区:对话单列与输入区」):基础 select 有 `align-items: center`;`--ctl-h: 28px` 存在且 `.kz-ctl` 高度走它;旧类(`.ctx-select` / `.seg-*` / `.composer-secondary` / `.composer-actions`)不再出现;`.autorun-bar` 与其 running/paused 变体不画框不加底;surface.css 的 `select::picker-icon` 用 `--icon-chevron`;`#delivery-select` 与 `#continue-btn` 各有一条按 `html[data-kz-activity]` 门控的规则。自带 7 个反例。
- **运行时**(ui-runtime-smoke 同分区 ⑫⑬⑭):源码配对标签扫描左右段控件顺序、带与工具行控件全是 `.kz-ctl`、「N 轮 · 阶段」在触发器里、SOP 是「更多」首项;行为断言触发器读屏名、无改动时的分支、语音键不冲掉图标;⑭(复核补)点 SOP 先收起「更多」菜单并以「更多」触发器为锚、展开继续文案时收起鞭挞菜单、附件芯片点名字不删 / 点 × 删、`data-i18n-zh` 与交付方式选项的中英文;变异守卫 `ctxBranchEarly` / `autorunTriggerLabel` / `voiceIconKeep` / `sopAnchor` / `continueClosesWhip` / `attachRemove` / `i18nZhDisplay`。R-342 断言改为「模式芯片在工具行左段、不在任何弹层」。

### 11.5 接缝

- 列宽与左右边归对话列(chat_presentation_contract.md §4.4);composer 只消费列宽表达式,自己的左右内边距走 `--composer-px`。
- 状态栏仍重复显示分支、模型、思考(非本节范围,记给状态栏负责方)。
- `#composer` 是 size container 后,锚在 composer 内的顶层弹层(鞭挞菜单、任务设置、更多、SOP 列表、文件补全)定位已在预览里复核。

## 变更记录

- 2026-09-26:起草并实施(release/2026-09-26-ui 分支,G3「弹层与外观」)。相对最初方案的调整:不引入 `--c-*` 原始层(组件层直接引用语义层);菜单/浮层用 `popover="manual"` + 模块统一点外关闭(替代 `popover="auto"`,理由见 §4.4);保留 `#viewer-dialog`/`#confirm-dialog`/`#input-dialog`/`#ask-dialog` 为纯排版容器以保住全部元素 id;浏览器冒烟新增「确认框排队」用例并据此修掉迟到 close 事件关掉排队弹窗的时序缺陷。
- 2026-09-26(UI2-0926 #11,ui2/chat 分支):§4.5 补 base-select 的两处坑(全局 `align-items: center`、`::picker-icon` 细 V 形遮罩);§5 决策表加输入区 `.kz-ctl` 一行;§7.3 加第 7 项输入区几何;新增 §11「输入区控件几何」。
- 2026-09-26(UI2-0926 #11 复核修复):鞭挞圆点待命改中性、推进中 / 等待下一轮改强调色,pending 阶段字回到 `--accent-text`(配色语义表落地后的口径;原方案「开 = --ok」写于配色落地之前),门禁 ⑥w;项目级来源标签改为只在运行态收起;交付方式英文改动词 Queue(`data-i18n-zh`);英文工具行两行回退写明并入几何测量;几何加 ⑥⑦ 与四种自检回归;运行时 ⑭ 补守卫。
- 2026-09-26(评审修正):弹窗里的菜单挂进锚点所在的 dialog(原先挂 body,模态开着时惰性、点不动)+ 门禁 H 组与模态外弹层告警;停靠卡片让位卡片外输入框/Monaco 的局部 Esc;程序化聚焦不弹 tooltip;补全列表放开 420px 宽度上限;toast 改浅底 + 软边;J1 覆盖局部变量写法。样例页与两份冒烟各补对应用例,手工变异均已实跑变红。

- 2026-09-26(UI2-0926 #4/#14):新增 §4.6 00-frame.js(框与分隔条唯一入口,几何经 `ui_prefs.ui_layout` 持久化)、surface.css §10 可调框落位;`#bg-panel`/`#agent-panel` 合成停靠的 `#tasks-panel`(`.k-panel[data-dock]`、`.k-scrim`、`--surface-panel-docked`、运行时变量 `--kz-dock-right`),H 组删掉 role=dialog 的侧面板白名单;权限卡锚在输入区上方。门禁新增 H 组可调框规则、S1 侧栏外观与框落位、T1 运行时变量豁免、J3 覆盖 00-frame.js、J4 几何手势唯一入口,均带反例自测。
- 2026-09-26(UI2-0926 #14#4 复核修复):`installSplit` 增加 `titleKey`/`ariaKey`(切语言重译);后台任务侧栏的分隔条上限按形态分开(抽屉 `主区宽 − 96`);侧栏自己的 Esc 监听跳过弹层内的按键(`[popover]` 里、或原生下拉列表开着),交给本模块与浏览器——此前「筛选与清理」菜单里下拉列表开着时按 Esc,00-surface 按设计放行,冒泡到侧栏被当成用户关闭。浏览器冒烟步骤 6 增补侧栏几何、芯片让位、弹层 Esc 与窄侧栏小表,并引入 `page.route` 变异守卫。
- 2026-09-26(UI2-0926 #6,ui2/files 分支):文件树分隔条上限改按文件页宽度(给编辑器留 360px)并换专属读屏名;`installSplit` 的手柄加 `aria-controls` 指向窗格。文件页编辑的弹层(未保存确认、新建文件输入、保存反馈)全部走本模块的 confirmDialog / inputDialog / toast,设计见 [files_editor.md](files_editor.md)。

- 2026-09-26(UI2-0926 #8,ui2/webfe 分支):00-surface.js 新增 `onSurfaceChange`/`surfaceElements`(网页预览冻结用的零依赖栈变化钩子);surface.css 常驻浮层宿主的 inset 右值加 `--surface-safe-right`(组件层默认 0px);门禁新增 P 组预览遮挡规则,带 4 条反例自测。

## 验证证据

- `node scripts/ui-a11y-smoke.mjs`(含 ui-surface-rules 与反例自测)、`ui-i18n-smoke`、`ui-markdown-smoke`、`ui-lint-smoke`(含 ESLint 8 条与浏览器样例冒烟)、`ui-connectivity`、`parallel-lines-regression`、`ipc-event-smoke`、`node --experimental-vm-modules scripts/ui-runtime-smoke.mjs`、`ui-narrow-layout-smoke` 全部通过;`KZ_SMOKE_MUTATE=surfaceEscTop` 实跑为红。
- UI2-0926 #4/#14:同一组门禁加 `check-design-freshness`、`ui-workspace-smoke` 全部退出码 0;可调框与侧栏的 15 条变异逐条实跑变红;`cargo fmt --check`、`cargo clippy -p kanzei-app --all-targets -D warnings`、`cargo test -p kanzei-app`(含 ui_layout 两级合并/超限丢弃/旧 app.json 回落三条)通过。

## TODO 与后续风险

- 第二波新增的弹层与菜单(模型芯片菜单与项目模型弹窗、需求焦点卡的「⋯」菜单与单页「筛选/更多」)必须用 00-surface 的 API,门禁会挡住 `<details>` 弹层与自写 fixed;后台任务侧栏已是停靠的 `<aside>`,不再是浮层。
- 新增的模态弹窗默认不可调:需要时在 index.html 补 `data-kz-frame*`(§4.6),并把 id 加进浏览器冒烟的框清单。
- 移动端 PWA 的 token 化与 WebView2 右键菜单等用户拍板后再做。
