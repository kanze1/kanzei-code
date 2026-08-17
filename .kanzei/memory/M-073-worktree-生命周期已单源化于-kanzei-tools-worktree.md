---
id: M-073
scope: project
category: fact
title: worktree 生命周期已单源化于 kanzei-tools::worktree(R-207):桌面 processes.rs 留转发壳,CLI kz worktree 同一实现
description: 改 worktree 建线/回执/回滚/合并/状态逻辑时必读:只动 tools/worktree.rs,processes.rs 壳无需动
status: candidate
created: 2026-08-16
updated: 2026-08-16
source: inbox 2026-08-14
refs: R-207
---

2026-08-16 R-207 落地。crates/kanzei-tools/src/worktree.rs 是 worktree 生命周期唯一实现(建线/回执/回滚/合并预检/状态/冲突解析),桌面 processes.rs 只剩转发壳+AppState 交互(bound_thread_for_worktree/with_idle_bound_process/acquire_project_write_lease/reclaim/discard_worktree_and_unregister/merge_worktree_and_release),CLI kz worktree create/merge-preview 直接调同一实现。WorktreeInfo/WorktreeReceipt 类型在 tools;state.rs re-export WorktreeInfo。改 worktree 逻辑只改 tools worktree.rs,processes.rs 的壳无需动。CLI 真机建树冒烟受 bash 禁 git 突变限制(临时仓无法用结构化 git 工具),验证走命令分发+kernel 测试。
