---
id: M-156
scope: project
category: fact
title: 记忆防重复规则：以 tracker refs 为准的交付状态事实判定
description: 处理 memory stale/duplicate 问题时必读：当记忆条目包含未验证的断言而 tracker 已有对应记录时，须退役该记忆并保留通用规则墓碑
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

规则证据：R-216 验收③确认 M-037（交付状态 R-178）正文含 `已在 dev 完整交付并关闭`，但 tracker(requirements.md) 已有单源真理记录。教训：防止以未验证记忆断言替代 tracker 记载；tracker 变更需经 engine 校验而非依赖散落记忆事实。退役理由模板："该交付状态已由 tracker/refs 取代，保留通用防重复实现规则的可追溯墓碑"。

错误候选模式证据：M-150/M-151 系先前 manager 未理解 STALE 而错误 ADD 的重复 candidate，须退役并标注"错误重复候选（因未理解 STALE 而错误 ADD）"。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-156-记忆防重复规则-以-tracker-refs-为准的交付状态事实判定.md)
