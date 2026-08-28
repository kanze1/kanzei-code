# Agent 绘图工具：架构图与研究科学图表统一设计

- 状态：草案
- 日期：2026-08-28
- 关联需求：R-335
- 关联缺陷：D-721
- 关联决策：无（最终引擎组合与迁移顺序待用户评审）

## 背景与问题

当前仓库有两条互不统一的绘图路径：architecture browser 从 `architecture_snapshot` 得到 workspace crate 依赖边后，在 `crates/kanzei-app/ui/19-arch.js:160-266` 内用固定三列和自绘 SVG 展示；research/plot 通过 `crates/kanzei-tools/src/plot_tool.rs:111-602` 接受 Vega-Lite JSON、PGFPlots/TikZ 或 matplotlib/Python，并按本机渲染器输出。两条路径都能产生可见结果，但不能把“渲染成功”当作“结构正确、布局可读、研究可复现”。

现状审计和证据在 `.kanzei/research/agent-visualization-tools/prior-art.md`：

- architecture browser 的真源、调用链和现有 smoke 证据：`crates/kanzei-app/src/docs.rs:808-889`、`crates/kanzei-app/ui/19-arch.js:160-266`、`scripts/ui-runtime-smoke.mjs:7710-7741`、`T-1786922726819`。
- 当前 architecture smoke 只验证 SVG、节点/边数量、文字树降级和节点读取，不验证边交叉、标签溢出、密度、导出或版本 manifest。
- plot 已有三轨和错误诊断，但输入校验仍主要是 JSON/`mark`/`data` 等结构检查；任意 Python/TikZ 的安全、语义和复现责任仍在 Agent。
- B2 对照了 Graphviz、Mermaid、PlantUML、Structurizr DSL，以及 Vega-Lite、Matplotlib、PGFPlots 的官方正文资料。对照不是迁移批准；推荐候选仍要经过用户评审。
- `D-721` 记录 architecture browser 的 `arch-goto-memory` 重复 click 绑定。它是页面接线缺陷，不由本文直接修复。

## 目标与非目标

### 目标

1. 给 Agent 一个同时支持架构图和科学图表的结构化绘图入口，避免“只给自由文本/脚本”造成不可验证结果。
2. 把模型、布局/编码、渲染、质量验证和产物登记分层；每一层输出可供下一层检查的工件。
3. 支持离线优先、可重放、可审阅的 SVG/PNG/PDF（按图种和用户请求选择），并生成输入/引擎/输出 hash 清单。
4. 把错误变成机器可读取的阶段、路径、严重级别、修复建议和可重试性，而不是只返回一段失败文本。
5. 保持 architecture index、research 数据文件和现有 `plot` 能力的真源边界；未来迁移可以由兼容适配器逐步完成。

### 非目标

- 本条不立即引入 Graphviz、Mermaid、PlantUML、Structurizr、Python、TeX 或新的 Tauri command。
- 不把任意绘图输出当作研究证据；图形只能呈现已有数据和证据引用，不能替代来源、统计方法或用户结论。
- 不把 architecture index 改成 Structurizr workspace，也不把 plot 数据库改成新的图形专用真源。
- 不在未获得用户对引擎组合、交互图、默认主题和依赖打包的确认前锁定迁移路线。
- 不在浏览器中执行未审查的 Agent 生成代码；Python/TikZ 只能作为后续受控高级轨道。

## 讨论摘要

外部方案的共同有效点不是某个固定视觉主题，而是分离“声明的模型”和“生成的图”：Structurizr 用 C4 workspace/model/views 管理架构对象，Graphviz 把 DOT 模型交给专门布局引擎，Mermaid/PlantUML 用短文本得到可版本化的图形源码；Vega-Lite 用声明式 JSON 约束数据、mark、encoding 和组合；Matplotlib/PGFPlots 则提供更高表达力或论文排版一致性，但也要求更严格的执行环境、依赖和验证。

