---
id: M-091
scope: project
category: fact
title: git commit 失败(未暂存):须补 git add 后再执行
description: 处理 git commit 失败(exit code/Changes not staged)时必读:先补 git add
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

bash git commit 失败(exit code 1/"Changes not staged for commit")时必读：先检查同批前置 git add。适用场景:git add 后补 commit;操作步骤:(1)git status 确认有未暂存更改 — (2)执行 git add ./路径或具体文件再 commit。
