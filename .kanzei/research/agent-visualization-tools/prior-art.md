---
kind: prior_art
topic: agent-visualization-tools
status: complete
trigger: explicit_user
entry_refs: R-335
websearch_round_limit: 4
---

# 先行方案对照

> 完成前请补齐双侧至少各一条对照，并将 `status` 改为 `complete`。

## 外部已有实现

### Graphviz DOT
- 出处: https://graphviz.org/doc/info/lang.html
- 出处: https://graphviz.org/docs/layouts/
- 出处: https://graphviz.org/docs/outputs/
- 证据域:文献/官方文档
- 证据等级: V2
- 证据锚:官方 DOT 语法正文定义 graph/node/edge/subgraph/cluster，布局页列出 dot(分层)、neato(弹簧)、fdp/sfdp、circo、twopi 等，输出页列出 JSON/PDF/PNG/SVG 等格式及 `-Tlang`
- 证据深度:官方文档正文级
- 输入模型:文本 DOT 图，原生表达节点、方向边、属性、子图和 cluster；适合把 crate/模块/边及分组语义显式写入版本库，输入比当前二元边更丰富。
- 自动布局/编码:多布局引擎按图类型选择；dot 提供有向图分层，cluster 可形成边界分组，节点/边属性控制形状、字体、标签和约束。布局是外部引擎决定，仍需对大图做结果检查。
- 渲染/导出/可读性反馈:命令行 `-T` 可导出大量位图、矢量和中间格式；标签、字体、cluster、tooltip/URL 等属性可改善阅读与回源。官方资料提供能力/格式清单，但没有替 Agent 生成统一的可读性评分或语义正确性报告。
- 版本化/权限/运行约束:DOT 文本易于 Git 版本化，Graphviz 可作为本机 CLI；格式插件取决于构建，实际可用格式需运行 `dot -T?` 检查。引入意味着新增本机依赖与进程权限，不能假设桌面端永远存在。
- 差异:比当前固定 SVG 有成熟布局、分组和多格式输出；比 Structurizr/C4 更底层，模型约束较弱；比 Mermaid 更适合离线 CLI 和复杂边布局。
- 决策:列为架构图首选候选；若用户允许新增受控本机依赖，优先把统一架构 spec 编译为 DOT，并将 SVG/PNG/PDF 与布局诊断纳入产物；不直接把 Graphviz 二进制 vendored 进当前 classic-script UI。

### Mermaid（含 C4/Architecture）
- 出处: https://mermaid.js.org/config/usage.html
- 出处: https://mermaid.js.org/syntax/c4.html
- 证据域:文献/官方文档
- 证据等级: V2
- 证据锚:官方 usage 正文说明 Markdown 文本定义可重复渲染、API `render` 返回 SVG、CLI/依赖安装和 `securityLevel`；C4 正文列出 Context/Container/Component/Dynamic/Deployment 五类并明确该实现是 experimental、布局并非完全自动化、按语句顺序影响位置
- 证据深度:官方文档正文级
- 输入模型:Markdown-like 文本；C4 提供系统/容器/组件/动态/部署语义，Architecture 页面提供图类型生态。文本短、适合 Agent 生成和代码审查，但自由字符串仍需类型/关系校验。
- 自动布局/编码:标准图由 Mermaid 渲染；C4 官方页明确不采用完全自动布局，形状位置会受语句顺序和 `UpdateLayoutConfig` 影响；11.17.1 起 C4 默认文字换行。不能把“声明式输入”误称为稳定的全自动布局。
- 渲染/导出/可读性反馈:API 可返回 SVG，官方 CLI/依赖可用于离线渲染；`securityLevel` 默认 strict 会禁用 click，loose/antiscript 才开放交互；官方页专门警告动态字体未加载会造成标签越界。可读性有换行/字体注意项，但没有统一布局评分或 PNG/PDF 产物清单。
- 版本化/权限/运行约束:语法文本易 Git 版本化，官方示例通过 npm/CDN/ESM 引入；版本可从文档下拉/依赖指定。CDN、ESM、浏览器安全级别和交互绑定会扩大当前离线桌面约束，不能直接复用现有 A-008 classic-script 边界。
- 差异:比当前自绘 SVG 有更多图类型和回源交互；C4 的自动布局限制与当前问题相似，不能单独解决拥挤；比 Graphviz 更易生成但对安全级别、字体加载和版本更敏感。
- 决策:保留为轻量文本/交互预览候选，不作为架构图唯一布局后端；只有在 vendored 版本、securityLevel、字体和 SVG/PNG 导出可验证时才接入，当前不改变运行时依赖。

