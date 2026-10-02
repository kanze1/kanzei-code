---
id: M-005
scope: project
category: sop
title: .kanzei/project 托管文件禁止 edit,须用专用工具
description: 处理 .kanzei/project/defects.md 或其他 policy-managed 文件写入时必读：先不要调用 edit；若出现 tracker integrity broken 或 permission denied by ruleset，立即改用允许的专用写入/grep 通道并先修复报告的完整性门禁，避免重复 edit。
status: active
created: 2026-08-07
updated: 2026-08-24
source: inbox 2026-08-07
---

适用场景：写入 .kanzei/project/defects.md 或其他 policy-managed 文件，尤其 tracker 报完整性错误时。
操作与判据：
1. 先读取目标文件和错误上下文；写入动作不要调用 edit。
2. 若报 `REFUSING to write .kanzei/project/defects.md: tracker integrity is broken. active D-692: reserved status field `` is invalid for header `fixing``，先按 tracker 提示修复 D-692 的 reserved status/header 完整性问题；读取仍可用。
3. 若 edit 直接被 `permission denied by ruleset` 拒绝，立即切换到获准的专用写入/grep 通道，不要重复 edit；本次改用 grep 成功。
边界与例外：这是环境/工具契约门禁，不是普通内容编辑失败；只有完整性修复后才重试托管文件写入。
[fp:defect|行动: 处理 或其他 policy-managed 文件写入时必读：不要调用 edit，改用 permission denied by ruleset 立即切换]
[fp:edit|permission denied by ruleset: edit on .]
