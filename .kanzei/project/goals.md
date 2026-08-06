# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: R-036 已完成：新增 dev-pair 结伴人格，桌面端模式选择器默认结伴开发并支持自主推进/research，run_prompt 已传递显式 agent，连跑仅在自主推进模式可用。R-033 已完成：消息区智能滚动跟随/回到最新、消息与工具一键复制、对话内搜索及上下匹配跳转。下一步按 priority 推进 R-030/R-037（由 Claude 落地）后的剩余前端对齐项。
