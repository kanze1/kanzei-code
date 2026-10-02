# R-221 研究计划：research 模式五批基座的实际落地

## 研究目标

核验已批准的 R-221 research 模式五批基座在仓库中的实际落地，重点检查：

- **B4 回流通道**：研究阶段的来源、finding、报告与待开发需求是否形成可追溯回流链路，且不直接修改既有需求/缺陷。
- **B5 记忆一元化**：研究模式是否通过统一的项目记忆检索与写入入口工作，避免重新启用历史 `.kanzei/research/memory.md` 或平行记忆账本。

## 执行步骤

1. 读取 `docs/design/research_mode.md`，核对五批基座、B4/B5 契约与验收要求。
2. 读取 `crates/kanzei-tools/src/profiles.rs`，核对 research profile 的真实实现、工具权限和运行入口。
3. 用 `memory_search` 检索与 R-221、research mode、B4 回流、B5 记忆一元化相关的项目记忆；不读取历史 `.kanzei/research/memory.md`。
4. 以只读 Git 操作核对既有 dev 实施提交 `ecfdca5b`，并将实际查阅的文档、源码、记忆与提交登记为 source。
5. 基于 `file:line`、提交和记忆锚点登记带 source refs 的 findings；每条结论明确代码/文献域、V0-V3、证据锚和文献证据深度。
6. 编写研究报告，保留“计划 → 来源 → finding → report → todo”的可复核链路；报告写入 `.kanzei/research/r221-chain/report.md`，并同步研究总报告 `.kanzei/research/report.md`。
7. 用 `req add` 创建一个带 `[todo]` 回流标记的研究草稿，引用本次 finding；不修改既有 R-/D- 条目，不提交 Git。

## 停止条件

完成上述链路后停止，不进入开发实施、不关闭或更新既有条目、不提交 Git。
