---
id: M-062
scope: project
category: fact
title: 环境约束:本机 WebView2 151 DevTools 端口从不绑定,e2e CDP 路线不可用
description: 处理 browser 报“需要 url 或 path 参数”或准备 e2e-smoke / connectOverCDP / WebView2 DevTools 端口路线时必读：先补齐合法 url/path；若目标是本机 WebView2 CDP，则停止该不可用路线，不要重推或反复调用 browser。
status: active
created: 2026-08-13
updated: 2026-08-24
source: inbox 升格(2026-08-13;原 inbox 条目随清理丢失,内容自探查代理摘录恢复)
---

环境约束：本机 WebView2 151 DevTools 端口从不绑定，e2e CDP 路线不可用；browser 工具调用必须提供 url 或 path 参数，缺参时先修正调用参数而不是重试空调用。

复发证据：本轮 browser 再次报错“browser 需要 url 或 path 参数”，且已有本记忆命中；处理时区分参数契约错误与本机 CDP 路线不可用：前者补齐参数，后者停止路线。
[fp:browser|browser 需要 url 或 path 参数]
