# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 本轮检查仍无解锁：R-030 的 Claude runtime 契约未落地，R-050/R-059 doing 项继续阻塞；R-064 及后续需求受 WIP 与列表顺序限制不能启动。活跃目标未达成，本轮无安全独立步骤，停止空转。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: 已推进 R-064 并完成：桌面端设置页现在支持逐个或一键测试全部 Provider 连通性，显示测试进度与可用数量；复用既有 provider_test 后端命令，node --check、cargo check -p kanzei-app、git diff --check 均通过。R-030/R-037 的 Claude runtime 前置契约仍未落地，R-050/R-059 继续阻塞。
