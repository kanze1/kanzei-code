---
id: M-044
scope: project
category: sop
title: tracker update 字段语义:中文键才精确匹配,英文键会追加,进展多行会产生永不可删的游离段落
description: 处理 edit 替换失败 fp:edit|这次替换看着像插入却未保住 old_string 原文或净删除时必读:先 read 重读核对;本意是 insert就把old_string逐行原样写进 new_string,只有确认要删才设 allow_deletion=true;正文的 [fp:edit|...] 标记是复发检测键不得改
status: active
created: 2026-08-11
updated: 2026-08-11
source: memory-manager;2026-08-12 库存合并(D-204/D-239 实测)
supersedes: M-045 M-047 M-048 M-050 M-051 M-053 M-054
refs: D-204 D-239
---

适用场景：处理 req/defect/goal 的 tracker update (优先级/进展/阻塞字段)

操作步骤:
1. get 读当前目标文件确认现有值
2. defect update: fields 是整字段替换(非增量合并),键名必须用中文(优先级/进展/阻塞),英文 key(name="priority"/"progress")会被当未知新键追加成重复脏数据,原两批交付证据丢失需从 git diff 找回再 update 恢复
3. 进展 field:单行 value=替换首行;多行含换行=value as paragraphs appended to END of entries(not replacing original location),游离段落(无"-键:"前缀的文本层)一旦产生**永不清除**:update single line only replaces first row, floating paragraph residue remains;tracker file direct write denied、git restore/checkout 被引擎拦截 shell 整文件重写被拦—没有任何工具能删除floating paragraphs
4. 清空字段传空字符串会留下空键(解析层忽略不删)

正确做法:①update前先get读当前field,进展必须拼旧内容+新 content整体单行传;绝不传多行数值

边界与例外:D-239因此积累3份验收复核、2份第二轮复核的重复段落。引擎需补"进度历史去重/字段删除"能力(D-239相关)
