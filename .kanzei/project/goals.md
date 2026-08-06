# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)
- 进展: 完成 R-046 首个统一连续推进闭环：新增过夜模式偏好持久化（重启不静默运行）、连续推进上限/当前轮次/等待状态展示，完成/拒绝/停止原因可见；保留暂停、本轮后停和安全边界。node --check、git diff --check、cargo test -p kanzei-app 通过。G-002 仍受 R-030/R-037（Claude 负责）阻塞。

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: R-046 已完成首个统一连续推进闭环：过夜模式偏好、连续推进上限/当前轮次/等待状态与停止原因已落地；node --check、git diff --check、cargo test -p kanzei-app 通过。R-036/R-033 已完成，G-002 仍等待 Claude 落地 R-030/R-037。
