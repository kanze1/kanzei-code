# 记忆知识图谱:按架构渲染的记忆关系图

- 身份: live_design
- 状态: 设计基线(2026-09-26 起草,随 ui2/memgraph 分支实施;本文描述的是已落地的实现)
- 日期: 2026-09-26
- 上游文档: [doc_reference_graph.md](doc_reference_graph.md)(节点模型与关系词表)、[ui_color_semantics.md](ui_color_semantics.md)(语义色表)、[ui_surface_stack.md](ui_surface_stack.md)(弹层唯一写法)、[memory_control_plane.md](memory_control_plane.md)
- 关联需求: R-368(本批落地其 B1 的最小子集:统一分词、弱引用抽取、节点键;并提供其 B5 要用的共享渲染器)、R-307(其 B3 的依赖 DAG 以后走同一个渲染器)
- 关联决策: A-015(图是只读投影,Markdown 唯一真源)、A-007(可替代区复刻);本文 §14 记录一条新决策「图可视化统一走 vendored force-graph 共享渲染器」,tracker 登记由集成方完成
- 一句话: 记忆页加「列表 | 图谱」切换,把记忆、需求/缺陷/决策/设计文档、代码区域(crate 与模块)和共享失败指纹画成 neo4j 式的力导向图;crate 按依赖深度钉在层带上,记忆被拉到它所说的代码区域周围,悬停看邻居、点开看全文、双击看 N 跳邻域,画布不可用时退到文本树。

## 1. 背景与问题

用户原话(问题清单第 9 条):「记忆可以按照架构渲染成neo4j那种图吗？有点像知识图谱那种」。

勘察结论(本仓 2026-09-26,项目库 + 全局库共 360 条记忆,活动 63 条、归档 297 条):

- **记忆之间、记忆与代码之间几乎没有结构化连接**。frontmatter 固定九个键;结构化 refs 只有 66 条,supersedes 6 条、superseded_by 56 条、subject 55 条,没有 tags/area。关联信息散在正文:R 提及 642 次、D 193 次、M 315 次、U 47 次,还有 `[fp:工具|错误头]` 失败指纹(204 条带指纹、102 个不同指纹、29 个被多条共享)。要画图只能靠抽取与推断。
- **桌面端拿不到图需要的数据**:`memory_entries` 只读活动条目、只带 refs,没有归档、subject、supersedes 与指纹。
- **没有「架构区域」这个概念**:现有架构图只有 crate 依赖边,前端是固定三列网格 SVG。
- **既有决策禁止图库、禁止全局图**:doc_reference_graph.md 非目标「不引入图库」、§8.4「手写 SVG」「全仓全局图不可读,不做」,R-368 边界同样写了「不引入图库」;architecture_browser.md 说「若用户明确需要图形拓扑,另立条目评估」。用户这次明确要了,所以要新决策(§14)。
- **两个实现陷阱**(原型实测):文件页的 Monaco loader 定义了全局 `define.amd`,普通 UMD 加载会走 AMD 分支、拿不到构造器;d3-force 遇到非有限坐标会死循环(原型里 `Math.max()` 对空数组返回 -Infinity,当锚点传进去页面永不就绪)。

## 2. 数据模型

后端 typed struct 经 serde 序列化(`crates/kanzei-tools/src/refgraph/memory_graph.rs`)。**所有节点同一形状**,可空字段显式为 null:IPC 契约只取数组首元素定形(`scripts/ipc-contract.json` 的 `memory_graph`),形状不一会漏字段。

| 字段 | 说明 |
|---|---|
| `id` | `M-009` / `U-015` / `R-368` / `D-749` / `A-015` / `doc:<文件名>` / `area:<crate>[/<模块>]` / `fp:<工具|错误头,截 80 字>` / `subject:<文本>` |
| `kind` | memory、requirement、defect、decision、doc、crate、module、fingerprint、subject |
| `label` / `title` / `description` | 画布标签、全称、召回钩子(非记忆为 null) |
| `scope` / `category` / `status` / `archived` / `updated` / `hits` | 记忆属性;hits 每次请求现取(见 §5) |
| `areas` / `primary_area` / `area_provenance` | 记忆所属区域(至多 4 个)、主区域、主区域依据 |
| `degree` | 全图度数(标签抢位优先级用) |