### PlantUML
- 出处: https://plantuml.com/command-line
- 出处: https://github.com/plantuml/plantuml
- 证据域:文献/官方文档与官方源码仓库
- 证据等级: V2
- 证据锚:官方命令行文档定义 `java -jar plantuml.jar`、`--check-syntax`、`--version`、`--format`、`--output-dir`、PNG/SVG/PDF/LaTeX/TXT 输出及非零错误码；官方仓库 README 列出 UML、deployment、Archimate、JSON/YAML 等文本图类型及 TeaVM/Graphviz 支持
- 证据深度:官方文档/仓库正文级
- 输入模型:纯文本描述，覆盖 component/deployment/Archimate 等架构相关图；语法紧凑、适合 Agent 和 Git，但模型约束依赖具体图类型/宏，需在统一 envelope 中声明 diagram_kind。
- 自动布局/编码:内置渲染并可使用 Graphviz Dot layout；支持主题、skinparam、超链接/tooltip。布局可表达架构图，但不同图类型和 Graphviz 依赖会带来结果差异，不能只按文本成功判断质量。
- 渲染/导出/可读性反馈:官方 CLI 支持 PNG 默认及 SVG/PDF/LaTeX/TXT 等格式，`--check-syntax` 可先做语法门禁，`--check-graphviz` 可检查外部布局依赖；可读性反馈主要靠预览/主题/字体与错误图，未提供统一语义或密度评分。
- 版本化/权限/运行约束:源文本可 Git 版本化，CLI 支持 `--version`、输入/输出目录和 metadata 开关；主要运行约束是 Java，部分图依赖 Graphviz，另有 server/Docker/browser 运行方式。Java/Graphviz 进程与输入文件权限必须显式隔离。
- 差异:比 Graphviz 提供更高层 UML/Archimate 语义与语法检查；比 Mermaid 更成熟的 CLI 多格式输出，但 Java 依赖更重；比 Structurizr 更自由、模型治理更弱。
- 决策:列为架构图高级轨道候选，适合 deployment/Archimate/交互序列等需要成熟语法的视图；默认轨道不直接执行任意 PlantUML，必须限制输入目录、检查语法、记录 Java/PlantUML/Graphviz 版本并生成 manifest。

### Structurizr DSL
- 出处: https://docs.structurizr.com/dsl
- 出处: https://docs.structurizr.com/cli/export
- 证据域:文献/官方文档
- 证据等级: V2
- 证据锚:官方 DSL 正文定义基于 C4 的 text DSL、workspace/model/views、implied relationships、styles/themes、ADRs 和 inspections；CLI export 正文列出 JSON/PlantUML/C4-PlantUML/Mermaid/DOT/static 等格式、workspace DSL/JSON 输入、输出目录及主题需要 Internet 的限制
- 证据深度:官方文档正文级
- 输入模型:以 workspace 为根的 C4 模型，实体与关系先建模，再定义 system context/container/component/deployment/dynamic views；比当前架构 snapshot 的裸边更能表达层级、视图和决策记录。
- 自动布局/编码:通过 C4 视图、styles/themes、implied relationships 和多种导出器组织表达；布局/最终画布依赖本地/云端 viewer 或导出后端，不应把 DSL 本身当作布局算法。
- 渲染/导出/可读性反馈:CLI 可导出 DOT/PlantUML/Mermaid/JSON/static，官方明确 CLI 不直接导出 PNG/SVG（浏览器渲染可由 headless Chrome/Puppeteer 自动化），并指出各导出器不支持全部形状/特征；模型 inspections 可用于结构检查，但需自建视觉检查。
- 版本化/权限/运行约束:DSL/JSON/workspace 可 Git 版本化，workspace 可本地/服务器管理，themes 可能要求 Internet；CLI 是 Java 应用，静态/PNG/SVG 还需浏览器。模型与导出产物要限制目录、版本和网络权限。
- 差异:比 DOT/Mermaid/PlantUML 更强的架构对象模型和多视图治理，尤其适合长期 architecture index；代价是引入 C4 workspace、Java/CLI 或服务，迁移不是当前 UI 小改。
- 决策:列为统一架构模型的长期候选：如果用户接受新增 workspace/C4 真源，可用 Structurizr DSL 作模型层、DOT/PlantUML/Mermaid 作导出层；本条不擅自把现有 architecture index 改成 Structurizr 真源。

