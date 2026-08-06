# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: R-059 阶段 A 的技术无关内存 broker/订阅 POC 已落地到 kanzei-core：AgentMessage 幂等去重、通知 sequence/cursor replay、limit 不丢事件、终态补发，新增 4 项测试；core 17 项、app 1 项通过。未接入网络、桌面事件或远程控制。cargo fmt 全量检查暴露仓库既存无关格式差异，已恢复无关文件，仅保留本次改动。下一步是补充内存订阅边界测试并评估 R-030/R-050 适配点。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: R-063 已完成：需求与缺陷全部清空时自动推进停止逻辑已落地并通过 node --check、git diff --check、cargo test -p kanzei-app。R-036/R-033 已完成，G-002 仍等待 Claude 落地 R-030/R-037。
