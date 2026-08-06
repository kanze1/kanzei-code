# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 本轮继续推进 R-050：统一 answer_ask 的会话容器取问路径，新增权限询问跨容器隔离测试；current_run 与 asks 均按 session_id 管理，stop_run 按目标容器清理。cargo test -p kanzei-app（5 项）通过，仍保留全局运行闸门。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: 前端/后端对齐继续推进：R-050 已完成 sessionId 事件路由、按 session_id 历史隔离、项目停止边界、运行时容器及权限询问路由测试；下一步仍需验证多线程运行句柄生命周期。R-064 已完成，R-059 继续排队推进。
