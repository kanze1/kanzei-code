---
id: M-003
scope: project
category: fact
title: tracker 状态机只进不退,doing→todo 会被直接拒绝
description: req/defect/goal 的 update 反复报 cannot move backward 时必读:状态只能沿列表顺序前进
status: active
created: 2026-08-07
updated: 2026-08-08
source: run(失败信号自动采集) + 人工校正
---

docstore 的 `transition_allowed` 是单向的:状态只能沿该文档类型的 statuses 列表向前走
(req: todo→doing→done/dropped;defect: open→fixing→fixed/wontfix),或直接进终态。
往回退会被拒绝,错误原文是 ``cannot move backward `doing` → `todo`; forward only``。
双向类型(goal/memory,bidirectional=true)不受此限,非终态之间可自由往返。

真要把已关闭的条目重新打开,只能手改 markdown——引擎不提供 reopen 动作。

校正记录(2026-08-08):本条由失败信号自动采集生成,fast 档蒸馏时把"同一错误重复出现 7 次"
误写成了"需要约 7 次重试才能成功",那是错的——重试多少次都不会成功,因为该转换本身非法。
教训见 [[记忆蒸馏改用 primary]]:失败**次数**是信号强度,不是被记忆的事实内容。
