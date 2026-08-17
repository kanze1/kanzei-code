---
id: M-090
scope: project
category: fact
title: git merge/commit in bash 被阻断:须改专用工具执行(有明确文件审查后)
description: 处理 bash git merge commit 被工具契约阻断时必读:bash/git 存在硬性mutation限制 — 改专用工具(stage→review hash→ff commit)
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

处理 bash git merge commit 被工具契约阻断时必读:bash/git存在硬性rule — Git mutation(bash git stage/merge/commit)必须在专用结构化工具中执行，使用 explicit files 并审查 staged_hash/diff 后再 commit。适用场景:bash 下尝试执行 git merge/git commit 操作;操作步骤:(1)bash 执行 git merge 或 git commit 前预判失败 — (2)改用专用工具(stage→review hash)。
