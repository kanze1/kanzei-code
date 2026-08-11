---
id: M-050
scope: project
category: sop
title: defect/req update 字段键名与值处理 SOP — [fp:tool] detection key
description: 缺陷更新导致英文键追加、整段替换：何时用中文键，单行多行拼接策略
status: deprecated
created: 2026-08-11
updated: 2026-08-12
source: memory-manager
refs: D-204 D-239
superseded_by: M-044
---

> 墓碑:2026-08-12 库存合并,本条已并入 M-044(内容重复,原文保留供追溯)。
defect/req update 的字段规则（SOP）：**[fp:tool|field key must be Chinese]**

适用场景：处理 defect/req 条目更新时出现"新键追加"导致字段数增加、或原值丢失的情况。

操作步骤：
① **建条前必 read** 当前文件，确认现有字段格式（如 `- title:`）；避免盲目构造 update payload。
② **中文键优先**：引擎按键名精确匹配，英文 key 会被当作"新键追加"导致重复/脏数据。必须用 `优先级`、`进展`、`阻塞` 等中文字段键。（测试：传`{ "priority": "P3", "进展":"" }`会新增 `- priority: P3`；而传入 `{ "优先級":"高"\}`则更新原字段）。
③ **单行/多行拼接**（针对"进展"这类可分段内容）：**单行值=替换首行；多行值(含换行)=追加段落尾**。不要传多行去避免意外产生游离段落。
   - 想插入旧内容的续写：先把已读到的完整文本拼回 single string，再整体更新（例:`"{ '进展':'原内容 + 新内容'}"`）。
④ **勿清空=增杂**：传空字符串会留下"空的字段行"(解析层忽略但不删)，反复清空白制造脏数据。

边界与例外：引擎目前没有「去重历史/删除已弃置段落」能力（见 D-204/D-239）。游离段落一旦产生，无任何 tool 能直接清除(update/git restore/shell rewrite均被截)。
