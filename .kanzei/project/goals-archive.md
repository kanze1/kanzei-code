# Goals Archive

## G-003 工具面与体验补全 [achieved]
- 类型: 短期
- priority: P1
- 验收: R-016、R-028、R-029、R-023 全部 done;达成即 `goal update G-003 achieved`
- 说明: 发版自更新闭环 + agent 工具面(todo/question/websearch)
- 进展: R-016 已完成：kzapp 启动时检测 pending 并派生 helper 完成安全替换、失败回滚，release.ps1 已改为下次启动自动安装。G-003 验收项 R-016、R-023、R-028、R-029 均已完成，现申请达成短期目标。

## G-002 前端与后端能力对齐 [achieved]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: R-030/R-050/R-059 及本轮 R-065、R-067~R-082 均已完成实现和验证；前端进程、工作树、通知、测试归档、重试与文档能力已与后端契约对齐。
- 验证: cargo test --workspace；node --check crates/kanzei-app/ui/main.js；git diff --check。
