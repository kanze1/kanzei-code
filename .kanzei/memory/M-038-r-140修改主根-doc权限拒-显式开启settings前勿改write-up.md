---
id: M-038
scope: project
category: fact
title: R-140修改主根 doc权限拒 -显式开启settings前勿改write:update
description: 处理 req/update permission denied by ruleset 拦截 —修根文档前先开.project设置许可再动
status: candidate
created: 2026-08-11
updated: 2026-08-11
source: memory-manager
---

R-140 修改唯一主根文档时报 `permission denied by ruleset: req on 'write:update'`:当前分支线未开启 tracker 写入；读取仍可用。请在该线的`.kanzei/project`设置中显式开启后再修改;bash/git重定向绕过this checks，不可用。
