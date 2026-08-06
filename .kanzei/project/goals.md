# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 完成 R-051：需求列表支持复杂度筛选/排序，行内显示小/中/大或未评估，详情可直接设置并保存复杂度，兼容旧条目。node --check、git diff --check、cargo test -p kanzei-app -p kanzei-tools 通过。G-002 仍受 R-030/R-037（Claude 负责）阻塞。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: R-051 已完成：复杂度字段的筛选、排序、列表显示和详情编辑已落地；node --check、git diff --check、cargo test -p kanzei-app -p kanzei-tools 通过。R-036/R-033 已完成，G-002 仍等待 Claude 落地 R-030/R-037。
