---
id: M-029
scope: project
category: fact
title: 所有 git mutation 在 bash 都被拦截,必须走结构化 git 工具
description: 处理任何 Git 分支/索引变更(merge/restore/rebase/add/commit/reset)在 bash 报 "is blocked in bash: git mutations must use the structured git tool" 时必读:不要换别的 git 子命令重试,改用结构化 git 工具——显式 stage 指定文件、核对 staged_hash/diff,再用该 hash commit;快进合并走 git merge_ff。
status: active
created: 2026-08-09
updated: 2026-08-12
source: inbox 2026-08-09;2026-08-12 合并 M-046/M-049(restore/rebase 同类复发)
supersedes: M-046 M-049
---

环境契约:bash 里执行 Git mutation 一律被拦截,**不限于 `git merge`**——实测已复发的还有 `git restore`、`git rebase`,同一条规则覆盖 add/commit/reset 等所有会改分支或索引的命令。只读查询(`git log`、`git status`、`git diff`)不受此限。

正确做法:用结构化 `git` 工具——`git stage` 指定明确文件,审阅它返回的 staged_hash/diff,再用该 hash 执行 `git commit`;快进合并用 `git merge_ff`(from/into)。该工具没覆盖的分支/索引操作需要用户授权。

原始错误(复发指纹,勿删):
[fp:bash|`git merge` is blocked in bash: git mutations must use the structured `git` tool]
[fp:bash|`git restore` is blocked in bash: Git mutations must use the structured `git` tool]
[fp:bash|`git rebase` is blocked in bash: git mutations must use the structured `git` tool]

关联:M-019(shell 整文件重写同样被拦,须用 edit 定点改)。
