# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 完成 R-025：设置页可查看当前项目已记住的放行权限规则（操作/资源/配置路径），确认后可删除单条规则；新增并注册 permission_rules_get/permission_rule_delete Tauri 命令，删除仅允许 allow 规则。node --check、git diff --check、cargo test -p kanzei-harness -p kanzei-app 均通过。G-002 仍受 R-030/R-037（Claude 负责）阻塞。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: R-025 已完成：设置页可查看/删除当前项目已记住的放行规则；node --check、git diff --check、cargo test -p kanzei-harness -p kanzei-app 通过。R-036/R-033 已完成，G-002 仍等待 Claude 落地 R-030/R-037。
