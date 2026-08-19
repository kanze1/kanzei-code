---
id: M-089
scope: project
category: fact
title: Git批次字段核对 SOP — 处理发版动作批次数不一致错误
description: 处理发版批次字段错配必读：手写批次(3/3)与Git提交历史标记数不一致必核查证;第N次复发时判据需补全"→修正再闭包"决策链
status: active
created: 2026-08-17
updated: 2026-08-19
source: 2026-08-13 inbox consolidation [note 5]
---

【复发检测键】[fp:req|R- 的手写批次是 Git 提交历史标记数为 ;请先核对并更新批次字段后再关闭。]

【适用场景】发版动作执行前或执行中，出现「批次数 mismatch」错误时必读:
- 手写工作区批次数字段(如 .kanzei/memory/batch=3/3)
 vs Git 提交历史实际标记数(如 git log --oneline -N | wc -l = 4)

【触发条件】闭包前必须满足三合一校验链:
1.核对: batch_field_value == git_log_commit_count
2.修正：batch_field_value ← 新计数值
3.闭包动作通过后验证: 提交状态=ok AND no "batch mismatch" error

【已知坑位/边界】
- R-243场景：验收条款对账需同步复核批次字段(如 note 6 的R-243 req 失败链路)
- 仅当确认这是另一个独立 pitfall 才新增条目;原样再记一遍为冗余

【晋升路径】第3次+复发且带修复成功证据→memory_add→memory_promote episode_id=868
