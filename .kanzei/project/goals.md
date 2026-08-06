# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 本轮完成 R-064：设置页增加 Provider 全量连通性检查与进度反馈，复用既有探测命令并防止重复触发；代码与项目追踪文档已提交（075fdc2）。后续 R-065 虽是队列中下一项，但当前 R-050/R-059 仍为 doing 且受 R-030 runtime 契约阻塞，受 WIP 上限暂不取活。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: 已推进 R-064 并完成：桌面端设置页现在支持逐个或一键测试全部 Provider 连通性，显示测试进度与可用数量；复用既有 provider_test 后端命令，node --check、cargo check -p kanzei-app、git diff --check 均通过。R-030/R-037 的 Claude runtime 前置契约仍未落地，R-050/R-059 继续阻塞。
