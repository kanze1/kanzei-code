---
id: M-027
scope: project
category: fact
title: edit 插入时必须原样保留 old_string，避免把匹配区块顶掉
description: 处理 edit 插入式修改报“未保留 old_string”或“净删除 N 行”时必读：先 read 重读并缩小唯一匹配区块；若是插入，new_string 逐字保留完整 old_string 后再追加内容，只有确需删除才设 allow_deletion=true。
status: active
created: 2026-08-09
updated: 2026-08-09
source: memory-manager
---

edit 做插入式修改时，new_string 必须逐字保留完整 old_string，只能在其前后追加内容；提交前先 read 重读当前区块、缩小 old_string 到正确且唯一的目标，并核对关键原文仍在。若确实要删除匹配内容，显式设置 allow_deletion=true；否则不要用会删除匹配区块的替换重试。

典型错误：未保留的原文包括 `use crate::{hidden_command, CONVENTIONS_REL};`；另一次将 5 行替换成 1 行并删除 `use kanzei_tools::tracker::schedule_for_display;`、`pub(crate) const CONVENTIONS_REL: &st...`。这类反馈分别表示插入意图误用了替换，或 old_string 命中不该动的位置，应收窄匹配或把原文完整放回 new_string。

[fp:edit|这次替换看着像插入(新文本多了 行),却没保住 old_string 里的原文——十有八九是想在附近加内容,结果把匹配到的那段顶掉了。要插入就把下面这些行原样写]
[fp:edit|这次替换净删除 行( 行换成 行)。确实要删就把 allow_deletion 置 true 重来;若本意是新增内容,说明 old_string 匹配到了不该动]
