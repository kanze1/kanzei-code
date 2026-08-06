# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 本轮检查仍未发现新的 ProcessHandle、process_id/session_id、真实双线程 runner 或权限/活动路由实现。R-050 的设计、SessionStore 隔离回归与 POC 验收脚本已完成，剩余实现继续等待 R-030（Claude）前置契约；G-002 继续等待 R-030/R-037。按列表顺序和阻塞规则，本轮无安全且有意义的独立步骤。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: R-063 已完成：需求与缺陷全部清空时自动推进停止逻辑已落地并通过 node --check、git diff --check、cargo test -p kanzei-app。R-036/R-033 已完成，G-002 仍等待 Claude 落地 R-030/R-037。