边:`source`、`target`、`rel`、`strength`(strong/weak)、`provenance`(about 边的依据)、`via`(经由的条目)、`anchor`(`<显示路径>:<行号>`)。

关系词表(`rel`):refs 关联、basis 依据、implements 实现、supersedes 取代、derived_from 来源——前五个与 doc_reference_graph §2 的前缀一一对应;另有 mentions 提及(弱)、cites 引用设计文档(弱)、about 关于(记忆→区域)、has_fingerprint 指纹、has_subject 同主题、contains 包含(crate→模块)、depends_on 依赖(crate→crate)。

返回体:`{version, generated_at, build_ms, cache:"hit"|"miss", areas:[{id, kind, label, parent, depth, band, memories}], nodes, edges, stats:{memories, live, archived, unassigned, by_provenance:{field, path, tool, via, keyword, none}}, warnings}`。`by_provenance` 用固定键而不是 map:契约按键定形,键集合不能随数据变。

## 3. 抽取规则

`collect_inputs(root)` 负责全部 IO,`build_graph(&GraphInputs)` 是纯函数。

1. **输入**:项目库与全局库的 `load_all()` + 新增的 `load_archived()`,同一 id 活动优先;tracker 三类文档(需求/缺陷/决策)的活动与归档,只取 id、标题、状态与字段正文;`docs/design/*.md` 文件名;区域注册表(§4)。
2. **强边**:frontmatter refs 经 `kanzei_harness::refs::split_refs` 切分(半/全角空格、逗号、顿号;剥括号注释;展开 `~`/`～` 区间,上限 200;识别 依据/实现/取代/来源 前缀;认不出的记为脏 token)。R/D/A 存在才建边,M/U 建记忆间边,`docs/design/x.md` 建文档节点,其它路径解析成区域后按 about(路径) 处理;不存在的写 warnings、不建边。supersedes 与 superseded_by 合成同一条「新→旧」边并去重。
3. **弱边**:标题、钩子、正文里的 `[RDAMU]-\d{2,4}` 提及——regex crate 没有 look-around,`\b` 又把中文当词字符,所以命中后手工检查前一个字符不是 ASCII 字母数字或下划线、后一个字符不是数字(「见R-123」命中,`XR-123`、`R-12345` 不命中);设计文档文件名提及记 cites;`[[标题]]` 按记忆标题解析,失败写 warnings。每条边记锚点行号(按文件原文算)。
4. **概念节点**:`[fp:…]` 标记与 fingerprint 字段形成指纹节点,subject 字段形成主题节点;只保留被 ≥ 2 条记忆共享的(本仓 39 个指纹、5 个主题,单例丢弃)。共享指纹就是重复记忆簇,详情里有「用对话合并」。
5. **区域(about 边)**,信号优先级:
   - field:frontmatter `area:`(唯一新增的真源字段);
   - path:正文里的 `crates/<c>/src/<m>…`、`crates/kanzei-app/ui/NN-x.js`、裸 `NN-x.js`、`kanzei_x::y`、`scripts/…`(`.kanzei/…` 这类隐藏目录不算——剥掉前导点会被当成 `kanzei` crate 的模块,真实数据里曾误挂 11 条);
   - tool:查 `TOOL_AREAS` 表——指纹里的工具名、标题首词后接空格/冒号/`]`、反引号里的工具名;
   - via:只看结构化 refs 指向的 R/D/A 条目正文路径,且该条目命中 ≤ 3 个区域时才采用(正文提及的条目太宽,实测噪声大);
   - keyword:`KEYWORD_AREAS` 小表(前端/i18n/冒烟→ui,SSE/provider→llm,权限→permission,tracker/需求→tracker,发版/verify.ps1→scripts…),ASCII 词按词边界匹配,总是作为弱候选加入。
   - 每个区域取最高档依据;`scripts` 降权(只有别无区域时才当主区域);每条最多 4 个;field/path 为强边,其余弱边(画虚线)。
