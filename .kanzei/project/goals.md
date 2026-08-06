# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 按新增的阻塞跳过调度规则，已跳过 R-030/R-050 的阻塞实现，取活 R-059。R-059 已完成移动端子代理通信与通知的第一步设计：新增 docs/design/r059-mobile-agent-communication.md，定义消息/通知模型、幂等与断线恢复、设备授权、权限门禁、升级回滚及 A/B/C 分阶段验收；不选择技术栈、不开放公网远程控制。r050-poc-check.ps1 验证通过（core 13 项、app 1 项、前端语法）。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: R-063 已完成：需求与缺陷全部清空时自动推进停止逻辑已落地并通过 node --check、git diff --check、cargo test -p kanzei-app。R-036/R-033 已完成，G-002 仍等待 Claude 落地 R-030/R-037。
