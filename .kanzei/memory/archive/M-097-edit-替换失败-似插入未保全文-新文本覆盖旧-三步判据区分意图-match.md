---
id: M-097
scope: project
category: sop
title: edit 替换失败 (似插入未保全文/新文本覆盖旧):三步判据区分意图 match exactly
description: 处理 edit 替换失败时必读：区分“新文本多/旧文丢失”vs“只想删旧文”，补全判据并保留 [fp] 标记供引擎检测复发
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
subject: edit 替换失败判据
---

edit 替换失败的三步判据：  
1) parse new_string 与 old_string 行数差 → 若新文本多了行但 old_string 没保住（旧文丢失） → action: copy original context lines to new_string；  
2) 若只是少了行、文本一致 → 可能只想删旧文 → action: allow deletion=true。  
3) （补充 Case3）当看到"old_string not found in Y — Closest line: '...'"时：检查构造的 old_string 是否完全匹配文件中的实际字符串（含空白/换行），否则重读再试。  

错误原文：这次替换看着像插入(新文本多了 10 行),却没保住 old_string 里的旧文—十有八九是想在附近加内容,结果把匹配到的那段顶掉了。
[fp:edit|这次替换看着像插入(新文本多了行),却没保住 old_string 里的原文—十有八九是想在附近加内容,结果把匹配到的那段顶掉了。要插入就把下面这些行原样写]

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=true，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-097-edit-替换失败-似插入未保全文-新文本覆盖旧-三步判据区分意图-match.md)
