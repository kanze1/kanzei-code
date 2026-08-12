---
id: M-010
scope: project
category: sop
title: edit 报 old/new 相同是 no-op 拒绝而非失败
description: 处理 edit 报“old_string and new_string are identical — nothing to do”时必读：停止重试，先 read 确认目标是否已是期望状态；若未完成则修改 new_string 使其与 old_string 不同，不要用 bash 绕过。
status: deprecated
created: 2026-08-07
updated: 2026-08-12
source: inbox note 2026-08-07
---

处理 edit 报错：old_string and new_string are identical — nothing to do。决策：这是 no-op 拒绝而非编辑失败；先 read 重读确认目标状态，已符合预期则结束，未符合则重新构造一个确实不同且正确的 new_string，禁止无意义重试或用 bash 绕过。\n[fp:edit|old_string and new_string are identical — nothing to do]
