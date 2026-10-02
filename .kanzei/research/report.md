# R-221 Research 总报告

- 课题：`r221-chain`
- 范围：R-221 B4 回流通道、B5 记忆一元化
- 总体证据等级：**V1（代码域，读码核实）**
- 详细报告：`.kanzei/research/r221-chain/report.md`
- 研究计划：`.kanzei/research/r221-chain/plan.md`

## 结论摘要

1. **B4（代码域，V1）**：`ResearchProfile` 注册 `source`/`finding` 并使 finding 引用来源；`req`/`defect` 仅开放既有条目 `get` 与新增 `add`，既有状态变更动作硬拒绝；研究上下文把 backlog 作为只读索引，并规定 add 产出 `[todo]` 草稿。证据锚：`crates/kanzei-tools/src/profiles.rs:609-743`、`docs/design/research_mode.md:32-37,95-109,120-130`、提交 `ecfdca5b`。Finding：**F-001**；来源：**S-001/S-002/S-003**。
2. **B5（代码域，V1）**：research 放行统一 `memory_search`/`memory_note`，上下文和 agent 提示词明确不使用历史 `.kanzei/research/memory.md`；设计要求 manager 晋升，但本次没有运行时实测。证据锚：`crates/kanzei-tools/src/profiles.rs:631-642,715-763`、`docs/design/research_mode.md:32-37,95-100,113-130`、提交 `ecfdca5b`。Finding：**F-002**；来源：**S-001/S-002/S-003**。

文献证据深度：**不适用**（本次结论全属代码/项目设计域）。本次未读取历史 `.kanzei/research/memory.md`，未修改既有 R-/D- 条目，未提交 Git。

## 回流

已登记一个引用 F-001/F-002 的 `[todo]` 研究草稿，交由 dev 后续审阅；本次研究到此停止。
