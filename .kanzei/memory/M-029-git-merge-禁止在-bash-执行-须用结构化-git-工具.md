---
id: M-029
scope: project
category: fact
title: git merge 禁止在 bash 执行，须用结构化 git 工具
description: 处理合并或其他 Git 分支/索引变更时必读：不要在 bash 运行 git merge；改用结构化 git 工具，并按要求显式 stage、检查 staged_hash/diff，再用该 hash commit。
status: active
created: 2026-08-09
updated: 2026-08-09
source: inbox 2026-08-09
---

环境契约：bash 执行 `git merge` 会被拦截，Git mutation 必须使用结构化 `git` 工具；应使用 `git stage` 指定明确文件，审阅 staged_hash/diff 后，再用该 hash 执行 `git commit`。原始错误：`git merge` is blocked in bash: git mutations must use the structured `git` tool. Use `git stage` with explicit files, review its staged_hash/diff, then `git commit` with that hash. Branch/index mutations not covered by that tool require th
[fp:bash|`git merge` is blocked in bash: git mutations must use the structured `git` tool]
