---
id: M-023
scope: project
category: fact
title: edit 报 cannot read 拒绝访问与 grep invalid regex 的判别及处理
description: 处理 edit 报 cannot read/拒绝访问或同时出现 grep 正则解析错误时必读：先 read 重读目标；grep 遇未闭合正则立即停止并修正/改用固定字符串，随后再重试 edit，不要把正则语法错误当权限问题或反复重试。
status: active
created: 2026-08-09
updated: 2026-08-21
source: inbox 2026-08-09
---

处理 edit 报 "cannot read ... 拒绝访问 (os error 5)" 时，先 read 重读目标，再重试 edit；将其判断为 Windows 瞬态访问拒绝，不是真实权限/路径问题，不要改用 bash 绕过或放弃。若 grep 报 invalid regex，错误原文示例：invalid regex `action == "claim"|legacy|claim(`: regex parse error: (?:action == "claim"|legacy|claim() ^ error: unclosed group；先停止使用未闭合的正则，改用 read 或修正正则，再继续定位。复发标记：[fp:grep|invalid regex : regex parse error:]；原有复发标记：[fp:edit|cannot read 拒绝访问。 (os error )]