因此统一设计不选择一个万能后端，而选择一个稳定 envelope：输入必须带图种、数据引用、语义意图和权限级别；中间必须留下规范化 spec；输出必须带质量检查和 manifest。架构模型与科学数据模型不同，不能强行共用节点或数据字段，但可以共用调用、验证、错误和产物契约。

## 候选方案

| 方案 | 适合范围 | 主要优点 | 主要代价 | 本设计定位 |
|---|---|---|---|---|
| Graphviz DOT | 架构依赖、模块关系、层级图 | 多布局引擎、cluster、SVG/PNG/PDF 等输出，适合 CLI | 低层模型；需本机依赖和结果质量检查 | 架构布局首选候选 |
| Mermaid | 轻量预览、文档内图、部分 C4 | Agent 易生成、Web SVG、文本易审查 | C4 experimental；官方明确不完全自动布局；security/font 细节敏感 | 轻量预览候选 |
| PlantUML | UML、deployment、Archimate、序列图 | 语义图种多、CLI 检查、多格式输出 | Java/Graphviz 依赖；自由文本和宏权限面大 | 受控高级架构轨 |
| Structurizr DSL | 长期 C4 架构模型、多视图、ADR | workspace/model/views、inspections、多导出器 | 引入新的模型层和 Java/服务/浏览器约束 | 长期模型层候选 |
| Vega-Lite | research 默认科学图表 | JSON 声明式、编码/聚合/组合可检查、现有实现复用高 | 仍需科学语义 lint 和渲染器版本固定 | 科学图表默认轨 |
| Matplotlib | 特殊统计图、复杂定制 | Python 生态与表达力最强 | 任意脚本、依赖/权限/可复现成本高 | 受控高级科学轨 |
| PGFPlots | 论文图、TeX 数学与字体一致性 | axis/ticks/数学模式和跨文档样式一致 | TeX 编译环境重，任意宏权限高 | 论文排版高级轨 |

这些取舍的出处、V0-V3 等级和正文锚点见 `.kanzei/research/agent-visualization-tools/prior-art.md`，不在本文重复冒充新的外部证据。

## 最终方案

“最终方案”在本设计阶段是**待评审的契约方案**，不是最终引擎批准。引擎组合的推荐默认值如下：架构以现有 `architecture_snapshot` 为兼容输入，长期模型候选为 Structurizr DSL；布局/导出优先候选为 Graphviz；Mermaid 只做轻量预览，PlantUML 只做受控高级 UML/Archimate。科学图表默认使用 Vega-Lite；Matplotlib 和 PGFPlots 仅在明确的高级权限/研究排版需求下启用。

### 统一 Agent API

未来提供一个统一入口（名称暂定 `visualize`，不是本条新增命令），所有字段使用 `snake_case`：

```json
{
  "action": "plan | render | inspect",
  "kind": "architecture | scientific",
  "input": {
    "source_ref": "architecture_snapshot | research_artifact:<id>",
    "data_path": "research/data/example.json",
    "data_sha256": "...",
    "spec": {},
    "spec_path": "..."
  },
  "intent": {
    "title": "...",
    "claim_scope": "descriptive | exploratory | report",
    "evidence_refs": ["file:...", "T-..."]
  },
  "engine": "auto | graphviz | mermaid | plantuml | structurizr | vega_lite | matplotlib | pgfplots",
  "layout": {
    "direction": "top_to_bottom",
    "max_width": 1600,
    "max_height": 1200,
    "seed": 0
  },
  "style": {
    "theme": "kanzei_dark",
    "font_family": "...",
    "min_font_px": 12,
    "palette": "okabe_ito"
  },
  "outputs": ["spec", "svg", "png", "pdf"],
  "artifact_dir": "research/artifacts/<artifact_id>",
  "permission": "default | elevated"
}
```

字段约束：