6. **输出范围**:节点输出全部 crate 级区域与至少被一条记忆 about 的模块;`areas` 数组给全量区域(「设为区域」下拉用)。

本仓实测:537 节点 / 1595 边(记忆 360、需求 63、缺陷 30、文档 3、crate 11、用到的模块 26、指纹 39、主题 5);主区域依据 工具 206 / 关键词 83 / 路径 34 / 经由 11 / 未归类 26;warnings 1 条(一个解析不到的 `[[标题]]`)。

## 4. 区域注册表

`crates/kanzei-harness/src/areas.rs`,纯文件系统扫描、只用字符串操作。放在 harness 是因为 memory(写入校验)与 tools(图谱)都要用,harness 是两者共同的最低层。

- Cargo workspace:members(含 `crates/*` 通配)对应 crate 区域;`src/*.rs` 与 `src/*/` 对应模块,跳过 lib/main/mod/bin/*_tests;crate 内非 src 子目录含 ≥ 3 个前端代码文件的成为 crate 级子区域(`kanzei-app/ui`、`kanzei-app/mobile-pwa`),其中脚本去掉数字前缀成为模块(`13-memory.js` → `kanzei-app/ui/memory`);项目根下含 ≥ 3 个代码文件的顶层目录成为 crate 级区域(`scripts`),没有模块层。
- 非 Cargo 项目:顶层目录(自身或下一层有代码)为 crate 级区域,其子目录为模块。
- `depth` = 沿内部依赖的最长路径(带环保护);子区域 = 父 crate + 1;顶层代码目录 = 最大深度。`band = round(depth·3/maxDepth)`,四条带:本仓 base 在基础层,core/harness/llm 在引擎层,memory/tools 在工具层,app/cli/ui/scripts 在入口层。
- `resolve_token` 接受 `area:` 前缀、区域 id、包名形态(`kanzei_tools/tracker`)、仓内路径(含行号锚点、反斜杠)、Rust 路径、裸前端脚本名(仅当恰好一个前端区域有它);写错的模块(`kanzei-tools/trackr`)返回 None,**不退化成 crate**——否则写入校验会把笔误悄悄收下。`nearest` 给解析失败时的候选。
- `crate_deps()` 取代 `docs.rs` 里 `build_workspace_graph` 的重复解析(返回形状不变,R-188 的单测照旧)。`watch_dirs()` 给缓存指纹用。

## 5. 缓存

- 后端(`crates/kanzei-app/src/memory.rs`):`static GRAPH_CACHE: OnceLock<Mutex<HashMap<项目根, (指纹, Arc<Value>)>>>`。指纹 = 记忆两个目录及其 archive/ 的 **.md** 文件 (路径, 长度, mtime)、tracker 三类文档与归档的 stat、docs/design 的 .md、注册表相关目录的**条目名**(Cargo.toml 另算 stat)。
  - 只看 .md:index.db、锁文件、WAL 是派生物,读图谱本身(查命中数)就可能创建或改动它们,算进来缓存永不命中(单测实测过)。
  - 目录只看条目名:改代码不该让图谱缓存失效,区域只取决于有哪些文件。
  - 命中数不进指纹,每次现取覆盖(它随每次召回变)。
- 前端:15 秒缓存;`kz:memory-changed` 强制重取。

## 6. area 字段写入

- `memory_add` / `memory_update` 新增可选 `area: string[]`,工具描述加一句「optional area: code areas this memory is about, e.g. kanzei-tools/tracker or crates/kanzei-app/ui/13-memory.js; omit if unsure」。每项经 `resolve_token` 归一,任何一项解析不到就整体拒绝,报错列出该 token 与至多 5 个最接近的区域 id。update 的 area 是整体替换,`[]` 清除,不传不动。
- `MemoryStore::set_area(id, &[String])`:持记忆树锁,写或删 extras 的 `area` 键(空格分隔、去重),写后 refresh_derived;归档条目只读。
- 桌面端 `memory_entry_save` 新增 `area` 参数(同样归一与报错);新命令 `memory_entry_get(project_dir, scope, id)` 读活动或归档条目,返回与 `memory_entries` 同形的数据加 `archived`;`memory_entries` 补 `areas` 与 `archived`。
- 工具 schema 只多一个可选字段,不新增工具(D-662 工具面预算)。

## 7. 渲染器与选型

**决策**:图可视化统一走 vendored force-graph 1.51.4(MIT,canvas 2D,177,599 B,首次打开才加载),封装成共享渲染器 `ui/24-graph-view.js`;记忆图谱现在用,R-368 B5 的引用邻域图与 R-307 B3 的依赖 DAG 以后用同一个渲染器(`layout: "dag-lr"` 已预留),不再各写一套手写 SVG。图仍是 Markdown 的只读投影(A-015 不变)。

| 候选 | 体积(jsdelivr,2026-09-26) | 结论 |
|---|---|---|
| **force-graph 1.51.4**(MIT) | 177,599 B 单文件 UMD,已捆绑 d3-force/zoom/drag 等 | **采用**:持续力导向模拟、拖拽节点周围跟着动,最接近 neo4j 的手感;一条记忆同时连多个区域是自然的多重 about 边;dagMode 可给以后的依赖图用;画布 API 允许完全自绘(外壳、标签抢位、线型) |
| cytoscape 3.34.3(MIT) | esm.min 434,102 B;fcose 另要 cose-base 119 KB + layout-base 148 KB(同样撞 AMD) | 复合节点做分区很合适,但布局是一次性的、没有持续物理;一个节点只能有一个父节点,一条记忆常涉及多个区域 |
| vis-network 10.1.2(Apache-2.0 或 MIT) | standalone esm min 651,775 B | neovis.js 基于它、外观最像 neo4j,但体积最大,且自带 DOM 悬浮提示,违反弹层唯一写法 |
| sigma 3.0.3 + graphology(MIT) | 187,876 + 73,629 B | WebGL 适合上万节点;ForceAtlas2 要自己打包,边标签弱;300~1000 节点上没有优势 |
| d3-force 3.0.0 单用(ISC) | 8.3 KB + quadtree/dispatch/timer | 缩放、拖拽、命中检测都要自己写,force-graph 已封装好 |

实现要点:

- **加载**:`loadForceGraph()` fetch 文本后 `new Function("module", "exports", "define", source)(module, module.exports, undefined)`,走 CommonJS 分支,避开 Monaco 的 `define.amd`;Promise 只加载一次,失败后下次重试。Tauri CSP 现为 null;以后收紧 CSP 需要 `unsafe-eval`,或改成离线 esbuild 打同版本 ESM 放进 vendor(README 写明)。
- **颜色**只从 `--graph-*` token 读(`getComputedStyle`),`kz:theme` / `kz:language` 事件触发重读重画;脚本里没有字面量颜色(C1/J2 门禁照常覆盖)。
- **关闭库自带浮动提示**(nodeLabel/linkLabel 返回空串):悬停信息写页面状态栏,不另起浮层(ui_surface_stack.md 的弹层唯一写法)。
- **自绘**:节点(圆、空心环、半透明虚线环、菱形、R/D/A/§ 字形)与边(强实线/弱虚线、关系边 3px 箭头、contains 细线);标签在帧末统一画,屏幕空间网格贪心抢位(选中 > 悬停 > 悬停邻居 > 检索命中 > crate > 模块 > 记忆按度数);边标签只给度数 ≤ 8 的悬停/选中节点画(模块这类枢纽一悬停十几条「关于」,叠成一团反而读不出);节点有屏幕像素下限半径(缩小看全局时不缩成看不见的点)。
- **生命周期**:视图隐藏或切回列表时 `pauseAnimation`,回来 `resume`;`destroy()` 只依赖 pause、清空 host、释放引用(库的 `_destructor` 没有文档)。

## 8. 纯函数模型

`ui/24-memory-graph-model.js` 零 import,node 直接测:

- `visibleGraph(payload, filters)`:记忆按 范围/分类/状态/区域 筛;**归档条目只看「含归档」开关**(归档里几乎都是 deprecated,跟着状态筛选走的话默认「active」下永远看不到);**crate 级区域只在它或它的模块、子区域有可见记忆时出现**;模块只在有可见记忆关于它时出现(且「关于」图层开着);需求/缺陷/决策/文档只在与可见记忆相连且那条边的图层开着时出现;概念节点要 ≥ 2 条可见记忆共享。默认图层 关于/关联/取代/指纹,提及关;contains 是骨架总在,depends_on 由层带位置表达、不画线。
- `egoSubgraph(payload, center, hops, layers)`:双向走 N 跳,不走 contains/depends_on;中心是 crate 时允许沿 contains 走一跳。
- `bandAnchors(areas)`:入口层在上、基础层在下;同带按 id 排序后居中;没有 crate 的层带不占行;空输入时未归类锚点回退为 0(变异守卫盯住这一行)。
- 其余:`nodeRadius`、`nodeStyle`、`convexHull`、`placeLabels`、`searchNodes`、`groupForTextView`、`neighborsOf`,以及关系/依据/种类/图层的文案 key 表(值都在 I18N_EN)。

## 9. 记忆页交互

- 工具栏末尾「列表 | 图谱」分段开关(`aria-pressed`);选择存 `~/.kanzei/app.json` 的 `memory_view`(prefs.rs 独立字段,D-404:本机 WebView2 localStorage 不落盘),localStorage 只作旧值兼容。
- 图谱模式:工作区两列(画布 | 右侧详情栏,详情栏 sticky 且可独立滚动),列表与排序隐藏;窄屏容器(≤ 760px)画布全宽,有选中时才显示详情栏。
- 工具栏:区域(按 crate 分组,只列有记忆的区域)、图层(关于/关联/取代/指纹/提及)、含归档、邻域条(「邻域 M-009 · 1 2 3 · 退出邻域」,只在邻域模式显示)、适配视图、文本视图。
- 联动:现有 范围/分类/状态 筛选变化派发 `kz:memory-filters`,图谱按同一组筛选重算(不重排,位置按 项目+id 保留);检索命中派发 `kz:memory-search-hits`,图上画强调色命中环、其余淡出、居中第一条,清空检索取消;列表选中(`kz:memory-selected`)同步选中环;切项目(`kz:memory-project`)清空状态并用代次守卫,项目 A 在途的图谱不会画进项目 B。
- 悬停:邻居高亮、其余淡出,状态栏写「id · 标题 · 分类/状态 · 关于 区域(依据)」。
- 单击记忆:右栏显示全文;归档条目先 `memory_entry_get`,以只读方式打开(不给保存/失效/删除/改标题/编辑正文,显示「已归档(只读)」)。
- 单击其它节点:右栏列关系(按关系类型分组,点条目跳到那个节点)与动作——需求/缺陷「打开条目」(jumpToEntry)、决策「打开决策文档」(docs_read 新增 decision kind)、设计文档「打开设计文档」(openArchDoc)、crate/模块「只看该区域」、指纹/主题「用对话合并」(把「合并这些重复记忆:M-a M-b(共同指纹 …)」预填进管理对话并切到对话页签,**不发送**);所有节点都有「看邻域」。
- 双击进入邻域(默认 2 跳),画布或文本树上按 Esc 退出(元素级监听;全局 Esc 归 00-surface.js 的弹层栈)。单击背景取消选中。
- 详情「区域」行:列出区域与依据徽标(字段实线框、推断虚线框,「经由 D-xxx」写出条目号),旁边「选择区域…」下拉(全量区域按 crate 分组)与「设为区域」「清除区域」,写 area 字段后触发刷新。列表模式下图谱载荷还没取过时先显示字段,取到后补上推断。

## 10. 视觉

- **类别色**(只在图谱画布与图例里区分记忆分类,不表达状态):避开橙(进行中)、琥珀(需要注意)、绿(成功)、红(失败)四种状态色相,取 天蓝 fact / 紫 sop / 靛 habit / 梅 preference,靠明度拉开。

  | token | 暗色 | 亮色 |
  |---|---|---|
  | --graph-fact | #61c0f2 | #2a7da6 |
  | --graph-sop | #b779f2 | #ae6fe8 |
  | --graph-habit | #4b65d9 | #2b3cad |
  | --graph-preference | #9e5484 | #7c2462 |
  | --graph-module | #7a7a7a | #8a8a8a |

  dataviz 校验(OKLab ΔE×100,全部两两对,节点图任何两个点都可能相邻):暗色 色觉异常最差 9.1、常视最差 17.8,亮色 10.4 / 17.9,都过 8 / 15 的线;对主区与卡片对比度全 ≥ 3。明度带检查不过是有意的:暗色带 L 0.48–0.67 里放不下四个非状态色相(实测最多三个且常视只有 13),只能靠明度拉开。原方案写的「fact 蓝、sop 绿、habit 紫、preference 金」是配色改版前定的,绿与琥珀现在是状态色,故改用上表。
- 其余 `--graph-*` 是别名(只在 :root 定义一次):上下文节点 --dim、crate --fg-strong、概念 --fg(空心菱形,中性)、强边 = 模块灰、弱边 --border-strong(装饰性虚线,豁免对比度)、标签 --fg / --dim、选中与悬停环 --fg-strong(中性)、外壳 --fg(5% 透明度)、背景 --bg;**检索命中环 --accent-text**(语义表「一次性的看这里」)。门禁见 §13。
- 节点:记忆半径 6 + min(6, 1.5·log2(1+hits)),crate 15,模块 8,其它 5.5;active 实心、候选/影子空心环、deprecated/invalid/归档 40% 透明加虚线环;需求/缺陷/决策/文档是灰色小圆内写 R/D/A/§。
- 边:强实线 0.8px、关系边带箭头;about 依据为字段/路径画实线、其余虚线;提及/引用虚线。
- 标签:crate 总显示;模块缩放 ≥ 0.55;记忆 ≥ 0.85 显示编号、≥ 2.4 附标题前 14 字;其它 ≥ 1.8;悬停/选中/命中及其邻居总显示。
- 区域外壳:同一 crate 的节点画一圈圆角凸包(线宽 36 世界单位、5% 透明度),放大到 2 倍以上或邻域模式下隐藏。

## 11. 布局

- crate 用 fx/fy 钉在层带锚点(列宽 360、带高 240,入口层在上);空 crate 不出现,所以层带只排有内容的 crate,缩放后能看清。未归类记忆的锚点在右上角。
- 自定义聚类力:模块被拉向所属 crate 锚点(0.06)、记忆拉向主区域的 crate(0.035),其余节点 0.02 弱向心;邻域模式换成径向力(每跳 130 单位),中心钉在原点。
- 斥力 crate −300、模块 −140、记忆 −28、其它 −24;连线距离 contains 70、about→模块 34、about→crate 48(弱 60)、其余 55;强度 提及/引用 0.08、contains 0.7、弱 about 0.3、其余 0.45。模块像花瓣围着 crate,记忆再围着模块。
- warmupTicks = min(120, 80000/节点数),alphaDecay 0.04,**d3AlphaMin 0.002**(force-graph 默认 0,只靠 cooldownTicks 200 收工,再小的图也要跑满 200 帧);力的参数在 graphData 之前设好(换数据会立刻用当前的力跑 warmup,之后再改再 reheat 等于白跑);适配视图只看记忆、模块与 crate,只在换了一批数据、进出邻域、换区域后自动适配(开关图层与筛选保持用户的缩放)。
- 库的坑:`dagMode` 每次设置都会把**当前** graphData 全部节点的 fx/fy 清掉,而节点对象在两次 setData 之间是复用的(位置保持),所以只在布局真的变了时才设 dagMode、且先设再钉 crate;否则第二次重排后整张架构骨架会散掉(浏览器冒烟用「同带 crate 的 y 完全相同」盯住)。

## 12. 降级与无障碍

- `loadForceGraph` 失败或环境没有 canvas(假 DOM)就显示文本视图,状态栏写「图形渲染不可用,已显示文本视图」。
- 文本视图:按 crate → 模块分组的树(`role=tree`/`treeitem`/`group`,方向键、Home/End 移动,回车打开),「文本视图」开关随时可切。
- 画布 `role=img`,读屏名称带节点数与关系数,`aria-describedby` 指向状态栏(`role=status`);画布本身不可逐节点聚焦,这一点由文本视图补齐。

## 13. 门禁与测试

| 位置 | 内容 |
|---|---|
| kanzei-harness | areas:工作区扫描、依赖深度与层带(含成环)、非 Cargo 回退、resolve_token 各形态与拒绝、本仓层带;refs:split_refs 契约 |
| kanzei-tools refgraph | 提及 ASCII 边界、[[标题]];区域优先级 field>path>tool>via>keyword 与 4 个上限;scripts 降权、via 只来自结构化 refs、隐藏目录不算区域;supersedes 双向去重;概念保留/丢弃;活动优先与归档标记;悬空进 warnings;未用模块不出节点;节点同形;collect_inputs 与指纹稳定/失效;注册工具全有区域或豁免;词表区域在本仓都解析得到 |
| kanzei-memory | load_archived、set_area(去重/清除/拒绝);memory_add 的 area 归一落盘、未知区域整体拒绝并给候选、memory_update 替换与清除 |
| kanzei-app | memory_graph 缓存命中/改文件后重建;memory_entry_get 读归档;memory_entry_save 的 area 归一与报错;memory_graph 形状契约;memory_view 偏好;build_workspace_graph 原单测 |
| ui-memory-graph-smoke.mjs | 纯模型 10 组;vendor SHA-256(换行按 LF 归一)/长度/README/LICENSE/加载方式;变异自检 anchor_finite、ego_contains、default_mentions(`KZ_GRAPH_MUTATE=<id>` 可单独复核,期望非零退出);`--browser`:vendor 请求 200、零 pageerror/console.error、布局 < 3 秒、画布非空白、程序化悬停写状态栏、切主题像素变化、重排后 crate 仍钉在层带上、文本视图条目数 = 可见记忆数、677 节点 / 1866 边稳定 < 3 秒 |
| ui-a11y-smoke 分区:记忆图谱 | 节点与强边对 --bg/--panel ≥ 3、标签 ≥ 4.5、选中/命中环 ≥ 3(两套主题);类别色只准出现在图谱选择器、必须是自己的 hex;--graph-* 别名不得指向状态色(--graph-hit 例外);画布/状态栏/树/开关的标记;4 个反例自测 |
| ui-runtime-smoke 分区:记忆图谱 | 点「图谱」调 memory_graph、假 DOM 降级文本视图、条目数、点条目开详情、切项目竞态、切回列表;夹具按契约校验;变异 memgraphToggle、memgraphProjectGuard |
| verify / CI | `ui-memory-graph-smoke --browser` 挂在 verify.ps1 的 ui_a11y 步(检查键集合不变,git.rs 的对齐守卫不用改),ci.yml 同步 |

预览:`node scripts/ui-preview/shoot.mjs --scenes memory,memory-graph --themes dark,light --width 1600 --height 960 --scale 1.25`(另有 1280×690@1.5);memory-graph 场景参数 `hover`/`select`/`ego`/`text=1`/`archived=1`(`--query` 传入)。夹具 `scripts/ui-preview/memory-graph-fixture.mjs` 是本仓真实记忆的子集(29 条项目记忆 + 相连的归档/条目/指纹 + 全部 crate,全局记忆换成 3 条合成条目),用 `KZ_MEMORY_GRAPH_DUMP=<out.json> cargo test -p kanzei-tools refgraph::tests::dump_repo_graph` 取样后切出。

## 14. 决策记录

- **图可视化统一走 vendored force-graph,共享渲染器 `ui/24-graph-view.js`**(本文 §7)。它取代 doc_reference_graph.md 非目标里的「不引入图库」和 §8.4 的「手写 SVG 分层布局」:R-368 B5 的邻域图与 R-307 B3 的依赖图走这个渲染器的 `dag-lr` 布局。「全仓引用全局图不可读,不做」对文档引用图仍然成立;记忆图谱是另一张图,靠 范围/状态/区域 筛选、默认不画提及、空 crate 不占层带、邻域模式来控制可读性。依据:用户明确要了图形拓扑(architecture_browser.md 预留的触发条件),原型实测 693 节点 / 1820 边可用。
- tracker 登记(新决策 A 条目、实施 R 条目、R-368 边界改写)由集成方按本节完成,本分支不写 tracker 文件。

## 15. 与既有条目的边界

| 条目 | 关系 |
|---|---|
| R-368 | 本批落地 B1 的最小子集(`kanzei_harness::refs` 分词、`refgraph::mentions` 弱引用抽取、节点键),并提供 B5 用的渲染器;B2-B5 的其余部分(tracker 写入校验、git 历史、体检、侧栏)不在本批 |
| R-307 B3 | 依赖 DAG 以后用 `createGraphView(…, { layout: "dag-lr" })`,不再手写 SVG |
| 架构页 19-arch.js | 仍是旧的三列网格 SVG;迁到共享渲染器另立条目(本批只把 `build_workspace_graph` 的解析收进 AreaRegistry) |
| D-662 | 不新增工具,只给 memory_add/memory_update 加一个可选字段 |

## 16. 与已批准方案的出入

- 类别色改为天蓝/紫/靛/梅(§10):方案写于配色改版之前,`--line-1..4` 已改为中性别名,绿与琥珀成了状态色。
- 概念节点用中性空心菱形(方案是琥珀):共享指纹是「重复记忆簇」,不是需要你立即处理的状态;要合并时走「用对话合并」。
- 空 crate 默认不画、层带压紧(方案是 crate 总在):真实数据里空 crate 占掉整条层带,记忆挤在中间看不清(原型截图同病)。
- 「含归档」与状态筛选独立(方案里归档跟着状态筛选走,默认「active」下永远看不到归档)。
- 视图选择存 app.json(方案是 localStorage):D-404,本机 WebView2 localStorage 不落盘。
- 新冒烟挂在 verify 的 ui_a11y 步而不是新增检查键:新增键要同时改 git.rs 对齐守卫、verify-policy 与发版证据,与同期其它分支冲突面大。
- 方案里的 tracker 动作(登记新决策与 R 条目、改 R-368 边界、核实关闭 D-749)留给集成方:本分支规则禁止写 tracker 文件。

## 17. 验证证据

- 本仓真实数据(调试构建):收集 94–220 ms、构建 150–170 ms;537 节点 / 1595 边;默认视图(项目库、active)在夹具上 65 节点 / 89 边、25 条记忆,布局约 0.2 秒稳定。
- 规模:无头 Edge 153 上 677 节点 / 1866 边约 0.94 秒稳定(39 帧,中位帧 11.6 ms;`ui-memory-graph-smoke --browser` 的上限是 3 秒)。设 d3AlphaMin 之前要跑满 200 帧、约 2.6–3.1 秒。
- 截图:1600×960@1.25 与 1280×690@1.5,暗/亮两套主题;图谱默认视图、悬停模块、选中记忆、邻域、文本视图、指纹节点详情、列表模式的区域行都看过(截图在会话 scratchpad,不入库)。

## 18. TODO 与风险

- 区域推断会误判(例如带 `[fp:bash|…]` 的 SSE 记忆被归到 bash)。缓解:推断关系画虚线并写依据,详情里一键「设为区域」写成字段,字段优先级最高。
- 全量视图(含归档、开提及)会挤成一团;默认只看活动记忆、提及关、空 crate 不画,并提供区域筛选与邻域模式。
- CommonJS 垫片依赖 `new Function`;CSP 收紧时见 §7 的备选方案。升级 force-graph 要重新 vendor 并更新冒烟里的 SHA-256。
- IPC 载荷约 200–300 KB(700 节点);项目记忆超过 3000 条时应改为服务端按筛选裁剪。
- 全局记忆里的路径属于别的项目,在当前项目解析不到,只能靠工具词表与关键词,未归类比例会偏高(本仓 26/360)。
- `vendor/force-graph/*.min.js` 在 autocrlf 检出下会变成 CRLF,SHA 校验按 LF 归一后比较;浏览器执行不受影响。