## 科学绘图方案

### Vega-Lite
- 出处: https://vega.github.io/vega-lite/docs/
- 证据域:文献/官方文档
- 证据等级: V2
- 证据锚:官方 Overview 正文定义 Vega-Lite 为高层图形语法、声明式 JSON spec，编译到 Vega；目录正文覆盖 data/transform/aggregate/bin/mark/encoding/axis/legend/scale/layer/facet/concat/parameter/tooltips 等
- 证据深度:官方文档正文级
- 输入模型:结构化 JSON，数据、转换、mark、encoding、组合和交互参数均可声明；最适合 Agent 的默认 API，但必须在仓内增加字段类型、单位、统计意图和数据快照检查。
- 自动编码/布局:编译器负责从高层语法生成 Vega，支持聚合、bin、regression、layer/facet/repeat 等；布局与图形语义比任意脚本可验证，但默认不等于科学结论正确，需检查聚合/尺度/缺失值。
- 渲染/导出/可读性反馈:官方文档覆盖 axis/legend/scale/tooltip/ARIA 配置和多视图；仓内已有 vl-convert PNG+SVG 主轨。仍需补统一尺寸、字号、对比度、数据/证据注记和产物 manifest。
- 版本化/权限/运行约束:JSON spec 可 Git 版本化，渲染器版本需锁定；仓内 `plot_tool.rs:111-180,457-602` 已做 workdir 边界和缺渲染器诊断，但当前没有 spec hash/依赖版本/种子 manifest。
- 差异:比 matplotlib/PGFPlots 更适合作为统一结构化 Agent 输入和机器验证默认轨；与现有代码复用最高，但要从“最低字段检查”升级到研究语义 lint。
- 决策:保留为科学图表默认轨，统一 API 以 Vega-Lite 风格 spec 为核心；不把现有 R-274 实现重复申报为本条交付，增强项放迁移边界。

### Matplotlib
- 出处: https://matplotlib.org/stable/
- 证据域:文献/官方文档
- 证据等级: V2
- 证据锚:官方首页正文定义其可创建 static/animated/interactive visualizations，列出 plot types、user guide、examples、API reference、backends 和安装方式（pip/conda/pixi/uv），并提示 uv/Python/backend 环境差异
- 证据深度:官方文档正文级
- 输入模型:Python 程序和任意数据结构，表达力/生态最强，但输入不是可限制的图表 schema；Agent 需要生成脚本、依赖、数据快照和 seed，验证成本高。
- 自动编码/布局:pyplot/figure/subplots 与大量绘图类型、第三方 styles/backend；布局几乎由脚本负责，可实现复杂科研图但容易产生不可复现的隐式全局状态。
- 渲染/导出/可读性反馈:支持静态/动画/交互后端，输出能力广；可通过代码设置字体、颜色、dpi、label 和 layout，但官方方案不替应用统一生成可读性报告。
- 版本化/权限/运行约束:官方安装依赖 Python/包管理器和 backend；仓内轨道通过 uv isolated 或系统 Python 执行任意脚本，属于高权限高级轨道，必须沙箱/超时/输出白名单并记录版本。
- 差异:比 Vega-Lite 表达力和生态高，尤其适合统计/特殊图；比 PGFPlots 不保证 TeX 字体一致；比结构化 spec 更难做语义与安全门禁。
- 决策:保留为显式 Python 高级轨道，不作为默认 Agent 绘图入口；只有通过受控脚本、依赖锁定、seed/数据快照和 PNG 合法性验证才能产出研究图。

