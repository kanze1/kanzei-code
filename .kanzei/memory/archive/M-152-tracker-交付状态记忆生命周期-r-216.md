---
id: M-152
scope: project
category: fact
title: tracker 交付状态记忆生命周期 R-216
description: 处理 tracker 交付状态管理时必读：R-216 验收流程、M-037 stale 化原因、归档路径验证
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
subject: tracker 交付状态管理
---

R-216 验收流程：当 tracker refs/交付状态内容（如"已在 dev 完整交付并关闭"）与现有 memory candidate 冲突时，必须执行以下操作：

1. 识别 M-XXX entry 是否仍为 project candidate 且含 tracker 交付状态内容
   - 检查 R-178/R-XXX 引用是否已存在（说明交付已按 tracker 完成）
   - 若存在且 delivery state 内容重复/过时，该 memory entry 不再适用

2. 对 M-037 等仍为 candidate 的条目执行 memory_stale
   - reason：「该交付状态已由 tracker/refs 取代，保留通用防重复实现规则的可追溯墓碑」
   
3. 归档其他相关候选（M-032/M-033/M-035/M-036/M-040）
   - 确保 archive 路径下原文墓碑保留
   - entry status 设为 deprecated

4. 回读验证：确认所有 6 个 ID（M-032~M-040+M-037）的状态一致性
   - archive 存在 + status=deprecated 或 stale

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-152-tracker-交付状态记忆生命周期-r-216.md)
