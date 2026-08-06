# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: R-066 已完成。当前按列表处理 R-030/R-050：R-030 明确归属 Claude，G-002 等待其落地；R-050 已推进前置设计阶段，在 docs/design/frontend-phase3.md 补齐线程—项目—session—worktree 关系、状态机、锁顺序、崩溃恢复和双线程只读 POC 验收矩阵。完整运行时实现仍等待 R-030 契约。git diff --check、node --check、cargo test -p kanzei-app 通过。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: R-063 已完成：需求与缺陷全部清空时自动推进停止逻辑已落地并通过 node --check、git diff --check、cargo test -p kanzei-app。R-036/R-033 已完成，G-002 仍等待 Claude 落地 R-030/R-037。
