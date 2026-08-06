# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 完成 R-045：新增工作区视图和 workspace_snapshot 命令，统一展示所有项目的运行状态、当前对话、排队输入、更新时间与最近活动；项目卡片可直接切换并复用既有刷新链路。node --check、git diff --check、cargo test -p kanzei-app 通过。G-002 仍受 R-030/R-037（Claude 负责）阻塞。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: R-045 已完成：工作区视图与 workspace_snapshot 已落地，覆盖项目状态、当前对话、排队输入和最近活动；node --check、git diff --check、cargo test -p kanzei-app 通过。R-036/R-033 已完成，G-002 仍等待 Claude 落地 R-030/R-037。
