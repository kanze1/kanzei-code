---
id: M-038
scope: project
category: fact
title: 分支线未开启 tracker 写入时 req/defect 写操作被 ruleset 拒
description: req/defect update 报 permission denied by ruleset: req on 'write:update' 时必读:当前线未开启 tracker 写入(读不受限);在该线 .kanzei/project 设置显式开启后再改,勿用 bash/git 重定向绕过
status: deprecated
created: 2026-08-11
updated: 2026-08-18
source: memory-manager
---

R-140 修改唯一主根文档时报 `permission denied by ruleset: req on 'write:update'`:当前分支线未开启 tracker 写入；读取仍可用。请在该线的 `.kanzei/project` 设置中显式开启后再修改;bash/git 重定向绕过该检查不可用。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-038-r-140修改主根-doc权限拒-显式开启settings前勿改write-up.md)
