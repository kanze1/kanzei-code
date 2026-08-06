# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 本轮继续推进 R-050：stop_run 现在按目标项目校验运行归属，仅清理对应 PendingAsk 和 session 队列，非目标项目不会误 abort；运行自然结束/失败会清理 running_project。测试通过，仍保留全局运行闸门，未开启并行写入/worktree。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: 前端/后端对齐继续推进：R-050 已完成 sessionId 事件路由、按 session_id 历史隔离及按项目停止边界基础；本轮回归通过，下一步仍需将运行句柄、权限队列进一步收进线程容器。R-064 已完成，R-059 继续排队推进。
