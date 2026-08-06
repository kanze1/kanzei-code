# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: 完成 R-048：底部状态栏新增稳定的“运行中/空闲”主状态，保留阶段详情与计时器，运行点不再持续闪烁；完成/停止/错误路径统一复位。node --check、git diff --check、cargo test --workspace 均通过。R-040 快捷键基础阶段已完成，切进程待 R-030/R-037。
