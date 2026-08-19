---
id: M-115
scope: project
category: sop
title: bash git merge blocked必读：使用结构化替代方案
description: 处理 bash git merge blocked 时必读：使用结构化工具替代 bash，保留 [fp] 标记
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
subject: git merge blocked bash 判据
---

bash git merge blocked in bash: Git mutations must use structured tool。  
**Pitfall**: "git merge" 被 bash 拒绝 → 使用方式不符合结构化工具规范 → action：用 git stage + explicit files → review staged_hash/diff → git commit with that hash；fast-forward merges 用 git merge_ff(from/into; ...)  

错误原文：`git merge` is blocked in bash: Git mutations must use the structured `git` tool. Use `git stage` with explicit files, review its staged_hash/diff, then `git commit` with that hash. Fast-forward merges go through `git merge_ff`.
[fp:bash|is blocked in bash: Git mutations must use the structured tool. Use with explicit files]

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=true，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-115-bash-git-merge-blocked必读-使用结构化替代方案.md)
