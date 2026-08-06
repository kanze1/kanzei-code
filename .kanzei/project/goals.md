# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 完成 R-055 首个独立管理闭环：新增需求/缺陷 Activity 视图，整页支持列表、详情、状态编辑、筛选和需求拖拽排序；进入该视图时侧栏只保留摘要。node --check、git diff --check、cargo test -p kanzei-app -p kanzei-tools 通过。G-002 仍受 R-030/R-037（Claude 负责）阻塞。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: R-055 已完成首个前端管理闭环：需求/缺陷独立 Activity 视图、详情/编辑/筛选/拖拽排序已落地；node --check、git diff --check、cargo test -p kanzei-app -p kanzei-tools 通过。R-036/R-033 已完成，G-002 仍等待 Claude 落地 R-030/R-037。
