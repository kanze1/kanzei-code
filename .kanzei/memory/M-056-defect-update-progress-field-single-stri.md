---
id: M-056
scope: project
category: sop
title: defect update progress field: single string replaces first line, multi-line appends — [fp:edit|progress flailing accumulation]
description: 处理 defect update 进展字段语义 (单行换首行、多行追加)及游离段落永不清除时必读:永远记着任何工具都不能移除已产生的游离段落 -- 只配成英文 key name 值来追加 new content;绝不传跨行新 text to avoid flailing paragraph accumulation
status: candidate
created: 2026-08-12
updated: 2026-08-12
source: memory-manager
refs: D-204 D-239
---

2026-08-13实测确认:defect update的「进展」字段语义——①单行值 =替换-new进展: -行;②多行值(含换行)=作为新段落追加到条目末尾 (不替换); 游离段落(无-new键:-前缀的文本行,由多行update产生)一旦产生**永不清除**:update单行只替换首行，游离段落残留；tracker文件direct write denied、git restore/checkout被引擎拦截、shell整文件重写被拦——没有任何工具能删除游离段落。D-239因此积累3份「验收②复核」, 2份「第二轮复核」重复段落。教训:update进展字段前先get读当前值，单行追加新内容(旧内容拼在单行里);绝不传多行值；绝不为清理反复update越修越脏
