---
kind: prior_art
topic: agent-visualization-tools
status: pending
trigger: explicit_user
entry_refs: R-335
websearch_round_limit: 4
---

# 先行方案对照

> 完成前请补齐双侧至少各一条对照，并将 `status` 改为 `complete`。

## 外部已有实现

<!-- 每条使用：### 方案名；- 出处: https://...；- 证据等级: V0..V3；- 差异: ...；- 决策: 采用/不采用及理由 -->

## 仓内既有设计

### 当前 architecture browser：workspace 依赖 SVG + 文档树降级
- 出处:file:crates/kanzei-app/src/docs.rs:808-847,854-889; file:crates/kanzei-app/ui/19-arch.js:14-115,160-266; file:scripts/ui-runtime-smoke.mjs:7710-7741
- 证据域:代码域;证据等级:V2（运行时冒烟）;证据锚:T-1786922726819（`node --experimental-vm-modules scripts/ui-runtime-smoke.mjs`，27 个脚本、0 个运行时错误；架构段断言 SVG、至少 6 节点/6 边、文字树保留与节点点击读取）;证据深度:运行时实测，并由源码核实
- 输入模型:后端从 Cargo.toml workspace members 与 `kanzei-*` 依赖抽取二元边，另返回 architecture index 文本和 docs/design 文件清单；不返回模块、接口、数据流、调用关系或设计文档之间的语义边。
- 自动布局/编码:前端按节点入度排序后使用固定 `W=320`、`ROW_H=56`、`COL_W=90` 的三列坐标；每个 crate 一个固定 52×22 矩形，8px 文本，边为直线箭头。没有图布局引擎、边交叉/循环处理、分组/折叠、缩放、全景或拥挤度反馈。
- 渲染/导出/可读性:自绘 SVG 仅写入 DOM，当前无 SVG/PNG/PDF 导出、无布局质量报告、无文字溢出/重叠断言；异常或旧环境直接隐藏图并保留文字树。现有 smoke 证明“能渲染”，不能证明“可读”。
- 版本化/权限/运行约束:架构索引的写入由专用 `architecture` 工具 CAS + 校验保护（file:crates/kanzei-tools/src/architecture.rs:39-182），浏览命令只读并已注册为 Tauri command（file:crates/kanzei-app/src/main.rs:231-235）；图本身没有 spec/布局版本、输入 hash、渲染器版本或导出 manifest。
- 差异:保留现有 architecture_snapshot 真源、文字树降级和离线零依赖；改变方向应从“固定自绘依赖草图”升级为结构化图工件 + 可验证布局/导出，而不是继续堆 CSS/SVG 特例。
- 决策:采用现有索引/文档作为架构图输入的兼容层；不把当前 SVG 当作最终架构图方案，待 B2 对照后确定 Graphviz/ELK/Mermaid/PlantUML 等布局候选及迁移顺序。

### 当前 plot 工具：Vega-Lite 主轨 + PGFPlots + matplotlib 增强轨
- 出处:file:crates/kanzei-tools/src/plot_tool.rs:1-180,283-602,656-1067; file:crates/kanzei-tools/src/base.rs:62-70
- 证据域:代码域;证据等级:V2（定向测试实测）;证据锚:T-1786922726819（`cargo test -p kanzei-tools plot_tool` 17 passed；同记录还包含架构边测试和 UI 冒烟）;证据深度:源码逐段核实 + 真实环境测试，其中依赖缺失路径由单测覆盖
- 输入模型:单一 `plot` 工具允许 `spec`/`spec_file`、`engine`、TikZ 或 Python 字符串、workdir、尺寸和色板；Vega 只机械检查 JSON、`mark`、`data/layer`，不检查字段类型、轴/图例/单位、统计误用或可读性；另外两轨接受任意代码片段。
- 自动编码/布局:Vega-Lite 把编码/布局责任交给 agent 提供的 spec；PGFPlots 与 matplotlib 把全部编码/布局责任交给 TikZ/Python。内置色板、宽高注入与 workdir 路径边界已有实现，但没有统一的图表语义 schema、无障碍色彩/字号/对比度门禁或数据到视觉编码的审计报告。
- 渲染/导出/可读性:Vega 依赖 PATH 中的 `vl-convert`，产出 PNG 回模型且 SVG 落盘；PGFPlots 经 LaTeX 产 PDF+PNG；matplotlib 依赖 uv/Python，要求脚本自行保存 PNG。测试已验证 PNG 魔数、SVG 实体落盘和错误诊断，但没有统一 PDF/SVG/PNG manifest，也没有跨引擎像素/字体/可读性基线。
- 版本化/权限/运行约束:工具通过 `workdir` 资源与研究目录边界限制写入，渲染器按本机 PATH 探测，缺失时给下载/安装指引；spec、脚本和输出虽落盘，却没有输入 hash、引擎/依赖版本、随机种子、数据快照或 provenance manifest。PGFPlots/matplotlib 还扩大了本机工具链与执行权限面。
- 差异:保留 Vega-Lite 的结构化 spec、PNG 回模型、SVG/PDF 落盘和现有诊断；改变为统一 envelope、语义验证、固定环境/版本记录和多格式产物清单；任意 Python/TikZ 只作为显式高级逃生舱，不作为默认 Agent API。
- 决策:现有 plot 能力标记为“已有可调用渲染能力”，不重复申报为本设计交付；B3 设计统一入口和验证契约，B2 再对照科学绘图方案后决定默认轨道与高级轨道权限。

### B1 已登记问题与证据边界
- 出处:file:crates/kanzei-app/ui/19-arch.js:143-152; tracker:D-721
- 证据域:代码域;证据等级:V1（读码核实）;证据锚:重复注册 `arch-goto-memory` click handler；证据深度:源码逐行核实，尚未声称用户现场复现
- 差异:这是现行页面的接线缺陷，不是绘图引擎能力；修复由 D-721 负责，R-335 不在审计设计批次内修改。
- 决策:设计 API 必须把每个动作绑定到单一消费者，并把入口重复触发纳入后续 UI 回归；当前保留缺陷状态，不将定向 smoke 的“0 runtime error”误解释为该问题不存在。

<!-- B2 补齐外部架构图/科学绘图方案；B3 写入统一 Agent 工具设计 -->
