---
id: M-187
scope: project
category: fact
title: D-480(root cause): req id 不匹配导致后续流程中断
description: 处理 D-480 失败根因分析：何时该记得「req ID 不匹配导致后续流程中断」——判断根因是否可复用。
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

【事件描述】
当前 D-480 迭代中，req 工具返回一次 unknown id 异常，具体为：unknown id `R-216`; existing: R-286,R-283,R-284,R-285,R-287,R-235,R-101,R-242,R-243,R-245,R-248,R-249,R-264,R-281,R-288。

【根因分析】
该异常为工具契约层面的问题，非本次任务的一次性噪声或 bug 本身：
- req 工具期望 R-*ID*在项目中存在且格式完整（如 R-286、R-287 等已知序列），而非不存在的 R-216。
- 异常触发点位于 D-480(fixed)流程中「req→collaboration_status」环节，表明后续依赖的 defect/toolchain 因缺少有效 ID 而中断。

【可复用知识】
- 通用约束：在使用 req 工具前，应先用 collaboration_status 或类似工具验证目标 R-*ID*的存在性与格式（至少确认是否存在 R-286、R-245 等已知序列）。
- 错误模式识别：req 返回 unknown id(R-*N*不存在于现有资源表)为重复失败的信号，此时应回退到 bash→defect 修正流程。

【边界】
若某任务明确使用不存在的 R-*ID*(如 R-216)，此为人为配置问题而非 toolchain 缺陷——应先修正 req 参数，而非依赖 SOP 自动修复。】

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-187-d-480-root-cause-req-id-不匹配导致后续流程中断.md)
