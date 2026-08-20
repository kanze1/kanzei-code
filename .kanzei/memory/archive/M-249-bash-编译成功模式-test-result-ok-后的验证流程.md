---
id: M-249
scope: project
category: fact
title: bash 编译成功模式：test result ok 后的验证流程
description: 处理 cargo test 运行时错误或编译成功后必读：确认测试通过并记录结果
status: deprecated
created: 2026-08-18
updated: 2026-08-20
source: memory-manager
refs: R-070
superseded_by: M-258
---

错误原文: exit code: 1 Diff in \\?\C:\Users\kanzei\Documents\.kanzei-worktree-kanzei-code.line-1787020530803-1\crates\kanzei-memory\src\memory\store.rs:3177 
成功模式：exit code: 0 running 38 tests ... test result: ok
复发检测键：[fp:bash|assert_eq!(report.deprecated, low_value_ids);]
涉及目标: cargo

本轮轮次已落库:episode_id=818
