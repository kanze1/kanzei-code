---
id: M-023
scope: project
category: fact
title: edit cannot read 与 grep invalid regex：先验证正则再判权限
description: 处理 grep 报 invalid regex/regex parse error，尤其查询含未转义括号、花括号或管道，或同时出现 edit cannot read/拒绝访问时必读：先将查询改为固定字符串或合法正则，并单独验证无 parse error；验证成功前不得判断路径/权限、重复 edit 或重试。
status: active
created: 2026-08-09
updated: 2026-09-02
source: inbox 2026-08-09
---

[fp:grep|invalid regex : regex parse error:]
[fp:edit|cannot read 拒绝访问。 (os error )]
错误原文：invalid regex `TrackerTool {`: regex parse error: (?:TrackerTool {) error: repetition quantifier expects a valid decimal。
行动判据：grep 查询包含未转义的 `{`、括号、管道等正则元字符时，先按固定字符串搜索或转义/改写为合法正则并单独验证；只有验证通过后，才判断 edit cannot read、路径或权限问题，不得重复 edit 或重试。
