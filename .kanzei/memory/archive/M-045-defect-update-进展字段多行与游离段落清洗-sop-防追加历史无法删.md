---
id: M-045
scope: project
category: sop
title: defect update 进展字段多行与游离段落清洗 SOP — 防追加历史无法删除的脏数据陷阱
description: 处理 defect/req进展字段多行导致游离段落无法清除时必读：单行替换不能传多行值；旧内容必须拼入同一行;engine无工具支持自动删除非键文本行，反复update只会更脏(D-239)
status: deprecated
created: 2026-08-11
updated: 2026-08-12
source: memory-manager
refs: D-204 D-239
superseded_by: M-044
---

> 墓碑:2026-08-12 库存合并,本条已并入 M-044(内容重复,原文保留供追溯)。
【适用场景】在执行 get/update tracker field(-进展:)时遇到以下情况：1) - progress /-进展:字段的值含多行换行，导致引擎把它当成新增段落追加到文件尾部；2) 由此产生无法自动删除的「游离段落」——无 key:-前缀的任何文本行;3)。多次update试图去重反而使问题恶化;4) 需要清空该字段(仅留空字符串或完全移除)-进展 的行).

【操作步骤】
1. get读当前文件确认现有- progress / - 进展:的值及其长度与换行数；2. 若单行值，将旧内容拼进新内容后作为一整行传给 engine update(-progress/- 进展:)的 key;3③ 绝不传多行的字符串给update(会触发追加行为而非整行替换);4)想要清空该字段:先get读其当前完整文本,然后update时传入- 进展:/空字符/或仅含换行符的值—engine不会删除键本身，只会把该行内容为空或被忽略;5)任何工具都不可靠去「清理游离段落」:bash的edit无法命中多行、结构化editor无直接支持清除非字段行的方法。

【边界与例外】
——所有试图通过重复 update 自行「清洗」的策略都将累积更多脏数据，因为 engine 没有提供 field deletion mechanism或multi-line replacement beyond full line；D-239明确提到反复update清理只会变差;引擎缺少对「history deduplication」「field removal」的原生支持是系统性缺口。
