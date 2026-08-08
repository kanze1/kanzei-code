---
id: M-016
scope: project
category: fact
title: docs 目录整理(2026-08-08):design 统一 snake_case、reference 归档 opencode-archive、R-050 移入 deep_parallel_dev、旧 G-003 重编号 G-005
description: 处理 docs 目录/文档位置、R-050 POC 方案出处、goals 编号 G-003/G-005、architecture README 索引缺失条目(direction_taste/memory_system/deep_parallel_dev)时必读
status: active
created: 2026-08-08
updated: 2026-08-08
source: inbox 2026-08-08
---

2026-08-08 docs 整理完成,三个提交 0f56fc6、3dc1129、c4be572:

① docs/design 下 10 个文档由 kebab-case 重命名为 snake_case:deep_parallel_dev / frontend_phase3 / r030_process_decoupling / r059_mobile_agent_communication / app_icon / interaction_modes / m2_sqlite_store / subagent_management / harness_m1 / memory_system。

② docs/reference 四个纯上游文档(todo / tui-package / schema-changelog / instructions)移入 docs/reference/opencode-archive/(含 specs-v2 子目录);reference/README.md 声明为 opencode 上游规格快照,非本仓契约。

③ frontend_phase3 §八 的 R-050 早期 POC 设计整体移入 deep_parallel_dev.md 附录(历史);R-050 方案唯一承载于 deep_parallel_dev.md。

④ 归档 goals 旧 G-003(工具面补全)重编号为 G-005;当前活跃 G-003 = 深并行。

⑤ 归档文件(requirements-archive/goals-archive)中的旧文件名引用保留为历史快照,未更新。

⑥ .kanzei/project/architecture/README.md 的索引更新被 ruleset 拒绝(该资源 policy-managed 且无专用工具,参见 M-005),仍缺 direction_taste/memory_system/deep_parallel_dev 等条目,待用户手动更新。
