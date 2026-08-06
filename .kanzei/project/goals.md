# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 继续推进 R-066：完成 D-036。独立需求/缺陷页面现在按类型隔离筛选状态；需求状态选项为 todo/doing/done/dropped，缺陷状态选项为 open/fixing/fixed/wontfix；缺陷页隐藏无效优先级筛选；需求列表不再临时污染侧栏 reqFilters。node --check、git diff --check、cargo test -p kanzei-app 通过。D-032、D-034 仍待后续治理。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: R-063 已完成：需求与缺陷全部清空时自动推进停止逻辑已落地并通过 node --check、git diff --check、cargo test -p kanzei-app。R-036/R-033 已完成，G-002 仍等待 Claude 落地 R-030/R-037。
