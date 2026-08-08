---
id: M-020
scope: project
category: sop
title: req/defect close 自动归档,关闭证据须先写入进展字段
description: 处理 req/defect 的 close 动作、close 后 update 报 unknown id、或需补验收证据(convention §1.25 逐项验收)时必读:证据必须在 close 前写入进展
status: active
created: 2026-08-08
updated: 2026-08-08
source: memory-manager
---

实测 2026-08-09:R-086/D-200 close 后条目立即出现在 requirements-archive.md/defects-archive.md,随后 req/defect update 报 unknown id(活动表里已不存在)。因此:
- close 会触发自动归档,归档后 update 拒绝(unknown id),没有合法补写通道。
- convention §1.25 要求的逐项验收证据(代码位置等)必须在 close 之前通过 update 写进条目的「进展」字段。
- close 只带走 close 时已存在的字段;close 之后想补证据只能放弃(.kanzei/project 托管文件禁止直改,见 M-005)。
