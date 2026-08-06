# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 本轮推进 R-050：运行元数据、turn/text/reasoning/tool/task/step 事件、权限询问及权限状态反馈统一携带 sessionId，PendingAsk 保存所属会话；cargo test -p kanzei-app、node --check、git diff --check 通过。下一步继续按线程隔离运行句柄与历史，尚未开启并行写入/worktree。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: 前端/后端对齐继续推进：R-064 已完成 Provider 全量连通性检查；本轮推进 R-050，桌面端运行事件和权限反馈补充 sessionId 路由标识，为后续多线程隔离铺路。R-050 仍在实现中，R-059 继续排队推进。
