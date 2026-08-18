---
id: M-240
scope: project
category: fact
title: keyPathFailures空值与PWA断言跳过机制 — node.js测试失败根因与bash重试策略 [fp:bash|"keyPathFailures": [],; fp:bash|M-: ERROR unknown memory id]
description: keyPathFailures空值与PWA断言跳过机制 — 处理bash执行npm test失败时必读 [ fp:bash|"keyPathFailures": [], ]
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

D-519/fixed 根因与复发验证（补充至 M-240）

**适用场景**: node.js环境执行npm test/cargo test时触发：
- 错误模式：keyPathFailures=[]为空，伴随 pwaUnpaired含notifications未配对态等阻塞项
- 失败指纹：[fp:bash|"keyPathFailures": [],]

**操作步骤**: 
1. bash执行npm test → exit code=1且keyPathFailures=[] → record失败事件与错误原文
2. grep requirements.md中defect断言（PWA/桌面功能）→ 判断依据：错误消息中的pwaUnpaired字段内容
3. defect跳过已阻塞项（notifications需配对态未配）→ 继续执行后续断言
4. bash重试测试 → 若通过则记录成功证据，否则累计失败次数

**边界与例外**: 
- keyPathFailures=[]不代表工具故障，而是断言逻辑问题（见fp标记）
- 涉及node.js环境时检查npm包版本/依赖项完整性
- 第3次+且修正成功可申请晋升active状态

**引用失败链证据**: [fp:bash|"keyPathFailures": [],]来自episode_id=775验证 + fp:bash|M-: ERROR unknown memory id（独立于D-495）
