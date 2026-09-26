# 架构图:好看、agent 能改、一个渲染器

- 身份: live_design
- 状态: 设计基线(2026-09-26 起草并随 ui2/arch 分支实施,本文描述的是已落地的实现)
- 日期: 2026-09-26
- 上游文档: [agent_visualization_tools.md](agent_visualization_tools.md)(R-335 草案,架构图一侧的待拍板项 1-3 由本文定下)、[ui_color_semantics.md](ui_color_semantics.md)(--diagram-* 在语义色表里的位置)、[ui_surface_stack.md](ui_surface_stack.md)(查看器与源码弹窗走 00-surface)
- 关联需求: 无(用户 2026-09-26 UI 问题清单第 7 条;tracker 条目待登记)
- 关联缺陷: 无
- 一句话: 架构页的图是自绘的三列网格 SVG——没有分层布局、边一律直线、文字不设颜色(暗色下黑字压深灰)。现在换成 Mermaid 12(ELK 分层布局,配色只由 --diagram-* token 注入):crate 依赖图由 Rust 从 Cargo 清单实时生成,手写图是 `docs/architecture/*.md` 里的 mermaid 文本,agent 用普通 write/edit 改、`architecture {action:"diagrams"}` 自查;同一个渲染器也接管了聊天、查看器、研究页里所有闭合的 ```mermaid 围栏。

用户原话:「架构浏览怎么是这种很丑的图要那种很好看的图，而且方便agent编辑的」。

## 1. 症状与根因

| # | 症状 | 根因(改前代码) |
|---|---|---|
| 1 | 线全交叉在一起 | `19-arch.js` 按入度排序后把节点塞进固定 3 列网格(`idx % 3`),依赖边一律从源节点底边中点画直线到目标顶边中点,目标在上方或同行时穿过节点 |
| 2 | 字几乎看不见 | text 没设 fill,两个主题下都是黑色;样式表里没有一条 `#arch-graph` 规则,也没有 hover/focus |
| 3 | 下方大片空白、文档树被挤到 118px | 画布高按「每个节点一行」算(`N * 56`),实际只用 ceil(N/3) 行 |
| 4 | 60 篇已入册文档全没显示,只剩「未入册(5)」 | 磁盘上的索引是 CRLF,按 `"\n"` 切行后章节标题正则 `/^#{2,3}\s+(.+)$/` 的 `.` 吃不了行尾 `\r` → 分组全落空 → 条目全被丢弃;冒烟桩是 LF,所以一直绿 |
| 5 | 「未入册(5)」是误报 | 文件名正则 `[a-z0-9_]+\.md` 不认连字符,5 篇 `oc-*.md` 明明在索引里;architecture 工具对它们的真实判定是「命名不合规」 |
| 6 | 右侧索引里 `live_design; last_verified_commit` 斜体错乱;记忆详情/聊天里 `old_string` 变成「old + 斜体 string」 | 行内 markdown `_([^_\n]+)_` 把词内下划线当强调 |
| 7 | 点 crate 节点提示「打开失败」 | 回退读 `crates/<crate>/Cargo.toml`,但 `docs_read_custom` 只放行 `docs/` |
| 8 | agent 已经在文档里写了 5 处 mermaid,界面上全是裸代码块 | 全站没有图渲染器 |

## 2. 选型

| 候选 | 结论 |
|---|---|
| **Mermaid 12.0.0**(MIT,2026-09-10) | **采用**。几乎所有模型都会写,仓里已有 agent 自然写出的 5 处;12 版把 ELK 分层布局内置为默认,边自动绕开节点、支持分组框;ESM 分块版按图种懒加载;GitHub 原生渲染同一份文本 |
| D2 | WASM 包解压约 60MB,MPL,模型写得少 |
| 自研 YAML + ELK | 最可控,但 agent 得学一套私有格式,聊天里也复用不了 |
| Excalidraw | 靠坐标的 JSON,agent 基本没法改 |
| Graphviz WASM | 能用,但好看程度和聊天复用都不如 Mermaid + ELK + token 主题 |

降级预案:12.0.0 出现阻塞回归时换 mermaid 11.17.2 + `@mermaid-js/layout-elk`,配置不变。

## 3. 真源与约定(给 agent 的规则,也写在 architecture 工具的 description 里)