### PGFPlots
- 出处: https://github.com/pgf-tikz/pgfplots
- 出处: https://tikz.dev/pgfplots/
- 证据域:文献/官方仓库与其手册 HTML
- 证据等级: V2
- 证据锚:官方仓库 README 与手册正文说明其直接在 TeX 中绘制 2D/3D、提供 axis labels/legend/coordinates、自动 axis scaling/ticks，支持 line/scatter/bar/area/mesh/surface/contour/quiver/histogram/box/polar/ternary 等，并强调文档字体/字号/数学模式与图的一致性
- 证据深度:仓库 README + 手册正文级（HTML 明确为非官方转换版，官方 PDF/仓库链接可追溯）
- 输入模型:TeX/TikZ/PGFPlots 代码与 coordinates/table，适合论文级图和数学标注；不是通用安全 schema，Agent 需限定 axis/数据入口并避免任意宏/文件读写。
- 自动编码/布局:PGFPlots 负责 axis scaling、log/ticks、plot 类型和 style cycle，布局由 TeX/PGFPlots 代码与文档 preamble 决定；优点是论文一致性，缺点是编译和宏包环境复杂。
- 渲染/导出/可读性反馈:主要产出 PDF/矢量图，也可转 PNG；官方资料强调字体、数学模式和跨文档样式一致性，但没有统一语义/可读性评分。仓内 `plot_tool.rs:283-335` 已有 LaTeX→PDF→PNG 轨道和模板测试。
- 版本化/权限/运行约束:TeX 源可 Git 版本化，实际依赖 TeX 发行版/宏包；编译进程、输入路径、超时和字体必须受控，且产物应记录 TeX/PGFPlots 版本。
- 差异:比 matplotlib 更适合论文字体/数学一致性，比 Vega-Lite 更适合复杂 TeX 标注；编译依赖和任意 TeX 权限显著高于默认轨。
- 决策:保留为论文排版高级轨道；默认 Agent 先用 Vega-Lite，只有研究报告明确需要 TeX 字体/数学一致性时才启用，并强制源码/数据/版本/编译诊断 manifest。

<!-- 每条使用：### 方案名；- 出处: https://...；- 证据等级: V0..V3；- 差异: ...；- 决策: 采用/不采用及理由 -->

## 仓内既有设计

### 当前 architecture browser：workspace 依赖 SVG + 文档树降级
- 出处: file:crates/kanzei-app/src/docs.rs:808
- 出处: file:crates/kanzei-app/src/docs.rs:854
- 出处: file:crates/kanzei-app/ui/19-arch.js:160
- 出处: file:crates/kanzei-app/ui/19-arch.js:201
- 出处: file:crates/kanzei-app/ui/19-arch.js:231
- 出处: file:crates/kanzei-app/ui/19-arch.js:268
- 出处: file:scripts/ui-runtime-smoke.mjs:7710
- 证据域:代码域
- 证据等级: V2
- 证据锚:T-1786922726819（`node --experimental-vm-modules scripts/ui-runtime-smoke.mjs`，27 个脚本、0 个运行时错误；架构段断言 SVG、至少 6 节点/6 边、文字树保留与节点点击读取）
- 证据深度:运行时实测，并由源码核实
- 输入模型:后端从 Cargo.toml workspace members 与 `kanzei-*` 依赖抽取二元边，另返回 architecture index 文本和 docs/design 文件清单；不返回模块、接口、数据流、调用关系或设计文档之间的语义边。
- 自动布局/编码:前端按节点入度排序后使用固定 `W=320`、`ROW_H=56`、`COL_W=90` 的三列坐标；每个 crate 一个固定 52×22 矩形，8px 文本，边为直线箭头。没有图布局引擎、边交叉/循环处理、分组/折叠、缩放、全景或拥挤度反馈。
- 渲染/导出/可读性:自绘 SVG 仅写入 DOM，当前无 SVG/PNG/PDF 导出、无布局质量报告、无文字溢出/重叠断言；异常或旧环境直接隐藏图并保留文字树。现有 smoke 证明“能渲染”，不能证明“可读”。
- 版本化/权限/运行约束:架构索引的写入由专用 `architecture` 工具 CAS + 校验保护（file:crates/kanzei-tools/src/architecture.rs:39-182），浏览命令只读并已注册为 Tauri command（file:crates/kanzei-app/src/main.rs:231-235）；图本身没有 spec/布局版本、输入 hash、渲染器版本或导出 manifest。
- 差异:保留现有 architecture_snapshot 真源、文字树降级和离线零依赖；改变方向应从“固定自绘依赖草图”升级为结构化图工件 + 可验证布局/导出，而不是继续堆 CSS/SVG 特例。
- 决策:采用现有索引/文档作为架构图输入的兼容层；不把当前 SVG 当作最终架构图方案，待 B2 对照后确定 Graphviz/ELK/Mermaid/PlantUML 等布局候选及迁移顺序。

