---
id: M-054
scope: project
category: sop
title: defect/req update 字段键名与进展处理规则 — [fp:field|Chinese key required for replace]
description: 缺陷更新时必读：整字段替换机制、中文键精确匹配要求、英文键追加陷阱、进展单线/多线语义差异。反复复发时的修复 SOP。
status: deprecated
created: 2026-08-11
updated: 2026-08-12
source: memory-manager
subject: 安装通道 defect update field behavior
superseded_by: M-044
---

> 墓碑:2026-08-12 库存合并,本条已并入 M-044(内容重复,原文保留供追溯)。
defect update defect D-204 fields: 机制与陷阱 — [fp:field|关键:Chinese key required for replace | English key appends new field causing duplicity]

【整字段替换行为】
- 引擎按键名精确匹配更新，英文键被视为未知新键会被追加到文件末尾。重复提交相同/相似结构导致不同文件名堆积。
- 错误示例：defect update D-204 {"priority": "P3", "进展":"..."} → 原"- 优先级:P2"保留且"- priority: P3"新增为新字段，原有两批交付证据丢失需从 git diff 找回再 restore 恢复。

【必须遵守的规则】
1) **键名用中文**: `优先级`/`进展`/`阻塞` (引擎按精确匹配更新),英文(如priority)/key会被追踪为未知追加新字段,重复提交导致不同文件累积、脏数据难以清理，需从 git diff 找回历史版本再 update。

2) **progress single-line vs multi-line semantics**
- **单行值**(换行符内): =替换 `- 进展:`行的整内容(旧内容全部移除);若只传新值而不用concat逻辑会丢失原多段证据、无法恢复原始交付证明，需从 git diff 找回历史。

3) **progress multi-line**: →追加段落到条目末尾,不替换原有首行;游离段落(无`-键:`前缀的文本行,由多行 update 产生)一旦创建**永不清除**(tracker direct write denied/git restore被引擎拦截/shell整文件重写被拦),反复update清理只会越发脏。D-239已积累3份「验收②复核」+2份「第二轮复核»重复段落。

4) **清空字段陷阱**: 传空字符串留下“空键”(解析层忽略不删,但仍占位标记占用)。

【正确 SOP】
Step1: update前先 get读当前值; Step2:单行追加新内容(旧内容拼在单行里);绝不要多线值造成游离段落永清除。

- Exception边界：若确实要删除某段证据且无法通过其他方式移除,则清空字段会留下空键占位——但引擎层面保留解析层可见性;需结合 git history 找回完整交付证明再恢复原始内容。