- **手写图**:`docs/architecture/NN_snake_name.md`,两位数字前缀决定标签页顺序。第一行 `# 标题`,接一段说明,**第一个** ```mermaid 围栏是主图(文件里还有更多围栏也照常在查看器里渲染)。普通 write/edit 就能改(docs/ 不在 .kanzei/project 的 deny 范围)。
- 写 `flowchart LR`(宽屏首选)或 `flowchart TB`;**每个标签加双引号** `id["名字<br/>文件或职责"]`——第一行是名字(渲染器加粗),`<br/>` 之后是说明(渲染器降为次要色、小一号);不用 `end/graph/click/style/class` 当 id。
- **不写颜色**:`style`、`classDef`、`linkStyle`、`%%{init}`、frontmatter 里的 `config` 一律禁止。强调只用五个语义类 `:::entry`(入口)、`:::ext`(外部)、`:::store`(存储)、`:::focus`(本图主角)、`:::muted`(次要)。
- **节点点击**:`click <id> "<项目相对路径>[:行号]" "<悬停提示>"`,或目标写条目号 `click n "R-123"`;目标必须存在、不能越出项目根。
- 一张图控制在 40 个节点 / 60 条边以内,按子系统拆。实测:页面宽度 960~1300px 的画布上,LR 方向 6 层以内、每层 5 个节点以内的图能整张落在「适应」里且字号不低于下限。
- **crate 依赖图不落盘**:`crates/kanzei-tools/src/arch_diagram.rs` 每次从 Cargo 清单生成——`[workspace].members`(支持末段 `*`)、各成员 `[dependencies]` 与 `[target.*.dependencies]` 里属于工作区的依赖(处理 `package =` 改名与 `workspace = true` 继承;dev/build 依赖单列 kind、不画;小写 `cargo.toml` 也认)。分组来自 `[package.metadata.kanzei] group`,节点说明来自 `[package] description`(≤12 字才放得进 LR 卡片),入口文件取 src/lib.rs > src/main.rs > `[lib]`/`[[bin]]` path。**传递约简**默认隐藏能由其它路径推出的依赖(本仓 22 条 → 8 条),「全部依赖」把它们画成虚线。没被任何成员依赖的 crate 标 `:::entry`。
- 本仓 8 个 crate 的分组:入口 kanzei-app、kanzei;能力 kanzei-tools、kanzei-memory;运行时 kanzei-core、kanzei-harness、kanzei-llm;基础 kanzei-base。

## 4. 主题:只由 kanzei 注入

style.css 两个主题块:

```
:root  --diagram-canvas: var(--panel)  --diagram-cluster: var(--bg)  --diagram-cluster-border: var(--border-soft)
       --diagram-node: var(--panel2)   --diagram-node-border: var(--border-strong)  --diagram-text: var(--fg-strong)
       --diagram-muted: var(--dim)     --diagram-edge: var(--dim)  --diagram-accent: var(--accent)  --diagram-accent-soft: var(--accent-soft)