- `kind=architecture` 时，默认 `source_ref` 必须是 `architecture_snapshot` 或其可验证快照；不能让 Agent 直接伪造当前 workspace 边。
- `kind=scientific` 时，`data_path`、`data_sha256`、数据字段说明和 `evidence_refs` 必须存在；内嵌小数据也必须在规范化工件中落盘。
- `spec` 与 `spec_path` 二选一；所有规范化 spec 必须写入 artifact directory，并计算 `spec_sha256`。
- `action=plan` 只生成选择的引擎、权限需求、检查计划和预期产物，不执行渲染；`render` 必须先通过计划阶段；`inspect` 只读取已有 manifest/产物并返回检查结果。
- `engine=auto` 只能按已批准的默认映射选择，不能绕过权限；`matplotlib`、`pgfplots`、PlantUML 高级能力和网络主题必须显式升级 `permission`。
- `outputs` 是允许的集合，默认架构图为 `spec,svg,png`，科学图表为 `spec,svg,png`；`pdf` 只在环境和请求明确支持时生成，缺失不应静默冒充成功。

### 两类最小成功闭环

#### 架构图闭环

1. 读取真实 `architecture_snapshot`，记录 index hash、workspace 输入 hash 和 `source_ref`。
2. 规范化为带稳定 ID 的 `nodes`、`edges`、可选 `groups`、`views` 和 `relations`；保留未知关系而不静默丢弃。
3. 运行结构检查：节点/边引用存在、重复边策略明确、循环和孤立节点报告、输入数量与来源快照一致。
4. 运行批准的布局后端；输出规范化 layout（节点 box、边 path、标签位置），而不是只保留不可复查的图片。
5. 运行视觉/可读性检查：节点重叠、标签越界/截断、边穿过节点、边交叉数量、画布边界、最小字号、对比度、孤立/未解析关系和图密度。
6. 通过后输出 DOT/DSL/JSON 等 source spec、SVG、PNG（及用户批准的 PDF）和 manifest；失败只输出诊断，不发布“成功图”。
7. 前端只消费 manifest 中的可信 SVG/PNG 和节点 ID 回源链接；文字树仍是数据故障时的明确降级视图。

#### 科学图表闭环

1. 从 research mode 的来源/发现/数据工件读取数据，锁定 `data_sha256`、字段类型、单位、缺失值处理和证据引用。
2. 生成规范化 Vega-Lite 风格 spec，明确 mark、x/y/color/size、聚合/变换、尺度、标题、轴、图例、注释和 claim scope。
3. 运行语义检查：字段存在且类型匹配、聚合与单位不冲突、时间/类别排序明确、缺失/异常值策略明确、图形不暗示未声明的因果或统计结论。
4. 运行可读性检查：标题/轴/单位、字号、对比度、色盲友好 palette、图例、tooltip/ARIA（如适用）、长标签、裁剪、分面规模和打印尺寸。
5. 由 Vega-Lite 默认轨渲染 SVG/PNG；只有 report 明确需要 TeX 或特殊图形时，才切换 PGFPlots/Matplotlib，并记录高级权限和依赖版本。
6. 输出 spec、数据快照引用、SVG/PNG（可选 PDF）和 manifest；报告中显示“图形产物”与“研究证据引用”两者的区别。
7. 研究报告只能引用 manifest 的输入/检查结果；图像本身不能作为无来源的新证据。

## 错误与验证契约

每次 `plan`、`render`、`inspect` 返回统一 envelope：

```json
{
  "ok": false,
  "phase": "input | semantic | layout | render | artifact | security",
  "error_code": "input_missing | source_changed | schema_invalid | semantic_invalid | layout_unreadable | renderer_missing | renderer_failed | artifact_incomplete | permission_denied",
  "severity": "error | warning",
  "retryable": true,
  "message": "面向 Agent 的短诊断",
  "diagnostics": [
    {
      "path": "input.spec.encoding.x.field",
      "line": 12,
      "code": "field_missing",
      "message": "字段不存在",
      "suggestion": "从 data_columns 选择一个实际字段"
    }
  ],
  "checks": [],
  "artifact_manifest": null
}
```

验证分为四层：

