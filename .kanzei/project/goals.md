# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: 继续完成 R-032:新增队列查询/单条撤销 Tauri command,前端新增排队输入面板、delivery 标识与单条撤销,发送/完成/停止/项目切换/启动自动刷新;cargo test --workspace、cargo check -p kanzei-app、node --check 全部通过。下一步按 P0 推进 R-044 右侧活动面板保持稳定。
