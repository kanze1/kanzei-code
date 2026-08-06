# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 完成 R-018：运行成功、失败、手动停止和权限拒绝停止均有 toast 与短提示音；失焦时更新窗口标题，已授权时发送系统通知；通知或音频不可用只记录警告，不影响对话结果。node --check、git diff --check、cargo test -p kanzei-app 通过。G-002 仍受 R-030/R-037（Claude 负责）阻塞。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: R-018 已完成：成功/失败/停止均有 toast、短提示音和失焦标题提醒，已授权时发送系统通知；node --check、git diff --check、cargo test -p kanzei-app 通过。R-036/R-033 已完成，G-002 仍等待 Claude 落地 R-030/R-037。
