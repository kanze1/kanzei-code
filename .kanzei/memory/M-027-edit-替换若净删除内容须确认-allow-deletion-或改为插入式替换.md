---
id: M-027
scope: project
category: fact
title: edit 插入/净删除时先保留原文并核对删除意图
description: 处理 edit 提示 old_string 区块被顶掉、替换净删除行或看似插入却丢原文时必读：先 read 重读并逐行核对；插入时 new_string 必须完整原样包含 old_string（含每行、缩进和上下文）再追加内容，只有确认要删除才设 allow_deletion=true，否则缩小 old_string 或改为插入式替换。
status: active
created: 2026-08-09
updated: 2026-08-10
source: memory-manager
---

处理 edit 插入/替换时，先 read 当前目标并逐行核对。若意图是插入，new_string 必须完整保留 old_string 后再追加；若提示净删除行，先判断是否误匹配：确实要删才设 allow_deletion=true，否则缩小 old_string 或改为保留原文的插入式替换。

复发错误原文：这次替换净删除 4 行(21 行换成 17 行)。确实要删就把 allow_deletion 置 true 重来;若本意是新增内容,说明 old_string 匹配到了不该动的位置——缩小 old_string 或改成插入式替换(把原文原样包含在 new_string 里)。 将被删掉的内容:   - // 打开设计文档/索引:docs_read 已支持 architecture 与任意 docs/design 文件——   - // 设计文档没有专用 kind,走 docs
[fp:edit|这次替换净删除 行( 行换成 行)。确实要删就把 allow_deletion 置 true 重来;若本意是新增内容,说明 old_string 匹配到了不该动]

保留既有复发判据：
[fp:edit|这次替换看着像插入(新文本多了 行),却没保住 old_string 里的原文——十有八九是想在附近加内容,结果把匹配到的那段顶掉了。要插入就把下面这些行原样写]
