# Goals

## G-001 把 kanzei 打磨成日常主力开发工具:清空 open 缺陷,推进 R-003 持久化与 R-012 子代理 [active]
- 进展: R-003 运行中 queue admission/drain 已落地并修正首个任务启动竞态：活动项目在 spawn 前登记；同项目输入持久化 pending、按 FIFO 自动提升执行，跨项目明确拒绝。D-017 已修复。cargo test --workspace 全部通过，仅保留既有 current_run unused warning。下一具体步：补 steer 前端入口，随后推进事件恢复消息历史。