[data-theme="light"]  --diagram-cluster: var(--bg-deep)  --diagram-node: var(--panel)
```

两个主题同一视觉语义:分组是凹下去的底,节点是凸起的卡片。`04-diagram.js` 渲染时用 `getComputedStyle` 读这些 token(脚本里没有字面量颜色):

| mermaid themeVariables | token |
|---|---|
| background、edgeLabelBackground | canvas |
| primaryColor、mainBkg | node |
| primaryBorderColor、nodeBorder | node-border |
| primaryTextColor、textColor、nodeTextColor | text |
| lineColor | edge |
| clusterBkg、secondaryColor、tertiaryColor | cluster |
| clusterBorder | cluster-border |
| titleColor | muted |
| darkMode | `data-theme !== "light"` |

另外固定 `dropShadow: none`、`useGradient: false`(neo 外观默认的投影在暗色下是一圈发灰的光晕)。themeVariables 只认 hex,非 hex 的 token 不传;门禁查全部 --diagram-* 解析结果是 hex(将来有人改成 `color-mix()`/`oklch()` 会先红)。

五个语义类由渲染器按当前 token 生成 classDef,**追加在源码末尾**(错误行号不漂移),只对 flowchart 追加:entry = 文字色实描边 1.5px;ext = 画布底 + 次要色虚线描边与文字;store = 分组底;focus = accent-soft 底 + accent 描边;muted = opacity .72(.6 时次行对比度只有 2.84,门禁红过)。

CSS(style.css,app 层)只写 mermaid 内联 `<style>` 不写的东西——那份内联样式不分层,压过任何分层样式对同一元素的声明:节点与分组的圆角(`rx`)、两行卡片里名字行加粗(写在内层 tspan 上,它自带 `font-weight="normal"`)与说明行的次要色/字号/下移 3px、悬停淡化的 opacity、焦点环、光标。

主题切换:MutationObserver 监听 `html[data-theme]`,把挂着的图按新主题重渲(缓存键是「主题 + 源码」,切回来直接命中)。

## 5. 渲染器 `ui/04-diagram.js`

- **零 import**:04-markdown.js 在 Node 冒烟里被直接 import,这里不能牵出应用其它模块。翻译、导航由 04-structured.js 顶层经 `setDiagramHost({ t, openPath, openRef })` 注入(导航调用时才取 structuredNav,冒烟换 spy 照样命中);源码查看器、放大查看、toast 由 15-views-misc.js 注入。模块顶层不碰 document。
- **懒加载**:第一次渲染才 `import("./vendor/mermaid/mermaid.esm.min.mjs")`;架构页打开时空闲期预加载。flowchart + ELK 首次约 3MB,之后按图种取 chunk。
- **配置**:`securityLevel: "strict"`、`htmlLabels: false`、`theme: "base"`、`look: "neo"`、`layout: "elk"`、`flowchart { curve: basis, padding 12, nodeSpacing 32, rankSpacing 44, useMaxWidth: false, wrappingWidth 360 }`、`elk { BRANDES_KOEPF, considerModelOrder NODES_AND_EDGES, mergeEdges: true }`(合并同向边:crate 图「全部依赖」22 条边不合并时自然宽 2100、边绕成一团,合并后 1428×270)、`maxEdges 200`、`maxTextSize 50000`、`suppressErrorRendering`、`deterministicIds`。
- **源码预处理**:`click` 行与 `%%{init}` 指令抹成注释 `%% ·`(行数不变;单独的 `%%` 会被 mermaid 12 当成节点画出来)。click 行必须抹掉:strict 下 mermaid 不绑定回调,但仍会把带 href 的节点包成 `<a>`,点了会让 WebView 自己导航。
- **串行队列 + LRU 缓存**:`mermaid.render` 不可重入;结果按「主题 + 源码」缓存 24 份。同一份缓存 SVG 挂到多处时换一个 id 前缀(否则箭头 marker 的 `url(#…)` 会解析到另一份里)。
- **错误**:先 `parse` 拿带行号的错误(`hash.loc.first_line`,兜底从消息里取 `line N`),取 mermaid 报错的最后一行实质内容(Expecting … got …)。引擎加载失败不进缓存。
- **安全**:插入前查字符串(script / foreignObject / iframe / on* / javascript:),插入后再查一遍 DOM;命中即拒绝显示并出错误卡。
- **节点映射** `bindDiagram(svg, clicks)`:节点取 `g.node[data-id]`,没有就从 id 取 `…-flowchart-<id>-<n>`(mermaid 12 的形状);边从 `path[data-id^=L_]` 读 `L_<from>_<to>_<n>`,id 里的下划线用已知节点集合消解。可点节点加 `.is-link`、`tabindex=0`、`role=link`、`aria-label`、`title`(走 00-surface 全局 tooltip);点击/Enter → 条目号走 `structuredNav.openRef`,路径走 `structuredNav.openPath(path, line)`(docs/*.md 进查看器,其余进文件页)。悬停/聚焦节点:宿主加 `.is-focusing`,相邻节点与边加 `.is-related`,其余 opacity .28。
- **查看器** `mountDiagram(host, source, { mode, title, path, sourceLine, toolbarHost, onRendered })`:
  - 工具条:适应 / − / 缩放百分比 / + / 1:1 / 源码 / 复制;inline 模式多一个「在查看器里放大」,平时收起、悬停或键盘进入时出现。页面模式可把工具条放进宿主给的位置(架构页放在标签栏右侧,不压在图上);图没画出来(加载中/出错)时只留「源码 / 复制」。
  - Ctrl+滚轮或触控板捏合缩放(普通滚轮不劫持);拖背景平移(手势挂 document、不捕获指针,J4);双击背景回到适应;画布聚焦后 +/−/0/方向键。
  - **适应** = min(1.15, 宽比, 高比),下限按设备像素算:13px 标签缩放后至少 11 个设备像素——1 倍屏 0.85、1.25 倍 0.68、1.5 倍 0.6(下限封顶 0.85、保底 0.6)。低于下限时保持下限、显示「拖动查看全部 · Ctrl+滚轮缩放」。页面模式画布高 = clamp(360px, 图高 × 缩放 + 边距, 70vh),inline 最高 520。
  - 错误卡:「第 N 行:<mermaid 消息>」(N 按 `sourceLine` 换算成文件行号)、「查看源码」(查看器里带行号、出错行浅红底并滚到中间)、「复制修复提示」(「修复 <文件> 第 N 行 的 mermaid 语法:<消息>」+ 该行原文,可直接交给 agent)。markdown 里的坏图在错误卡下保留原文代码块,不吞内容。
- 冒烟接缝 `setDiagramEngine(engine)`(同 setRenderMarkdown):桩引擎提供 `parse / render / mount`。

## 6. Markdown 全站一个入口

- `04-markdown.js` 的 `renderMarkdownInto(el, raw, { streaming })` 是写进 DOM 的唯一入口:`el.innerHTML = renderMarkdown(raw)`,再 `hydrateDiagrams(el)`。全部 17 处 `x.innerHTML = renderMarkdown(…)` 已改;聊天流式正文与思考块传 `streaming: true`。ui-markdown-smoke 静态门禁:04-markdown.js 以外出现 `.innerHTML = renderMarkdown(` 即红,报错写明改法。
- 未闭合的围栏(EOF 时仍在代码块里,即流式输出写到一半)输出 `<pre class="code" data-open="true">`;`hydrateDiagrams` 只处理闭合围栏里的 `code.language-mermaid`,**半截图永不渲染**。缓存命中同步替换(流式每帧重设 innerHTML 也不闪);未命中先留代码块、异步渲染,完成时代码块若已被下一帧替掉,就对宿主再跑一遍(缓存已热)。
- 顺手修掉词内下划线:`(^|[^\p{L}\p{N}_])_([^_\n]+)_(?![\p{L}\p{N}_])`(Unicode 字母数字都算「词」,CJK 词内也不斜体)。

## 7. 架构页(index.html `#view-arch` + `19-arch.js`)

- 上方大图卡 `#arch-diagram-card`:标签栏 `role=tablist`(第一张固定「Crate 依赖」,其后是 docs/architecture 的图,有 lint 问题的标签带问题数;←/→/Home/End 切换),右侧是「直接依赖 / 全部依赖」切换(只换源码,不重取快照)与图工具条;画布是页面模式;脚注是来源路径 · 节点/边数 ·「已隐藏 N 条可由传递得到的依赖」·「点击节点打开实现或文档」· 只列本图用到的语义类的图例;有 lint 问题时图下逐条列「代码 · 第 N 行 · 消息 → 修法」。选中的标签与依赖范围记在界面布局偏好 `ui_layout.arch.{diagram, deps_full}`(本机 localStorage 重启即丢,D-404)。
- 下方两栏固定 480px 高、各自滚动:文档树(切行前 `\r\n?` 归一;文件名认 kebab,kebab 名在它的章节里标「命名不合规」,不再归「未入册」)| 架构索引原文。
- `renderArchGraph` / `openArchCrate` 已删;crate 节点点击打开入口文件。

## 8. 后端快照与 agent 自查

- `architecture_snapshot`(crates/kanzei-app/src/docs.rs)新增 `diagrams: [{ id, path, title, summary, source, source_line, issues: [{ line, severity, code, message, hint }] }]` 与 `crates: { members, edges: [{ from, to, kind, transitive }], mermaid: { reduced, full }, hidden_transitive }`(不是 Cargo 工作区时为 null)。旧的 `graph` 二元组字段兼容保留一个版本,之后删。
- `architecture` 工具新增只读动作 `diagrams`(dev 档默认放行):列出每张图与 lint 结果,附生成的 crate 图源码作为新图起点;有 error 判错(同 check),只有警告照常成功;docs/architecture 为空时给创建提示。
- lint(`arch_diagram::lint_mermaid`,每条带文件行号、代码与一句修法;只对确定写错的形态报 error,门禁必须可满足):

| 代码 | 级别 | 判据 |
|---|---|---|
| D1 | error | 缺 `# 标题` / 缺 mermaid 围栏 / 围栏未闭合 / 围栏为空 |
| D2 | error | 文件名不是 `NN_snake_name.md` |
| D3 | error | `%%{init}` 指令,或 frontmatter 里有 `config` |
| D4 | error | `style` / `classDef` / `linkStyle`,或未知语义类(`:::x`、`class a x`) |
| D5 | error | click 语法不对、节点不存在、目标不存在或越出项目根 |
| D6 | error | 节点/子图 id 是保留字;未加引号的标签含括号 `()[]{}`、竖线或双引号(`a[bad(]` 这类弱模型最常犯的错) |
| D7 | warn | 超过 40 节点或 60 条边,提示拆图 |
| D8 | warn | 裸 id 全图只出现一次且没有标签(多半是拼错了已有节点) |
| D9 | warn | 标签里写了字面量 `\n`,建议 `<br/>` |

  跨行的引号标签(markdown 字符串续行)整段跳过,收尾行引号之后的 `:::类` 与语句照常检查。与计划的差异:D7 定为警告(规模不是写错,不该挡住门禁);原计划把 `:;<>#` 也列进 D6,用 mermaid 12 实测这几种不加引号也能解析(括号、花括号、竖线、双引号与保留字 id 确实解析失败),只报确定失败的字符。
- 完整语法以真实渲染为准:应用里渲染失败出错误卡;本仓 verify 跑浏览器门禁。

## 9. 默认图

- `docs/architecture/01_runtime_loop.md`(LR,12 节点):入口(输入区、kz 命令行)→ kanzei-app 一轮任务(run_task、执行循环)→ kanzei-core 主循环(run_once_with_parts 为 focus,开跑装配 / 工具执行 / 上下文压缩)→ kanzei-llm(ext)、工具流水线;事件落 state.db(store)并推到界面。
- `docs/architecture/02_harness_registries.md`(LR,15 节点):组件来源(dev/research 档组件为 entry、只读档 muted、kanzei.toml 层叠、agent/skill 定义)→ HarnessDraft 五个注册表 → `Harness::resolve`(focus)→ HarnessSnapshot → 主循环 → 工具流水线;设计文档 harness_m1.md 为 ext。
- 两张图的每个点击目标由 `arch_diagram::tests::repo_default_diagrams_have_no_lint_errors` 与 ui-diagram-smoke 的路径检查持续核对。第一版 01 有 19 个节点、8 层,ELK 摆成对角线、在 960px 画布上缩到 0.55 仍裁切,重画成 6 层 12 节点后整张落进适应。

## 10. 校验(三层)

1. **Rust**(`cargo test -p kanzei-tools arch_diagram`、`architecture`、`profiles`;`-p kanzei-app architecture_snapshot`):清单解析(改名、target 依赖、dev/build 单列、缺描述/分组兜底、`[lib] path` 入口)、传递约简(含环)、本仓真实工作区约简后正好 8 条直接边且 8 个成员都有分组与描述、crate 图 golden(`crates/kanzei-tools/tests/fixtures/arch_diagram/*.mmd`,含引号/反引号转义与截断;`KZ_UPDATE_GOLDEN=1` 重写)、D1–D9 正反例与 CRLF 行号、`diagrams` 动作输出与描述里的约定、dev 档放行 `architecture/diagrams`、快照带手写图与 crate 图。
2. **浏览器门禁** `scripts/ui-diagram-smoke.mjs`(verify 的 **ui_diagram** 步,新检查键;ci.yml 与 git.rs 守护测试同步):无头 Edge 打开 gallery.html 直接 import 04-diagram.js,把 docs/architecture 的主图、docs/**/*.md 里全部 mermaid 围栏、两份 crate golden,在暗/亮两套主题下按页面模式宽 960 各渲染一遍,查:解析、click 映射率 100% 且目标存在、节点包围盒两两不重叠、标签不溢出形状、主标签有效字号 ≥ 11px(次行 ≥ 10px)、自然尺寸 ≤ 2400×1600、标签对节点底 ≥ 4.5(muted ≥ 3)、边对画布 ≥ 3、无 foreignObject/script/on*、token 是 hex;截图与 report.json 写入 dist/ui-diagrams/。`selfTestDiagramGate` 喂 10 个坏样例(语法错误、click 指向不存在的节点、目标文件不存在、foreignObject、重叠、溢出、字号、尺寸、非 hex token、低对比 token 覆盖——最后一个真改 `--diagram-text` 渲染),任何检查器恒绿即失败。verify-policy 的 `isDiagramPath`:docs/ 下的 .md、arch_diagram.rs、它的 golden、门禁脚本本身;前端改动一律带上。
3. **假 DOM 冒烟**(ui-runtime-smoke「分区:架构图」,桩引擎):CRLF 索引分组、kebab 名不算未入册、标签页与 role、送进引擎的源码抹掉 click 行并追加语义类、crate 节点点击 → `openPath(入口文件)`、带行号与条目号的 click、直接/全部依赖只换源码、错误卡的文件行号与两个按钮、修复提示内容、lint 问题列表、闭合围栏换图而 `data-open` 的不渲染、聊天流式传 `streaming: true`、没有绕开 renderMarkdownInto 的写法。变异守卫 `archCrlf` / `archKebab` / `diagramOpenFence` / `diagramClickMap` 实跑均红。ui-markdown-smoke 另有词内下划线、`data-open` 与静态入口门禁;verify-policy-smoke 补 ui_diagram 路径触发用例。

## 11. 风险与后续

- Mermaid 12.0.0 发布才半个多月(ELK 刚成为默认、neo 外观刚上线):版本锁死、配置显式写全、门禁截图留底;预案见 §2。
- 节点/边的 DOM 形状(`…-flowchart-<id>-<n>`、`L_a_b_n`)不是公开 API:两套选择器兜底,门禁要求点击映射率 100%,结构一变就红。
- 包体约多 5.4MB(ESM 分块版);第一次画 flowchart 加载约 3MB JS。
- tauri CSP 目前为 null;将来收紧 CSP 时 mermaid 生成的内联 `<style>` 需要 `style-src 'unsafe-inline'`。
- 许可:elkjs 为 EPL-2.0(按文件生效),分块原样分发并附声明(vendor/mermaid/THIRD_PARTY.md),不得修改分块。
- 聊天里模型生成的 mermaid 是半可信输入:按威胁模型不做对抗防御,保留 strict + htmlLabels false + 插入前后两道 SVG 检查;单图最多 200 条边、50000 字符。
- 待用户拍板:crate 图是否也在聊天里提供「插入当前 crate 图」的快捷方式;是否给研究页的探索路线图换成同一渲染器(现为自绘,本文不动)。

## 变更记录

- 2026-09-26:起草并实施(ui2/arch 分支,UI2-0926 #7)。相对最初方案的调整:crate 图改为 LR(8 个 crate 链深 6 层,TB 在横向画布上只能缩到 0.5)、crate 描述压到 ≤12 字;适应下限按设备像素算;muted 透明度 .6 → .72(门禁对比度);手写图 01 重画为 6 层 12 节点;ELK 由不合并边改为合并同向边(mergeEdges);D7 定为警告、D6 只报确定解析失败的字符;选中标签页存 `ui_layout` 而不是 localStorage(D-404)。

## 验证证据

- `cargo fmt --all -- --check`、`cargo clippy -p kanzei-tools -p kanzei-app --all-targets -- -D warnings`、`cargo test -p kanzei-tools`、`cargo test -p kanzei-app` 通过。
- 前端:`ui-a11y-smoke`、`ui-i18n-smoke`、`ui-markdown-smoke`、`ui-lint-smoke`、`ui-connectivity`、`parallel-lines-regression`、`ipc-event-smoke`、`check-design-freshness`、`ui-narrow-layout-smoke`、`ui-workspace-smoke`、`ui-runtime-smoke`、`ui-diagram-smoke`、`verify-policy-smoke` 全部退出码 0;上列变异守卫与 ui-diagram-smoke 的检查器变异(重叠/对比度/点击映射恒绿)实跑均红。
- 预览截图:`node scripts/ui-preview/shoot.mjs --scenes arch`(1333×695@1.5、1600×900@1.25,暗/亮;`tab=`、`full=1`、`broken=1`、`hover=` 场景参数)。
