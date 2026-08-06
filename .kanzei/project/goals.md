# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 再次调度复核：活跃 doing 项 R-050、R-059 均明确阻塞于 R-030 的 process_id/session_id runtime 契约或认证部署方案；R-030 本身仍由 Claude 负责未落地。按 WIP 上限暂不能启动列表后续 R-064，当前无安全独立代码步骤，停止空转等待解除条件。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: 调度复核：R-030 仍由 Claude 负责且缺少 process_id/session_id runtime 契约，R-037 也未落地；因此 G-002 当前无法继续推进，R-064 需等待 doing 槽释放后按列表顺序取活。
