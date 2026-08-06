# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: R-032 队列可视化、R-043 项目初始化/切换管理、R-044 右侧活动面板稳定开关已完成;本轮推进 R-031 实时阶段:子代理内部工具 start/end 结构化转发,task 块可展开子工具状态/预览/display,活动面板跨轮保留。cargo test --workspace、cargo check -p kanzei-app、node --check 全部通过。R-031 剩余历史轨迹持久化回放,下一步继续完成该收尾。
