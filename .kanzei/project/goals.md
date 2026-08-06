# Goals

## G-001 把 kanzei 打磨成日常主力开发工具:清空 open 缺陷,推进 R-003 持久化与 R-012 子代理 [active]
- 进展: R-003 本轮已将取消能力接入桌面端停止入口：stop_run 传入当前 project_dir，按 session_id 批量取消 pending queue 输入，保留已 promoted 与其他会话；UI 在 kz:stopped 中显示取消数量。新增存储层隔离测试。cargo test --workspace 全部通过（仅既有 current_run unused warning）。下一具体步：补运行中 queue admission/drain，使 pending queue 能在当前任务结束后自动提升执行。
