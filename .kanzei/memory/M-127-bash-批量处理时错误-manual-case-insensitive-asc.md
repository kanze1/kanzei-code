---
id: M-127
scope: project
category: fact
title: bash 批量处理时错误：manual case-insensitive ASCII comparison — 先统一 locale 再比较
description: 何时遇到 bash 批量处理时的手动 ASCII 大小写比较报错：检查 trim/to_ascii_lowercase 使用场景
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
subject: bash 多批处理 ASCII 大小写比较
---

[fp:bash|> error: manual case-insensitive ASCII comparison]
环境/工具契约类知识，跨项目复发性模式。批量执行 Shell 脚本时报错：手动进行的小写 ASCII 比较逻辑在 Windows bash 下与 POSIX locale 不同步导致失败。
操作步骤：1.检查脚本是否显式设置 LANG/C/LC_ALL=en_US.UTF-8；2.改用 strcasecmp() / LC_COLLATE=C;3.直接字符串通配化而非逐字符比较。边界：仅当多文件批量处理触发此错误才用本 SOP；单文件交互式命令无需套用。
