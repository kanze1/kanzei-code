# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: R-059 阶段 A 再推进：订阅内 event_id 去重已落地，重复事件不重复投递且 cursor 推进到最新 sequence；core 25 项、app 1 项、r050-poc-check.ps1 与 git diff --check 全部通过。已新增 R-075 网络错误有限重试机制需求，暂等待队列顺序与执行槽。跨重启去重、移动端通信仍等待 runtime 契约与认证方案。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: R-063 已完成：需求与缺陷全部清空时自动推进停止逻辑已落地并通过 node --check、git diff --check、cargo test -p kanzei-app。R-036/R-033 已完成，G-002 仍等待 Claude 落地 R-030/R-037。