### 当前 plot 工具：Vega-Lite 主轨 + PGFPlots + matplotlib 增强轨
- 出处: file:crates/kanzei-tools/src/plot_tool.rs:111
- 出处: file:crates/kanzei-tools/src/plot_tool.rs:283
- 出处: file:crates/kanzei-tools/src/plot_tool.rs:457
- 出处: file:crates/kanzei-tools/src/plot_tool.rs:602
- 出处: file:crates/kanzei-tools/src/plot_tool.rs:656
- 出处: file:crates/kanzei-tools/src/base.rs:62
- 证据域:代码域
- 证据等级: V2
- 证据锚:T-1786922726819（`cargo test -p kanzei-tools plot_tool` 17 passed；同记录还包含架构边测试和 UI 冒烟）
- 证据深度:源码逐段核实 + 真实环境测试，其中依赖缺失路径由单测覆盖
- 输入模型:单一 `plot` 工具允许 `spec`/`spec_file`、`engine`、TikZ 或 Python 字符串、workdir、尺寸和色板；Vega 只机械检查 JSON、`mark`、`data/layer`，不检查字段类型、轴/图例/单位、统计误用或可读性；另外两轨接受任意代码片段。
- 自动编码/布局:Vega-Lite 把编码/布局责任交给 agent 提供的 spec；PGFPlots 与 matplotlib 把全部编码/布局责任交给 TikZ/Python。内置色板、宽高注入与 workdir 路径边界已有实现，但没有统一的图表语义 schema、无障碍色彩/字号/对比度门禁或数据到视觉编码的审计报告。
- 渲染/导出/可读性:Vega 依赖 PATH 中的 `vl-convert`，产出 PNG 回模型且 SVG 落盘；PGFPlots 经 LaTeX 产 PDF+PNG；matplotlib 依赖 uv/Python，要求脚本自行保存 PNG。测试已验证 PNG 魔数、SVG 实体落盘和错误诊断，但没有统一 PDF/SVG/PNG manifest，也没有跨引擎像素/字体/可读性基线。
- 版本化/权限/运行约束:工具通过 `workdir` 资源与研究目录边界限制写入，渲染器按本机 PATH 探测，缺失时给下载/安装指引；spec、脚本和输出虽落盘，却没有输入 hash、引擎/依赖版本、随机种子、数据快照或 provenance manifest。PGFPlots/matplotlib 还扩大了本机工具链与执行权限面。
- 差异:保留 Vega-Lite 的结构化 spec、PNG 回模型、SVG/PDF 落盘和现有诊断；改变为统一 envelope、语义验证、固定环境/版本记录和多格式产物清单；任意 Python/TikZ 只作为显式高级逃生舱，不作为默认 Agent API。
- 决策:现有 plot 能力标记为“已有可调用渲染能力”，不重复申报为本设计交付；B3 设计统一入口和验证契约，B2 再对照科学绘图方案后决定默认轨道与高级轨道权限。

### B1 已登记问题与证据边界
- 出处: file:crates/kanzei-app/ui/19-arch.js:143
- 证据域:代码域
- 证据等级: V1
- 证据锚:重复注册 `arch-goto-memory` click handler
- 证据深度:源码逐行核实，尚未声称用户现场复现
- 差异:这是现行页面的接线缺陷，不是绘图引擎能力；修复由 D-721 负责，R-335 不在审计设计批次内修改。
- 决策:设计 API 必须把每个动作绑定到单一消费者，并把入口重复触发纳入后续 UI 回归；当前保留缺陷状态，不将定向 smoke 的“0 runtime error”误解释为该问题不存在。

<!-- B2 补齐外部架构图/科学绘图方案；B3 写入统一 Agent 工具设计 -->
