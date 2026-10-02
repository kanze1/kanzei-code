---
id: M-083
scope: project
category: sop
title: Git mutations bash blocked -> use structured tool
description: 处理 bash git 命令被结构化工具拦截时必读:必须使用结构化 git 工具而非bash — 用git stage/git_merge_ff替代
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

`git merge` in bash blocked: Git mutations must use structured `git` tool. Use `git stage` with explicit files, review its staged_hash/diff, then `git commit` with that hash。Fast-forward merges go through `git merge_ff`(from/into; no conflicts). Always validate staged content before committing via structured flow — never bypass with bash for mutations beyond non-conflict fast-forwards.
判据: 当bash返回"is blocked in bash"或类似拦截信息时,立即切换到结构化工具流程 — 用git stage管理显式文件变更,通过review staged_hash/diff确认内容后再执行git commit with hash。避免直接bash命令进行任何Git突变操作。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-083-git-mutations-bash-blocked-use-structure.md)
