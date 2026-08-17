# Defects

## D-480 memory-manager 退役链路未暴露 memory_stale，R-216 请求被错误新增为候选记忆 [open] (medium)
- 复现: 向项目 memory inbox 投递 R-216 验收③请求，要求 memory-manager 对 project/M-037 执行 memory_stale；连续运行真实 `kz run`，manager 报 `memory inbox: 1 -> 1 pending`/failed，并产生 M-150、M-151 两个重复 candidate，而 M-037 仍 candidate。尝试写 M-037 归档时被 managed-file guard 回滚。
- 影响: 六条交付状态记忆无法完成逐条处置；退役意图被污染成新候选记忆，inbox 无法销账，可能继续增加重复记忆并使 R-216 无法关闭。
- 来源: self-found：R-216 验收③真实 manager 运行复现。
- 标签: 核心
- 进展: 已保留真实失败证据：3 次 `kz run` 均未使 inbox 从 1 降为 0；当前不再重复运行，下一步检查 memory-manager 工具装配、profile 权限和 managed-file 写日志接线，修复后再做一次隔离回归。
- refs: R-216
- 优先级: P1
