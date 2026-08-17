# Defects

## D-435 询问弹窗弹出位置偏移问题 [fixing] (medium)
- 原始描述: 询问弹窗弹出的位置不对。
- 复现: 待澄清: 请说明在什么场景下出现？期望的弹出中心坐标/锚点位置是什么？是否受其他元素影响？
- 标签: 前端
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-435
- 停车: 等待用户补充 D-435 的复现入口、实际/期望坐标或锚点以及是否受窗口缩放/侧栏影响；信息齐全后恢复并跑 UI 运行时冒烟。
- 进展: 已读码核对：crates/kanzei-app/ui/07-events.js:641-717 负责 pumpAsk/show/hide 与 #ask-overlay；crates/kanzei-app/ui/style.css:1054-1065 将询问卡片设为 position:fixed、右下停靠（inset:auto 0 0 0、padding:0 22px 18px）。现有复现只有“位置不对”，没有场景、期望中心坐标/锚点或受影响元素，不能安全判断应居中还是右下停靠；未修改 UI。
- observed_head: 3950c0348331956fda32a18d0789ce52d3d30eee
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786959925052

## D-479 轮末 memory manager 产生 candidate 但未完成晋升与 inbox 销账 [open] (medium)
- 复现: 在隔离项目执行真实 `cargo run -p kanzei -- run --new --project-root <isolated-project> --prompt-file <memory_note prompt>`；research agent 成功调用 memory_note，轮末 manager 写入 `M-001` candidate，但 `.kanzei/memory/inbox.checkpoint.json` 为 `status=failed`、`success_notes=0`、`pending_after=1`；再次 follow-up 后仍为 pending。当前项目首次运行还触发 managed-files 回滚，不能作为成功链路。
- 影响: R-289 要求的 memory_note→manager 晋升→memory_search 回读无法以真实运行时证据闭环；candidate 未 active，inbox 未逐条销账，研究记忆不能确认进入可检索状态。
- 来源: self-found：R-289 真实运行时验收；失败记录 T-1786922726176，确定性工具回归 T-1786922726177。
- 标签: 核心
- 验收: 真实 research/CLI 运行中，memory_note 投递的候选由轮末 manager 使用真实 episode provenance 晋升为 active，逐条销账 inbox，并由 memory_search 回读同一条目；不得用 candidate 文件或仅单测替代。
- refs: R-289
- 优先级: P1
