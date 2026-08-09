---
id: M-027
scope: project
category: fact
title: edit 插入时必须原样保留 old_string，避免把匹配区块顶掉
description: 处理 edit 看似插入却报“未被保留的原文”或替换后 old_string 区块被顶掉时必读：先 read 核对目标区块；要插入就把完整 old_string（含每行、缩进和上下文）原样放进 new_string 后再追加内容，只有确需删除原文才设 allow_deletion=true。
status: active
created: 2026-08-09
updated: 2026-08-09
source: memory-manager
---

[edit] [插入时必须原样保留]
[fp:edit|这次替换净删除 行( 行换成 行)。确实要删就把 allow_deletion 置 true 重来;若本意是新增内容,说明 old_string 匹配到了不该动]
[fp:edit|这次替换看着像插入(新文本多了 行),却没保住 old_string 里的原文——十有八九是想在附近加内容,结果把匹配到的那段顶掉了。要插入就把下面这些行原样写]

错误原文：这次替换看着像插入(新文本多了 1 行),却没保住 old_string 里的原文——十有八九是想在附近加内容,结果把匹配到的那段顶掉了。要插入就把下面这些行原样写进 new_string;确实是要替换掉它们,就置 allow_deletion=true。未被保留的原文：  - let output = std::process::Command::new("git")  - .current_dir(root)  - .output()  - .ok()?;

决策判据：收到该错误后停止凭意图重试，先 read 重读实际目标；插入时 new_string 必须包含完整 old_string，再在正确位置追加新行；若确实要删除，显式设置 allow_deletion=true，否则缩小匹配区块或改用保留原文的替换。
