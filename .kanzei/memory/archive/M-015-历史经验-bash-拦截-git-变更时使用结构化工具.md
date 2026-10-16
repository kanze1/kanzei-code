---
id: M-015
scope: project
category: fact
title: 历史经验：bash 拦截 Git 变更时使用结构化工具
description: 处理 bash git 拦截时必读:改用结构化工具显式 stage+核对 hash;保留所有 [fp:bash|...]指纹
status: deprecated
created: 2026-08-08
updated: 2026-10-07
source: inbox 2026-08-08
area: kanzei-tools/git kanzei-tools/bash
---

所有 git mutation 在 bash 都被拦截，必须走结构化 git 工具 — 处理任何 Git 分支/索引变更(merge/restore/rebase/add/commit/reset)在 bash 报 "is blocked in bash: git mutations must use the structured git tool" 时必读:不要换别的 git 子命令重试,改用结构化 git 工具——显式 stage 指定文件、核对 staged_hash/diff,再用该 hash commit;快进合并走 git merge_ff。

**适用场景 + 操作步骤**: 
- **适用场景**: bash 执行任何 git 修改时收到"is blocked in bash"拦截
- **操作步骤**:  
  1. 使用 [git] tool 的 stage 子命令,显式指定 files (如 `git stage path/to/FILE`)
  2. 用 [git] tool 的 diff 或 staged_hash 查看 staging 状态
  3. 执行 commit: `git commit <staged_hash>` 
  4. 快进合并: `git merge_ff from into`
- **边界与例外**: 禁止在 bash 中执行任何 form of git mutation(Set-Content/git add等整写或修改都会触发拦截)

[fp:bash|`git merge` is blocked in bash: Git mutations must use the structured tool][fp:bash|`git commit` is blocked in bash: Git mutations must use the structured tool]

(stale: 2026-10-07 用户授权项目记忆清理；源码复核：重复 M-029 的结构化 Git 变更契约；保留 M-029 的统一来源与适用条件。)
