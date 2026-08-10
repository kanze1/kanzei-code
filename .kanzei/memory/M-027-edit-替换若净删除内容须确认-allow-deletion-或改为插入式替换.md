---
id: M-027
scope: project
category: fact
title: edit 插入/净删除时先保留原文并核对删除意图 — 处理 edit替换未保住old_string内容:看新增是否含原匹配段,有则必保;明确删才allow_deletion=true
description: 处理 edit 替换导致净删除或大段内容消失时必读：re-read target file, check if added text preserves matched old_string lines；若插入，full retain original content first; only allow_deletion=true when removal confirmed. Two failed cases share same recurrence key pattern — must keep all [fp:edit|...] markers verbatim as detection signatures
status: active
created: 2026-08-09
updated: 2026-08-10
source: memory-manager
---

[fp:edit|这次替换看着像插入(新文本多了 行),却没保住 old_string 里的原文——十有八九是想在附近加内容,结果把匹配到的那段顶掉了。要插入就把下面这些行原样写]
[fp:edit|这次替换净删除 行( 行换成 行)。确实要删就把 allow_deletion 置 true 重来;若本意是新增内容,说明 old_string 匹配到了不该动]
处理 edit 插入/净删除时，先 read 当前文件并逐行核对删除意图。插入时 `new_string` 必须完整、原样包含 `old_string`（含每行、缩进和上下文）再追加内容；只有确认确实要删除才设置 `allow_deletion=true`。本次错误原文：这次替换净删除 24 行(29 行换成 5 行)。确实要删就把 allow_deletion 置 true 重来;若本意是新增内容,说明 old_string 匹配到了不该动的位置——缩小 old_string 或改成插入式替换(把原文原样包含在 new_string 里)。被删内容包括 backlog 判定注释（原前端 stopAutoWhenBacklogEmpty：活动条目存在可推进项→Workable；无活动条目→Empty）。
