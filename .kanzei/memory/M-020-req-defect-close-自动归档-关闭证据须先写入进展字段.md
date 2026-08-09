---
id: M-020
scope: project
category: sop
title: req/defect close 自动归档,关闭证据须先写入进展字段
description: 处理 req/defect 的 close、close 后 update 报 unknown id，或需补验收证据时必读：先把证据写入进展字段再 close；close 后若 ID 不再可见，不要反复 req 重试，改用 git/history 检查归档记录。
status: active
created: 2026-08-08
updated: 2026-08-09
source: memory-manager
---

req/defect 执行 close 会自动归档；验收证据必须在 close 前写入进展字段（按 convention §1.25 逐项验收）。close 后再 update 可能报 `unknown id`，这是 ID 已归档/不在活动集合的信号；不要继续用 req 重试，改用 git/history 检查归档内容。

[fp:req|unknown id `r-`; existing: r-, r-, r-, r-, r-, r-, r-, r-, r-, r-, r-, r-, r-, r]
