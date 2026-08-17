---
id: M-013
scope: project
category: fact
title: git commit 报 exit code 1 + "Changes not staged"表示没有暂存内容 — 处理 bash/git commit 失败必读：先 check 缺失的 git add
description: Git commit 失败(exit code 1、"Changes not staged for commit")时必读:先检查同批前置 git add — 不能断言用户忘记,只记症状如路径未匹配。
status: active
created: 2026-08-08
updated: 2026-08-17
source: inbox note 2026-08-08 [fp:bash|exit code:]
---

git commit 报 exit code 1 + "Changes not staged..."表示没有暂存内容 — bash/git commit 失败时必读：逐层回溯 check git status → find missing staging entries → run corresponding git add(s)；不能仅凭症状断言忘记 do add。跨轮复发机制（第 2 次失败建 candidate(未验证);第3+且带修复成功证据时用 memory_add+memory_promote，附 episode_id）: [fp:bash|行动:git commit 失败(exit code、"Changes not staged for commit")时必读：先检查同批前置 git add]
