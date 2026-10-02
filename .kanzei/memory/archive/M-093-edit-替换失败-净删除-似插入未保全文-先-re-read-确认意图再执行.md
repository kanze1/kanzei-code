---
id: M-093
scope: project
category: fact
title: edit 替换失败 (净删除/似插入未保全文):先 re-read 确认意图再执行
description: 处理编辑旧字符串看似插入却丢失原文时的召回钩子
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

当 edit 替换操作看似插入新行却丢失 old_string 原文时（十有八九是想在附近加内容，结果把匹配到的段落顶掉），必须先 re-read 确认是「追加」还是「覆盖」意图。若为追加：将待插入行原样写入 new_string；若确需替换某段：置 allow_deletion=true。
[fp:edit|这次替换看着像插入(新文本多了行),却没保住 old_string 里的原文—十有八九是想在附近加内容,结果把匹配到的那段顶掉了。要插入就把下面这些行原样写]

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=true，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-093-edit-替换失败-净删除-似插入未保全文-先-re-read-确认意图再执行.md)
