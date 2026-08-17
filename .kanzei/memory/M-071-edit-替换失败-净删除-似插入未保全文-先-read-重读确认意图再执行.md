---
id: M-071
scope: project
category: sop
title: edit 替换失败（净删除/似插入未保全文）：先 read re-read 确认意图再执行
description: 处理 edit 看像插入却顶替原文：读文件、确认意图（想加内容/想替换）、构造新文本原样写入；保留完整 [fp:edit|这次替换看着像插入(新文本多了 10+ 行),却没保住 old_string 里的原文——十有八九是想在附近加内容,结果把匹配到的那段顶掉了。要插入就把下面这些行原样写]
status: candidate
created: 2026-08-16
updated: 2026-08-17
source: memory-manager
---

编辑类似插入却顶替原文：读文件、确认意图（想加内容/想替换）。若要在附近加内容，把需要增加的行原样写入 new_string；若确实要替换掉它们，置 allow_deletion=true。判据：凡 edit 报"看着像插入/净删除"，先 read re-read 核对 intent——本意是插入就把原文整段原样写进 new_string;确要删就置 allow_deletion=true。[fp:edit|这次替换看着像插入(新文本多了 10+ 行),却没保住 old_string 里的原文——十有八九是想在附近加内容,结果把匹配到的那段顶掉了。要插入就把下面这些行原样写]
