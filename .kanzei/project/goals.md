# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 本轮复核无解除条件变化：R-030 仍等待 Claude 提供 process_id/session_id runtime 契约；R-050/R-059 两个 doing 项仍被该契约或认证部署方案阻塞。依 WIP 上限与需求顺序，R-064/R-075 暂不启动；当前无安全独立代码步骤，避免空转。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: 最新调度结论未变化：R-030/R-037 的 Claude 前置实现仍缺失，G-002 的前端对齐无法继续；R-064 按顺序等待现有 doing 槽释放后再取活。