1. **输入层**：路径在允许的 project/research artifact 范围内，文件存在，hash 与声明一致，JSON/DSL/脚本可解析；输入变化返回 `source_changed`，不得继续使用旧布局。
2. **语义层**：架构图检查引用和关系完整性；科学图检查字段/单位/聚合/尺度/claim scope。语义警告可以进入人工审阅，但 `error` 不得生成可交付成功状态。
3. **布局/渲染层**：检查 renderer 是否存在、版本是否满足、画布/字号/重叠/裁剪/字体和输出魔数；渲染器报错时保留 stdout/stderr 摘要和可修复路径，不把空文件算成功。
4. **产物层**：每个声明输出必须是存在、非空、媒体类型正确、hash 可读的文件；manifest、spec、检查结果和输出必须同目录原子落盘，否则返回 `artifact_incomplete`。

错误反馈必须告诉 Agent“哪一层失败、哪个路径、是否可重试、下一步要改 spec/环境/权限还是请求用户”。禁止用“请换一个引擎”替代具体诊断，也禁止隐藏安全拒绝或依赖缺失。

## 产物格式、版本化与运行权限

每次成功或失败的 render 都在隔离 artifact directory 生成 `manifest.json`；建议结构如下：

```json
{
  "manifest_version": "1",
  "artifact_id": "...",
  "kind": "architecture | scientific",
  "source_refs": ["architecture_snapshot", "research_artifact:..."] ,
  "input_sha256": "...",
  "spec_path": "spec.json",
  "spec_sha256": "...",
  "engine": "graphviz",
  "engine_version": "...",
  "dependency_versions": {"graphviz": "...", "font": "..."},
  "layout": {"name": "dot", "seed": 0, "direction": "top_to_bottom"},
  "theme": {"name": "kanzei_dark", "font_family": "...", "palette": "..."},
  "permission": "default | elevated",
  "command": ["..."],
  "checks": [{"name": "label_overflow", "status": "passed", "details": "..."}],
  "outputs": [
    {"path": "diagram.svg", "media_type": "image/svg+xml", "bytes": 0, "sha256": "..."}
  ],
  "status": "passed | failed",
  "created_at": "..."
}
```

- `spec.json`/`source.*` 是可审阅、可 Git 版本化的输入工件；图片是派生物，不能反向成为真源。
- `input_sha256`、`spec_sha256`、引擎/依赖版本、字体/主题、布局参数和 seed 是复现所需最小集合；随机渲染无法提供 seed 时必须显式记为 `non_deterministic` 警告。
- 默认产物要求 SVG + PNG；PDF 作为报告/论文可选产物，必须记录实际生成引擎。不存在的 PDF 不得用 PNG 改名或空文件冒充。
- 默认权限只允许读取已声明的数据和写入 artifact directory；禁止网络、任意工作目录、未声明文件和静默覆盖。
- Matplotlib/Python、PGFPlots/TeX、PlantUML/Java 和 Graphviz 进程属于高级或受控运行轨道：需要 allowlist、超时、输出白名单、资源限制、stderr 保存和版本探测；不能在 UI 主线程执行。
- 主题、字体、palette 是版本化配置，不应仅由 CSS 当前状态决定；缺字体/色板时返回 warning 或 error，不能默默换成不可读默认值。

## 实施边界与调用方

### 保留项（已有能力，非本条交付）

- `architecture_snapshot` 继续从 architecture index、Cargo workspace 和 design docs 读取；architecture index 的写入仍由专用 architecture 工具的 hash/校验契约保护。
- `arch-tree` 作为图失败或旧环境下的文字降级；架构节点的回源读取继续走真实 `docs_read_custom` 调用方。
- 当前 `plot` 的 Vega-Lite PNG 回模型、SVG 落盘、PGFPlots/Matplotlib 轨道和已有错误单测继续可用；这些是既有 capability，不重复申报为本设计实现。
- research mode 的来源、发现、数据和报告真源继续由现有 research workspace 约束；图形只是派生 artifact。

### 改变项（后续实施条目）

