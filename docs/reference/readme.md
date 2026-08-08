# reference:opencode 上游规格参照(非本仓契约)

本目录内容于 **2026-08-06** 从 [opencode](https://github.com/anomalyco/opencode) 仓库整体拷贝,保留作为 kanzei(Rust 重写)的**架构参照与术语来源**。

> **重要声明**:以下文档是 opencode(TypeScript/Effect 栈)的 V2 设计与实现规格,**不是 kanzei 当前行为的契约**。kanzei 参考其分层、权限、会话与压缩思想,但具体实现、数据模型、命令面与状态机以本仓 `docs/design/` 与代码为准。两者出现不一致时,以 kanzei 的 Rust 实现与 `docs/design/` 为真源。

## 与 kanzei 的关系

| 文件 | 主题 | 与 kanzei 的关系 |
|---|---|---|
| `CONTEXT.md` | Session Runtime 术语与契约 | 术语参考(kanzei 参考其 context/epoch 概念) |
| `specs-v2/tools.md` | V2 工具 API 与输出边界 | 参考工具注册/权限/输出截断思想(见 `docs/design/harness_m1.md`) |
| `specs-v2/session.md` | Session API / Context Epoch / 自动压缩 | 参考会话调度与压缩思想(见 `docs/design/m2_sqlite_store.md`) |
| `specs-v2/provider-policy.md` | provider 策略 allow/deny | 参考 last-match-wins 规则(kanzei 权限 Ruleset 同源) |
| `specs-v2/provider-model.md` | provider/model catalog | 参考模型分层与解析链思路(见 `docs/design/deep_parallel_dev.md`) |
| `specs-v2/config.md` | V2 配置评审 | 参考配置分层与字段取舍 |
| `specs-v2/catalog-config-plugin-lifecycle.md` | catalog/config/plugin 生命周期 | 参考插件与热更新思路 |
| `specs-v2/schema-changelog.md` | 上游 schema 变更日志 | 仅上游历史,kanzei schema 独立(已归档至 `opencode-archive/`) |
| `specs-v2/instructions.md` | opencode V2 core 开发指引 | 仅上游开发规范,非 kanzei 规范(已归档至 `opencode-archive/`) |
| `specs-v2/todo.md` | opencode V2 内部待办 | 纯上游内部笔记,kanzei 无执行价值(已归档至 `opencode-archive/`) |
| `tui-package.md` | 上游 TUI 包抽取计划 | 与 kanzei(Tauri 桌面)无直接关系(已归档至 `opencode-archive/`) |

## 维护约定

- 本目录是**快照材料**:除非追踪条目明确要求对齐上游行为(如 `docs/design/direction_taste.md` 的复刻清单),否则不随上游更新。
- 新增 kanzei 自有规格一律写入 `docs/design/`,不要追加到本目录。
- 需要引用上游行为做对照时,在本目录文档之外(设计文档或追踪条目)写明"CC/opencode 基线",并给出此处文件路径。
