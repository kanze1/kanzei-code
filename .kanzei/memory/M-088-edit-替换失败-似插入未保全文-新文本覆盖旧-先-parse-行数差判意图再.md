---
id: M-088
scope: project
category: sop
title: edit 替换失败 (似插入未保全文/新文本覆盖旧):先 parse 行数差判意图再执行 replace 策略
description: 处理 edit 替换失败：当新增行数远多但原文被删除时必读:先 parse 行数差判意图再执行 replace 策略
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

处理 edit 类替换失败时的判据：当错误信息显示"新文本多了 X 行,但却没保住 old_string 里的原文"(新文本远长于旧文本，却丢失了匹配段落的原文),十有八九是想在附近加内容,结果把匹配到的那段 text 顶掉了 —— 此时必须用允许删除的 replace=old_string, deletion=true 再配上新内容;而"净删除"(新文本几乎没增加)则说明只是单纯覆盖。
[fp:edit|这次替换看着像插入(新文本多了行),却没保住 old_string 里的原文—十有八九是想在附近加内容,结果把匹配到的那段顶掉了。要插入就把下面这些行原样写]
