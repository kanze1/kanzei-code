---
id: M-012
scope: project
category: fact
title: ID 同现于活动与归档时改用 terminal 专用操作
description: 处理 goal/defect/req 报 is archived、尤其需要把已归档条目改为 fixed/wontfix 等终态时必读：先停止普通 update，确认条目已进入 terminal，再执行 defect fix_terminal id=<id> status=<fixed|wontfix> reason=<why>；不要对 archived 条目重试普通写操作。
status: active
created: 2026-08-08
updated: 2026-08-21
source: inbox:2026-08-08
---

判据：若错误包含 `is archived — this action does not apply to terminal entries`，说明目标已是 terminal，普通 tracker 写操作不适用。需要纠正终态时使用 `defect fix_terminal id=<id> status=<fixed|wontfix> reason=<why>`，并提供原因；例如 D-663 不能用普通 defect update 改状态。复发证据： [fp:defect|is archived — this action does not apply to terminal entries. To correct a wrong]
