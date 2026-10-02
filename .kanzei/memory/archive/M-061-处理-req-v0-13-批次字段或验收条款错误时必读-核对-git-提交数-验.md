---
id: M-061
scope: project
category: sop
title: 处理 req v0.13 批次字段或验收条款错误时必读：核对 Git 提交数/验收逐条对账,否则拒收
description: 处理 req v0.13 批次字段或验收条款错误时必读：核对 Git 提交数/验收逐条对账,否则拒收 — 补「Git marker vs batch」判据
status: deprecated
created: 2026-08-13
updated: 2026-08-18
source: 会话 2026-08-13 自举质量分析(R-199/D-320 案例)
---

[req] 验收条款对账失败根因与修复 SOP
R-243 手批次数≠ Commit Count; R-243 验收条款未覆盖 → 闭包被拒

[Pitfall History] [fp:req|R- 验收条款对账未过... T- 测试记录 file:line/submit_id] 本轮复发→判据不够尖锐
- M-061 原有描述：「每次自举运行结束后做质」→过于模糊，缺失具体条款验证步骤与证据锚要求
- R-243 错误原文证明：验收列出条款①②③④⑤⑥,其中部分在进展中未提及。关闭前每条验收必须在进展里逐条覆盖并带证据锚

[适用场景]
[req] v0.13 自举运行/发布流程中 R-243 验收对账失败时必读：确保所有条款逐项验证，否则拒绝闭包

[操作步骤]
1. 列出所有验收条款(①@#6) → 在 [进展日志] 中逐条引用并附证据锚 (file:line/commit_id)
2. 若某条款无法覆盖 → 显式声明：「验收降级:〈编号〉原文→实际+理由」或「〈编号〉由用户执行」
3. 跳过不声明即触发拒 → 闭包动作被 R-243 回退

[边界与例外]
- 证据真伪由波次审计另查 (docs/design/bootstrap_quality_audit.md) → 本地 SOP确保格式正确即可

(stale: fact superseded by R-243 refs: Git marker≠batch 判据缺失，导致 recall hook 不足以阻止复发；新 SOP 将含完整判据+fp 标记)
