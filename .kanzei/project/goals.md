# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 本轮再次检查目标与队列：活跃目标未达成；R-030 仍缺 Claude 的 process_id/session_id 契约，R-050/R-059 仍是被依赖契约或认证方案阻塞的 doing 项。R-064 是下一个候选但不能突破 WIP 上限提前启动；无安全独立步骤，继续等待解除条件。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: 最新调度结论未变化：R-030/R-037 的 Claude 前置实现仍缺失，G-002 的前端对齐无法继续；R-064 按顺序等待现有 doing 槽释放后再取活。
