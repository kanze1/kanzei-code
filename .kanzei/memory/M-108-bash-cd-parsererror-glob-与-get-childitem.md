---
id: M-108
scope: project
category: fact
title: bash cd ParserError: glob(与 Get-ChildItem 路径解析失败必读
description: 处理 cd 命令 ParserError 报错 — glob/Get-ChildItem 组合路径解析失败,第 2 次跨轮复发
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

处理 bash 重试中 "ParserError:" 相关错误 (如行号 1 处的 `cd "C:\Users\..."; glob(...) { };`组合语法) 时必读：这是 C:/Windows PowerShell env 下的第 2 次跨轮复发故障模式。保留指纹：[fp:bash|> ParserError:](refs: )
