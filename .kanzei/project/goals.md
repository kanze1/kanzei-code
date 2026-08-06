# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 本轮调度复核：R-030 受 Claude 的 process_id/session_id 前置契约阻塞；R-050、R-059 的 doing 工作均依赖该契约或认证部署方案。按 WIP 上限不能启动 R-064；R-075 继续保持 todo，待可执行槽释放后按需求顺序推进。当前无安全的独立代码步骤，避免空转。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: 调度复核：R-030 仍由 Claude 负责且缺少 process_id/session_id runtime 契约，R-037 也未落地；因此 G-002 当前无法继续推进，R-064 需等待 doing 槽释放后按列表顺序取活。
