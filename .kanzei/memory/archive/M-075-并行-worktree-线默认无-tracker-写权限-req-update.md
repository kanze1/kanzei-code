---
id: M-075
scope: project
category: fact
title: 并行 worktree 线默认无 tracker 写权限,req update/close 被拒是设计行为,须主根线或用户执行
description: 并行 worktree 线 req update/close/defect 被拒时报「当前分支线未开启 tracker 写入」时必读:这是设计行为,须主根线或用户执行条目状态管理
status: deprecated
created: 2026-08-16
updated: 2026-08-18
source: inbox 2026-08-15
refs: R-263
---

kanzei-app/src/permission_tests.rs:108-141 验证:并行 worktree 线默认 block_tracker_writes(commands/run.rs:277-278:worktree_path.is_some() && !tracker_writes_enabled)。R-263 关闭被拦时实锤:req update/close 报「当前分支线未开启 tracker 写入;读取仍可用」,不是故障。条目的 req update/close/defect 登记须由主根线(claim 列表里的「默认」线)或用户执行;并行线只交付代码提交,收活时引擎合并到 dev。processes 测试 r247 佐证设计意图。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-075-并行-worktree-线默认无-tracker-写权限-req-update.md)
