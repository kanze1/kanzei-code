---
id: M-247
scope: project
category: sop
title: bash guard全文件改写拦截SOP — 识别whole-file rewrites via shell bypass并使用edit/memory writer完成写入【新版】
description: M-247 bash guard全文件改写拦截 — 遇[fp:bash|...]必read再update：whole-file rewrites→ident→用edit/memwriter
status: active
created: 2026-08-18
updated: 2026-08-19
source: memory-manager
---

bash guard 全文件改写拦截：[permission denied by guard : is blocked: whole-file rewrites via shell bypass th] → Set-Content被block，whole-file rewrites bypass edit syntax validation/diff display。必须识别此模式并使用edit（单行修改）或memory manager完整写入。 [记忆命中 M-247 | sop]。复发判据：[fp:bash|permission denied by guard : is blocked: whole-file rewrites via shell bypass...]。M-247旧版未强调“改用req通道”分支，导致本轮仍没触发替代方案；新版补回shell bypass→识别→改走req的因果链。
