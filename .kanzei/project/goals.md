# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: R-050 当前可独立推进的只读阶段已完成：设计文档、SessionStore 跨 session 事件/队列隔离回归测试、scripts/r050-poc-check.ps1 验收入口均已提交并通过。剩余真实双线程 runner、权限询问/活动轨迹路由、worktree 冲突合并和恢复均依赖 R-030 的 process_id/session_id 契约；R-030 明确归属 Claude，当前无安全且有意义的独立实现步骤，等待前置契约。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: R-063 已完成：需求与缺陷全部清空时自动推进停止逻辑已落地并通过 node --check、git diff --check、cargo test -p kanzei-app。R-036/R-033 已完成，G-002 仍等待 Claude 落地 R-030/R-037。
