# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 完成 R-061：新增 APP 图标设计规范并验收现有多平台图标资产；已通过图标尺寸检查、cargo test -p kanzei-app、git diff --check。当前 backlog 剩余需求均为 R-030/R-050/R-059，分别受 Claude、需确认的高风险架构方案和远期移动端范围约束。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: R-061 已完成：APP 图标设计规范与现有多平台资产验收已完成；图标尺寸检查、cargo test -p kanzei-app、git diff --check 通过。当前可做需求已清空，R-030/R-050/R-059 仍分别受 Claude/高风险架构/远期移动端范围约束。
