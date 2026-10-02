---
id: M-024
scope: project
category: fact
title: user/edt 检测旧新字符串完全一致时报错—nothing to do
description: 处理 user/edit 报错 "old_string and new_string are identical — nothing to do"：这是工具契约，无需实际操作文件
status: stale
created: 2026-08-09
updated: 2026-08-09
source: memory-manager
---

当 user/edit.rs (或类似用户编辑工具) report "old_string and new_string are identical — nothing to do"，说明：1）当前文件在该行无需修改；2）这是工具的契约检测机制用于防重复 edit。应对策略：确认差异是否真实存在——若确实相同则标记完成（no-op），若怀疑误判应检查源文件中该行内容。
指纹: [fp:edit|old_string and new_string are identical — nothing to do] - 这是复发检测键，丢失引擎就失去"记了但没用"的追踪能力

(stale: 已整合进 M-025 扩展版条目（增加通用性描述，覆盖场景更广）)
