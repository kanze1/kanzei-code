---
id: M-132
scope: project
category: fact
title: bash ParserError on command line execution (cmd/glob)
description: 处理 bash/command 失败时必读：ParserError 提示命令/语法解析错误
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

适用场景：PowerShell/cmd 执行报错 `ParserError`，提示命令/语法无法解析

操作步骤：
1) 读取完整 stderr → 确认解析失败位置（行号/字符）
2) 检查命令行结构：是否混入引号/转义不当；glob 模式是否正确闭合
3) 修复：使用结构化调用替代直接命令执行，或规范化字符串参数

边界与例外：错误中包含路径信息说明是测试环境命令执行问题，非一次性噪声；重复解析错误需记录 fp 标记用于复发检测

Failure marker: [fp:bash|> ParserError:] (来自 note 10)

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=true，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-132-bash-parsererror-on-command-line-executi.md)
