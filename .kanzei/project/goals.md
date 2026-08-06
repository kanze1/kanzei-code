# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 完成 R-058：新增 docs/design/subagent-management.md，形成 agent/任务/策略/审计四层子代理管理方案，明确硬门禁、实施顺序和验证路径，并与 R-049 静态风险报告对齐。cargo test -p kanzei-harness -p kanzei-core -p kanzei-tools 通过。G-002 仍受 R-030/R-037（Claude 负责）阻塞。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: R-058 已完成：子代理管理扩展方案与验证文档已提交，现有只读子代理、活动面板、轨迹回放作为基线；cargo test -p kanzei-harness -p kanzei-core -p kanzei-tools 通过。R-036/R-033 已完成，G-002 仍等待 Claude 落地 R-030/R-037。
