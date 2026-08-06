# Requirements Archive

## R-001 harness 双模式 dev/research profile [done]

## R-002 Tauri 桌面端(类 VSCode 布局) [dropped]

## R-004 本地模型跑并行子代理(M4) [done]

## R-005 桌面端基础件:多项目管理/运行状态/设置页 [done]

## R-006 桌面端 UI 美化(用户反馈:现在有点丑) [done]

## R-008 自举:用 kanzei 开发 kanzei [dropped]
- 备注: 自举是持续工作方式而非可完结需求,由长期目标 G-001 承载;并入后关闭

## R-009 对话历史记录持久化 [dropped]
- 备注: 与 R-003 是同一事件日志/投影,并入 R-003 一并交付;关联缺陷 D-008 已修复

## R-011 Agent 通用工具能力对齐 Codex 与 Claude Code [dropped]
- 备注: 伞形需求,已由 2026-08-06 工具审计具体化:检索=R-026、子代理=R-012、todo=R-028、question=R-029、websearch=R-023、多模态=R-014/R-024;不再单独追踪

## R-012 将子Agent调度能力开放给主Agent [done]
- 实现: task 工具 + 只读 explore 子代理(read/glob/grep),同轮多 task 并行,fast/primary 双档位,E2E 验证通过

## R-015 对话全状态显示(diff/终端块/轮次/思考块/markdown/git 状态) [done]

## R-017 终端命令执行不弹出黑色控制台窗口 [done]

## R-019 支持设定目标并持久化长期工作 [done]

## R-020 编辑 diff 默认收纳并显示改变量摘要 [done]

## R-021 上下文自动压缩:超阈值自动总结并延续对话,压缩不丢数据 [done]

## R-022 LLM 请求瞬断自动重试(流未建立时退避重试) [done]

## R-026 glob/grep 检索工具(ripgrep 内核,head-limit 早停) [done]

## R-003 SQLite 事件溯源 + steer/queue 调度(M2) [done]
- 范围: SQLite 事件溯源(state.db)、steer/queue 双投递、运行中 queue drain、事件恢复消息历史;对话历史持久化(原 R-009)一并交付
- 已完成: SessionStore + 迁移、prompt admitted/promoted 事件、会话状态生命周期(running/idle/failed)、CLI/桌面端接入、事件恢复第一阶段、steer 输入入口与优先调度、运行中队列调度
- 下一步: 运行中 queue admission/drain 收尾,使 pending 输入在当前任务结束后自动提升执行
- 文档: docs/design/m2-sqlite-store.md
- refs: R-013 D-010
- 最新提交: 91d3f2b
- 进展: 已完成 queue/steer drain 的关键修复：promote_next_input 逐条 FIFO 提升 steer，避免后续 steer 丢失；新增连续 steer→queue 回归测试。cargo test --workspace 全部通过。剩余运行中 admission/drain 竞态与端到端覆盖。
- 完成说明: 已完成 SQLite 事件溯源、steer/queue admission 与桌面端运行中 drain；修复多个 steer 逐条 FIFO 提升问题，并通过 lifecycle 锁消除 queue admission 与 drain 收尾竞态。相关回归测试、M2 调度文档已更新，cargo test --workspace 全部通过。
- 验收: 运行结束边界提交的输入不会因 worker 在最后检查后直接退出而遗留 pending；steer/queue 按既定优先级与 FIFO 逐条提升。
