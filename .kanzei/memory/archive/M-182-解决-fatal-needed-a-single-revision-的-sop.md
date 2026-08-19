---
id: M-182
scope: project
category: sop
title: 解决 "fatal: Needed a single revision" 的 SOP
description: 处理 git merge "fatal: Needed a single revision"失败时必读：如何生成/确认唯一的revision
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

步骤 1：确认当前分支存在可合并的 revision（查看 `git log` 和 `git branch --list`, 判断依据：缺少 single revision 将导致 merge abort）、是否已在本地或远程有对应 commit。
步骤 2：若需强制生成 single revision，使用 `git update-ref -d <tag> origin/<branch>` 删除旧 tag/commit（判断依据：避免重复尝试合并已存在的分支）。
边界与例外：- 仅适用于需要统一 single revision 的场景；- 当本地/远程均无 target commit 时不可用；- 若分支在推进中，先 `git push` 同步再此步。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-182-解决-fatal-needed-a-single-revision-的-sop.md)
