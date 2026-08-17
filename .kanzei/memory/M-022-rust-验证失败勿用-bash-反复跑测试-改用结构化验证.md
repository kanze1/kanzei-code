---
id: M-022
scope: project
category: sop
title: [git commit] bash 拦截时须 check staged add 再提交 — [fp:bash|> action: git commit 失败...]
description: 处理 git commit 被 bash 拦截须先 check add — [fp:bash|git commit 失败]必查前置 add/确认 staging 状态，否则强制 stage再 commit
status: active
created: 2026-08-09
updated: 2026-08-17
source: inbox:2026-08-09
---

git commit 失败后重试必查前置 git add 状态 — bash exit code:1/"Changes not staged for commit"或中文乱码拦截时，先检查同批前置 git add 是否执行/确认 staged_hash/diff，否则强制 stage 后再 commit。复发关键标记：[fp:bash|> action: git commit 失败(exit code、"Changes not staged for commit")]
