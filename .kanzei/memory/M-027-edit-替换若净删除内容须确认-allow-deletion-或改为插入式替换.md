---
id: M-027
scope: project
category: fact
title: edit 插入时必须原样保留 old_string，避免把匹配区块顶掉
description: 处理 edit 插入式修改报“净删除 N 行”或替换结果少于 old_string 时必读：先 read 核对差异；插入必须将完整 old_string 原样放入 new_string 后追加内容，只有确需删除才设 allow_deletion=true，否则缩小匹配区块或改用插入式替换。
status: active
created: 2026-08-09
updated: 2026-08-09
source: memory-manager
---

处理 edit 插入式修改报“未保留 old_string”或“净删除 N 行”时，先 read 重读并缩小唯一匹配区块；若是插入，new_string 必须逐字保留完整 old_string 后再追加内容，只有确需删除才设 allow_deletion=true。

本轮错误原文：这次替换净删除 7 行(12 行换成 5 行)。确实要删就把 allow_deletion 置 true 重来；若本意是新增内容，说明 old_string 匹配到了不该动的位置——缩小 old_string 或改成插入式替换(把原文原样包含在 new_string 里)。先检查被删内容是否应保留，再决定 allow_deletion；不要以为 edit 成功就代表插入正确。
[fp:edit|这次替换看着像插入(新文本多了 行),却没保住 old_string 里的原文——十有八九是想在附近加内容,结果把匹配到的那段顶掉了。要插入就把下面这些行原样写]
[fp:edit|这次替换净删除 行( 行换成 行)。确实要删就把 allow_deletion 置 true 重来;若本意是新增内容,说明 old_string 匹配到了不该动]
