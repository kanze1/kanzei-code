# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: 完成 R-040 快捷键基础阶段：Ctrl/Cmd+K 聚焦输入框、Ctrl/Cmd+Shift+N 新对话、Ctrl/Cmd+Shift+C 停止，复用既有 UI 流程；切进程快捷键待 R-030/R-037 多进程模型落地后补齐。node --check、git diff --check、cargo test --workspace 均通过。此前 R-035 已在需求归档中完成，旧进展文字已纠正。
