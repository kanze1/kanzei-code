---
id: M-134
scope: project
category: fact
title: edit command replacement failure: preserve original content
description: 处理 edit 命令插入替换失败（原文被覆盖）时必读：区分 insert vs delete 参数 — R-165 需用 episode_id=608
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

适用场景：edit 命令插入替换时意外覆盖原文

操作步骤：
1) 观察到替换后 new_string 多了额外行，original content 被顶掉
2) 判断原因：想插入内容却错误地匹配覆盖了应保留的文本
3) 修复方案 A：将需要新增的行原样写进 new_string (不删除目标行)；方案 B：确实要替换目标行时设置 allow_deletion=true

边界与例外：未被保留的原文行需逐行检查并明确写入 new_string。Fingerprint (verbatim from note): [fp:edit|- assert!(].

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=true，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-134-edit-command-replacement-failure-preserv.md)
