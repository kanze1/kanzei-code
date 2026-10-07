# Kanzei 项目架构与边界知识整理

日期：2026-10-07。按用户要求先整理架构与记忆，再独立测边界探索收益。

这次整理改变项目知识资产，不修改运行代码。参考发布基线 `8e96c7eaf1931d9b944a5410da404e9c2006b13a`；当前工作区已有的并行运行改动继续保留，不能据此声明它们已发布或验证。记忆字段只描述读码确认的职责与条件，不把目录或图上的推断当成完整调用图。

架构入口：`docs/architecture/00_system_boundaries.md`。保留运行循环图，更新 Harness 的全局 Skills 来源，并补齐项目知识、记忆证据、对话生命周期与前后端消费边界。精确 crate 依赖仍由 Cargo 实时生成，手写图用于解释职责和流程。

## 知识目录

每条模块知识具有唯一 subject、显式 area、真实源码 refs、适用条件与限制。新事实通过现有管理工具生成 candidate，再以本次外部 Codex 源码审计的真实 episode 记录晋升；这是外部源码审计记录，不冒充产品模型探索或失败恢复运行。旧经验保留原始内容与失败指纹，补区域绑定；两个错位标题纠正为正文主题；旧候选门槛记录降级并保留墓碑。

| 主题键 | 职责或契约 | 核验源码 |
| --- | --- | --- |
| `responsibility:kanzei-base` | 基础层只提供文件、路径与审计原语 | `crates/kanzei-base/src/lib.rs`, `crates/kanzei-base/src/atomic_file.rs` |
| `responsibility:kanzei-harness` | Harness 定义能力契约与装配快照 | `crates/kanzei-harness/src/harness.rs`, `crates/kanzei-harness/src/tool.rs`, `crates/kanzei-harness/src/permission.rs` |
| `responsibility:kanzei-llm` | 协议层归一请求、流事件与错误 | `crates/kanzei-llm/src/lib.rs`, `crates/kanzei-llm/src/event.rs`, `crates/kanzei-llm/src/request.rs` |
| `responsibility:kanzei-core` | Core 负责执行循环和持久会话事实 | `crates/kanzei-core/src/lib.rs`, `crates/kanzei-core/src/runner/drive.rs`, `crates/kanzei-core/src/store/mod.rs` |
| `responsibility:kanzei-memory` | 记忆层保存真源并执行准入与证据门槛 | `crates/kanzei-memory/src/lib.rs`, `crates/kanzei-memory/src/memory/mod.rs`, `crates/kanzei-memory/src/memory/store.rs` |
| `responsibility:kanzei-tools` | 工具层组合能力并提供公共运行装配 | `crates/kanzei-tools/src/lib.rs`, `crates/kanzei-tools/src/run.rs` |
| `responsibility:kanzei-app` | 桌面端负责 IPC、界面与运行生命周期适配 | `crates/kanzei-app/src/main.rs`, `crates/kanzei-app/src/run/coordinator.rs`, `crates/kanzei-app/ui/07-events.js` |
| `responsibility:kanzei` | CLI 是共享能力的命令行入口 | `crates/kanzei/src/main.rs`, `crates/kanzei/src/cli/run.rs`, `crates/kanzei/src/cli/run/finalize.rs` |
| `contract:kanzei-tools/project_knowledge:two-roots` | 代码树与项目资产根必须分别传递 | `crates/kanzei-harness/src/tool.rs`, `crates/kanzei-tools/src/architecture.rs`, `crates/kanzei-tools/src/project_knowledge.rs` |
| `contract:kanzei-tools/project_knowledge:graph-evidence` | 层级与依赖图用于定位，不能证明完整影响范围 | `crates/kanzei-harness/src/areas.rs`, `crates/kanzei-tools/src/project_knowledge.rs`, `crates/kanzei-tools/src/arch_diagram.rs` |
| `contract:kanzei-tools/files:purpose-validity` | 用途摘要只在当前指纹匹配时有效 | `crates/kanzei-tools/src/files.rs`, `crates/kanzei-app/src/files_view.rs`, `crates/kanzei-tools/src/project_knowledge.rs` |
| `contract:kanzei-memory/memory:lifecycle-evidence` | 草稿、候选和有效记忆有不同的来源门槛 | `crates/kanzei-memory/src/memory/manager.rs`, `crates/kanzei-memory/src/memory/lifecycle.rs`, `crates/kanzei-memory/src/memory/store.rs` |
| `contract:kanzei-harness/tool:outcome-vs-error` | 工具错误布尔值与业务终态不能混用 | `crates/kanzei-harness/src/tool.rs`, `crates/kanzei-core/src/runner/recall.rs`, `crates/kanzei-core/src/runner/metrics.rs` |
| `contract:kanzei-memory/memory:recall-channels` | 开跑提示与失败召回走不同消息通道 | `crates/kanzei/src/cli/run.rs`, `crates/kanzei-core/src/runner/drive/assembly.rs`, `crates/kanzei-core/src/runner/recall.rs` |
| `contract:kanzei-core/store:segment-vs-purge` | 新段、关闭身份与永久清理是三种操作 | `crates/kanzei/src/cli/run.rs`, `crates/kanzei-core/src/store/session.rs`, `crates/kanzei-app/src/processes/lifecycle.rs` |
| `contract:kanzei-harness/skills:scope` | 全局 Skills 与项目知识开关不共用作用域 | `crates/kanzei-harness/src/skills.rs`, `crates/kanzei-tools/src/project_knowledge.rs`, `crates/kanzei-tools/src/run.rs` |
| `contract:kanzei-app:ipc-and-esm` | 前端接线须覆盖 command、订阅和源状态 | `crates/kanzei-app/src/main.rs`, `crates/kanzei-app/src/ipc_contract.rs`, `crates/kanzei-app/ui/01-core.js`, `crates/kanzei-app/ui/07-events.js` |
| `contract:kanzei-llm:stream-contract` | 公共消息变更须核对所有协议与流恢复 | `crates/kanzei-llm/src/event.rs`, `crates/kanzei-llm/src/request.rs`, `crates/kanzei-llm/src/protocol/mod.rs`, `crates/kanzei-core/src/runner/drive.rs` |
| `contract:kanzei-memory/memory:derived-and-current` | 检索索引与图投影不能升级候选或失效知识 | `crates/kanzei-memory/src/memory/store.rs`, `crates/kanzei-tools/src/project_knowledge.rs`, `crates/kanzei-tools/src/refgraph/memory_graph.rs` |
| `contract:kanzei-tools/run:shared-entry` | CLI 与桌面运行装配共用一个能力入口 | `crates/kanzei-tools/src/run.rs`, `crates/kanzei/src/cli/run.rs`, `crates/kanzei-app/src/run/assembly.rs` |

## 核验边界

- 用途摘要按当前代码指纹更新有限的核心文件和目录，不批量信任旧缓存。
- `directory_fingerprint` 不是递归子树 hash，size+mtime 快照也不是正文证明。
- `ToolOutcome::is_expected_rejection` 与 runner 的稳定头过滤当前列表不同，知识记录实际差异，不把设计意图写成现有行为。
- 永久清理过程对话有“活跃工作树留下时连同记录保留”的例外；退役历史删除是另一分支。
- Markdown 是真源，检索索引与图可重建，候选/失效/修订不匹配不当作有效约束。

原始备份、逐工具调用、源码指纹、投影快照与三组对照实验保存在 `dist/boundary-knowledge-20261007/`。复测保持同一代码和模型，比较整理前启用、整理后关闭层级、整理后启用层级，分别观察知识内容与层级组织的影响。
