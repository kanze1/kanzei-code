# R-221 B4/B5 研究报告

- 课题：r221-chain
- 范围：R-221 批4「回流通道」与批5「记忆一元化」
- 研究链路：计划 → 来源 → finding → 报告 → [todo] 回流草稿
- 总体证据等级：**V1（代码域，读码核实）**
- 文献证据深度：**不适用**；本报告只作代码/项目设计文档结论，不作文学或论文主张。
- 约束：未读取历史 `.kanzei/research/memory.md`；未修改既有 R-/D- 条目；未提交 Git。

## 1. 来源

- **S-001**：`docs/design/research_mode.md`，R-221 设计基线；核对定调点 4/5、工具面、批4/批5、回流和记忆边界（锚：§2、§6-§10）。
- **S-002**：`crates/kanzei-tools/src/profiles.rs`，`ResearchProfile::contribute` 真实实现（锚：602-769）。
- **S-003**：Git 提交 `ecfdca5b`，标题为「R-221 B5 research 记忆统一走 memory 工具」；作为提交级版本锚。
- **S-004**：`.kanzei/research/r221-chain/plan.md`，本次 B4/B5 执行范围和停止条件（锚：3-18、20-22）。

## 2. Findings

### F-001 — B4 研究结论可经受限 tracker 权限回流为草稿

- **结论**：B4 回流通道已在 research profile 形成受限的可追溯链路：source/finding 工具可登记研究证据，finding 强制引用 `SOURCES`；`req`/`defect` 仅允许 `get` 与 `add`，既有条目变更动作被 managed hard deny；`research/docs` 注入 Sources/Findings、只读 backlog，并明确新增条目是 `[todo]` 草稿。
- **域与等级**：代码域，**V1**（读码核实）。
- **证据锚**：`profiles.rs:609-629`（source/finding 注册及 finding→SOURCES refs）；`643-671`（四类 tracker 仅 `read:get`/`write:add`，禁止既有状态变更）；`674-685`（req/defect 研究工具）；`715-743`（研究上下文、只读 backlog、[todo] 回流契约）；设计对照 `research_mode.md:32-37,95-100,103-109,120-130`；提交锚 `ecfdca5b`。
- **文献证据深度**：不适用（代码域）。
- **来源 refs**：S-001、S-002、S-003。

### F-002 — B5 research 记忆统一走 memory 工具并停用历史账本

- **结论**：B5 已在 research profile 的工具面与上下文契约中落地：只注册并放行统一 `memory_search`/`memory_note`，`research/docs` 与 research agent 提示词均明示历史 `.kanzei/research/memory.md` 不是研究记忆来源；设计基线规定研究结论经 `memory_note→manager` 晋升，`refs.bib` 与记忆晋升分离，但本次没有把 manager 晋升宣称为运行时实测完成。
- **域与等级**：代码域，**V1**（读码核实）。
- **证据锚**：`profiles.rs:631-642`（memory 工具注册与放行）；`715-743`（统一记忆指导及历史 memory.md 禁止注入）；`746-763`（research agent 提示词）；设计对照 `research_mode.md:32-37,95-100,113-130`；提交锚 `ecfdca5b`。
- **文献证据深度**：不适用（代码域）。
- **来源 refs**：S-001、S-002、S-003。
- **边界**：需要后续运行时验证 memory_note 入 inbox、manager 晋升和检索回读，才能升级为 V2；本 finding 不替代该验证。

## 3. 结论与未决项

本次读码确认 R-221 B4/B5 的入口和硬权限边界已经存在：研究可以把带来源的 finding 转成待 dev 审阅的 tracker 草稿，且不能直接改动既有条目；研究记忆统一经 memory 工具，历史 research memory 文件不进入 research 上下文。证据当前为代码域 V1，未进行运行时复现，因此不宣称 V2/V3。

## 4. 建议登记

- 建议由 dev 审阅一个 `[todo]` 研究草稿，落实 B4 的真实 `req add` 回流验收；该草稿引用 **F-001** 与 **F-002**。
- 后续另行补运行时验收：验证 `memory_note` → inbox → manager 晋升 → `memory_search` 回读，并保留 V2 证据锚；不要把本次 V1 读码当成运行时证据。
