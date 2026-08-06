# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 本轮继续推进 R-050：会话任务自然完成/失败时清理对应 current_run，并处理 spawn 快速结束与句柄安装竞态，避免已结束 JoinHandle 残留；回归测试通过，仍保留全局运行闸门。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: 前端/后端对齐继续推进：R-050 已完成 sessionId 事件路由、按 session_id 历史隔离、项目停止边界、运行时容器、权限询问路由及 current_run 生命周期收尾；本轮 5 项 app 测试通过，下一步仍需验证真实多会话并行运行闸门。R-064 已完成，R-059 继续排队推进。
