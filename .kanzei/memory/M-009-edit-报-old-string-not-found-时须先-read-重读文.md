---
id: M-009
scope: project
category: sop
title: edit 报 old_string not found 时须先 read 重读文件再精确匹配
description: 处理 edit 替换失败(old_string not found / must match exactly including whitespace)时必读:先 read 重读磁盘实际内容再构造 old_string
status: active
created: 2026-08-07
updated: 2026-08-07
source: inbox 2026-08-07
---

错误原文: `old_string not found in <path> — it must match exactly, including whitespace.`

实例(2026-08-07):对 scripts/ui-runtime-smoke.mjs 执行 edit 时因 old_string 与磁盘内容不一致而失败;工具提示最接近行 `if (!source.includes('if (isActivityTool(e.payload.name)) bgAdd')) {`。改用 read 重读该文件、按实际内容重建 old_string 后 edit 成功。

处置 SOP:
1. edit 报 old_string not found 时,不要用记忆中的内容重试——文件可能已被外部(格式化工具、其他会话、生成脚本)修改。
2. 先 read 目标文件的相关区段,复制磁盘上的确切文本(含缩进/引号/换行)作为 old_string 再 edit。
3. 注意失败计数本身不是知识;约束是"old_string 必须与磁盘逐字节一致"。
