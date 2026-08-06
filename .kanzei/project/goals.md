# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: R-031 已完成归档;本轮完成 R-039:权限弹窗显示当前/总数与待处理预览,连跑支持暂停/恢复、本轮后停止、1-100 上限持久化,停止/错误会取消旧 timer 防止自动重启。cargo test --workspace、cargo check -p kanzei-app、node --check 全部通过。下一步在无 doing 需求后推进 R-035 diff 查看器升级或 R-041 错误分级。