- 增加一个统一 adapter/normalizer，把 architecture snapshot 与 research 数据转换为统一 envelope，不让各引擎直接接收未经声明的自由输入。
- 增加 plan→validate→render→inspect 的可调用链，并让工具输出带结构化错误和 manifest；所有输出通过真实 Agent/tool consumer 回到会话和 research artifact。
- architecture 先补结构化关系/视图和可读性检查，再接 Graphviz 或其他批准后端；不能先把当前自绘 SVG 换成另一张没有验证的图片。
- plot 先补数据/统计语义 lint、字号/对比度/色板/输出 manifest；高级脚本轨道随后单独加权限和隔离。
- UI 只展示产物和检查结果，并提供回源链接；不在前端复制引擎选择、权限或语义判定。

### 调用方与接入边界

统一工具必须有两个真实消费者：

1. Agent tool pipeline：Agent 通过 `plan/render/inspect` 调用，收到 `diagnostics`、manifest 和 artifact 引用，并可据错误一轮修复 spec；不能只生成一个展示 shell。
2. architecture/research artifact consumer：architecture browser 消费架构图 manifest，research workspace/报告消费科学图表 manifest；两者都能回到 source/data/evidence refs。

本条只定义接口，未新增上述调用方实现。后续实施必须先在 kanzei-tools 注册真实 tool，再接 runner/权限和桌面转发；没有真实注册和消费者的设计命令不算交付。

## 用户待拍板项

以下事项不由 Agent 自行决定，当前保持草案：

1. 架构最终是否采用 Structurizr DSL 作为长期模型层，及是否允许 Graphviz/Java/浏览器渲染依赖。
2. Mermaid 是否只作为预览，PlantUML 是否允许进入默认安装，Graphviz 是否随应用打包或由用户安装。
3. 是否支持交互式架构图（节点点击、筛选、局部展开）以及交互状态是否进入 manifest；静态 SVG/PNG/PDF 是当前最小范围。
4. research mode 默认主题、字体、色板和 PDF/TeX 需求；当前只推荐 Vega-Lite 默认轨，不锁定视觉 taste。
5. Python/TeX/Java 高级轨的权限模型、沙箱/超时/依赖缓存和发布包体边界。
6. `visualize` 的最终命名、schema 版本、迁移顺序，以及是否把当前 `plot` 兼容包装为新入口。

## 变更记录

- 2026-08-28：新增草案，承接 R-335 B1/B2 审计与先行方案对照；定义统一 envelope、两类最小闭环、错误/验证/manifest 和迁移边界。来源：`.kanzei/research/agent-visualization-tools/prior-art.md`、`T-1786922726819`、D-721。

## 验证证据

- B1 当前实现审计和定向验证：`T-1786922726819`；`cargo test -p kanzei-tools plot_tool` 17 passed，`cargo test -p kanzei-app workspace图` 1 passed，`node --experimental-vm-modules scripts/ui-runtime-smoke.mjs` 通过。
- B2 先行方案校验：`.kanzei/research/agent-visualization-tools/prior-art.md`，`prior_art validate --entry-ref R-335 --topic agent-visualization-tools` 返回 `valid=true`、`external_count=4`、`internal_count=3`。
- 设计证据边界：以上证据证明现状和资料工件，不证明本文设计已实现，也不证明最终引擎选择已被用户接受。

## TODO 与后续风险

- TODO：用户评审六项待拍板项后，登记独立实现条目；不得在 R-335 设计草案中直接迁移引擎。
- TODO：实现条目需为统一 API 增加 schema/单测、renderer capability 探测、错误 fixture、artifact hash/manifest 和真实 Agent/research consumer 回归。
- TODO：补架构图真实宽屏/大图视觉基线；现有 smoke 的 fixture 只覆盖节点/边存在，不覆盖读图质量。
- TODO：修复 D-721 的重复 click 绑定并补单击一次断言；不能用 R-335 设计文档关闭该缺陷。
- 风险：Structurizr/Graphviz/PlantUML 的 Java/CLI/Graphviz 依赖可能影响 Windows 离线发布；Mermaid 的 C4 experimental 和布局顺序限制可能无法满足稳定架构图；Matplotlib/TeX 任意代码轨道扩大权限与复现风险；字体、主题、headless 浏览器差异可能使像素结果不稳定。
