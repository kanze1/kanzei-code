---
id: M-027
scope: project
category: fact
title: edit 替换若净删除内容,须确认 allow_deletion 或改为插入式替换
description: 处理 edit 报「净删除 N 行」或「看着像插入却没保住 old_string 原文」时必读:先 read 重读目标文件核对删除意图;本意是插入就把 old_string 原文逐行原样写进 new_string 再追加新内容,只有确认要删除才设 allow_deletion=true;正文的 [fp:edit|...] 标记是复发检测的键,不得删改。
status: active
created: 2026-08-09
updated: 2026-08-12
source: memory-manager;2026-08-12 修正被写坏的标题并合并 M-052
supersedes: M-052
---

[fp:edit|这次替换看着像插入(新文本多了 行),却没保住 old_string 里的原文——十有八九是想在附近加内容,结果把匹配到的那段顶掉了。要插入就把下面这些行原样写]
[fp:edit|这次替换净删除 行( 行换成 行)。确实要删就把 allow_deletion 置 true 重来;若本意是新增内容,说明 old_string 匹配到了不该动]
处理 edit 插入/净删除时，先 read 当前文件并逐行核对删除意图。插入时 `new_string` 必须完整、原样包含 `old_string`（含每行、缩进和上下文）再追加内容；只有确认确实要删除才设置 `allow_deletion=true`。本次错误原文：这次替换净删除 24 行(29 行换成 5 行)。确实要删就把 allow_deletion 置 true 重来;若本意是新增内容,说明 old_string 匹配到了不该动的位置——缩小 old_string 或改成插入式替换(把原文原样包含在 new_string 里)。被删内容包括 backlog 判定注释（原前端 stopAutoWhenBacklogEmpty：活动条目存在可推进项→Workable；无活动条目→Empty）。
