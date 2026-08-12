---
id: M-020
scope: project
category: sop
title: req/defect close 自动归档,关闭证据须先写入进展字段
description: 处理 req/defect 的 close、close 后 update 报 unknown id，或需补验收证据时必读：先把证据写入进展字段再 close；close 后若 ID 不再可见，不要反复 req 重试，改用 git/history 检查归档记录。
status: deprecated
created: 2026-08-08
updated: 2026-08-12
source: memory-manager;2026-08-12 合并 M-039
supersedes: M-039
---

req/defect 执行 close 会自动归档；验收证据必须在 close 前写入进展字段（按 convention §1.25 逐项验收）。close 后再 update 可能报 `unknown id`，这是 ID 已归档/不在活动集合的信号；不要继续用 req 重试，改用 git/history 检查归档内容。

补充(M-039)：close 动作本身只归档、**不校验证据内容**——没有任何门禁会替你把关，所以证据(精确到代码位置的验收细节)必须在 close 前用 update 写进进展字段。reopen 去够归档条目同样要先证明条目的代码级细节已落进展。

[fp:req|unknown id `r-`; existing: r-, r-, r-, r-, r-, r-, r-, r-, r-, r-, r-, r-, r-, r]
