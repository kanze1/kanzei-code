# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 本轮继续推进 R-050：AppState 内存对话历史改为按 session_id 隔离，conversation_clear/get、运行恢复、自动压缩和持久化投影均使用对应会话；仍保留全局运行闸门，未开启并行写入/worktree。cargo test -p kanzei-app、node --check、git diff --check 通过。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: 前端/后端对齐继续推进：R-050 已完成 sessionId 事件路由和按 session_id 的内存历史隔离基础；本轮测试通过，下一步仍需隔离运行句柄、权限/队列和停止边界。R-064 已完成，R-059 继续排队推进。
