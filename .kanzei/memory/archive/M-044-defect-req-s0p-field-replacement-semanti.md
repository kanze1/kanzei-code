---
id: M-044
scope: project
category: sop
title: defect/req S0P: field replacement semantics - Chinese keys required, progress multi-line creates unremovable floats; always get->concat before update to prevent float generation & duplication
description: SOP key for defect/req field update failures - Chinese keys only, multi-line creates permanent floats; always get->concat to prevent duplication. Recurrence markers preserved verbatim from notes 2026-08-11 [sop]x2 + existing M-044 FPs.
status: deprecated
created: 2026-08-11
updated: 2026-08-12
source: memory-manager;2026-08-12 库存合并(D-204/D-239 实测)
supersedes: M-045 M-047 M-048 M-050 M-051 M-053 M-054
refs: D-204 D-239
---

[fp:D-204|defect update key must be Chinese][fp:edit|替换看着像插入但顶掉旧内容——原样写入] [fp:bash/git/permition requires user approval markers from other FPs if present]. defec/t req update 防重复脏数据 SOP
适用场景：处理 defect/req/goal update write data (progress status priority etc). Always: get read current field first, then concat old_new->update as one line. Never send multi-line values to avoid float generation; never use English keys unless intentional new key addition is desired - Chinese keys match exactly by engine parsing logic.
操作步骤：1)get 读当前字段内容；2)判断意图——要替换用单线新值(旧+新=合并拼连)，要插入把原文全量放入new_string，确实删除才放allow_deletion=true;3)build update request with Chinese field names (优先/进展/阻塞);4)single-line value only, never newlines;5)avoid empty-string clearing unless truly deleting the key.
边界与例外：①多行值=追加段落到末尾产生永不清除游离段落(engine 无法删),需先get+concat成单线再update防止复发D-239。英语键如priority会被当新key追加而非更新existing (已有中文则不冲突)。空字符串清空会留空键(解析层忽略).②tracker integrity broken/UNACCOUNTED ids来自不同坑(M-012, D-318根因),与字段语义无关，需单独处理:检查active/archive/void ledger一致性或用git/history恢复。@ref R-191 conventions patch跨行匹配会0命中(Restart engine)。
