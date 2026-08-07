---
id: M-005
scope: project
category: sop
title: .kanzei/project 托管文件禁止 edit,须用专用工具
description: 处理 .kanzei/project 下文件 edit 被 ruleset 拒绝(permission denied / policy-managed)时必读
status: active
created: 2026-08-07
updated: 2026-08-07
source: inbox 2026-08-07
---

对 `.kanzei/project/` 下由规则集托管的文件(如 defects-archive.md)调用 edit 工具会被拒绝,错误原文:`permission denied by ruleset: edit on \`.kanzei/project/defects-archive.md\`. This resource is policy-managed; use the dedicated tool for it.`。改用该资源对应的专用工具(如 tracker/defect 管理工具)或经允许的 bash 方式操作,不要反复用 edit 重试。
