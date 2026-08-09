---
id: M-027
scope: project
category: fact
title: edit 插入时必须原样保留 old_string，避免把匹配区块顶掉
description: 处理 edit 报“净删除 N 行”或插入式修改未保留 old_string 时必读：先 read 重读目标区块；若意图是插入，new_string 必须逐字包含完整 old_string 并仅在其前后追加内容，提交前核对关键原文仍在；只有确需删除时才设 allow_deletion=true。
status: active
created: 2026-08-09
updated: 2026-08-09
source: memory-manager
---

处理 edit 报“净删除 N 行”或插入式修改未保留 old_string 时：先 read 重读并核对命中区块。若是插入，new_string 必须逐字保留完整 old_string，再追加新增行；不要用“附近内容+新增内容”替代命中区块。提交前检查原文关键行仍在；确实要替换/删除这些行时才设 allow_deletion=true。

本次错误原文：这次替换看着像插入(新文本多了 30 行),却没保住 old_string 里的原文——十有八九是想在附近加内容,结果把匹配到的那段顶掉了。要插入就把下面这些行原样写进 new_string;确实是要替换掉它们,就置 allow_deletion=true。 未被保留的原文：   - pub fn process_list(state: State<'_, AppState>, project_dir: String) -> Result<Vec<ProcessInfo>,

[fp:edit|这次替换看着像插入(新文本多了 行),却没保住 old_string 里的原文——十有八九是想在附近加内容,结果把匹配到的那段顶掉了。要插入就把下面这些行原样写]
[fp:edit|这次替换净删除 行( 行换成 行)。确实要删就把 allow_deletion 置 true 重来;若本意是新增内容,说明 old_string 匹配到了不该动]
