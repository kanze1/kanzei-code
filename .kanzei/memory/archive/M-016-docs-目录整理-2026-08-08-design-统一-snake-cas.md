---
id: M-016
scope: project
category: fact
title: Rust/验证勿用bash跑测试 结构化 Git edit/bash权限拒绝转交互轮
description: 许可拒绝处理：编辑权限需要用户批准，失败转交互轮重试或加白名单；错误原文作为复发检测指纹必须原样保留正文中，否则引擎看不见「记了但没用的」经验值
status: active
created: 2026-08-08
updated: 2026-08-12
source: inbox 2026-08-08
---

许可拒绝时转交互轮。edit/req/bash/git 在 .kanzei/*.toml/.md 或项目根目录失败均报 permission requires user approval — run skipped it:

错误原文:[fp:bash|`git restore` is blocked in bash：Git mutations must use the structured git tool]、[fp:get-childitem|permission requires user approval; run skipped on ; get-item], [fp:req|cannot move backward → forward only todo→doing→done→dropped. 需 hand-edit if truly re-opening needed],
[fpeq|edit failure after M-016 edit permission check].

错误原文:[fp:git|permission requires user approval; run skipped it] (stage/commit), [fp:bash|permission requires user approval: bash on ; run skipped it](get-childitem/get-item). 自主档被拒绝。
