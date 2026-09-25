---
id: M-009
scope: project
category: sop
title: edit 报 old_string not found + must match exactly:先重读再精确构造含 whitespace 与缩进—非唯一匹配勿设 replace_all
description: 处理 edit 报 old_string not found、尤其 bash/cargo 修改失败时必读：先 read 重读目标文件并核对实际换行、缩进和空白，再构造与原文逐字符匹配的 old_string；多处匹配时不要盲用 replace_all。
status: active
created: 2026-08-07
updated: 2026-09-25
source: inbox 2026-08-07;2026-08-13 自 quarantine 原版恢复
<span class="highlight">[fp: edit|old_string not found in — it must match exactly, including whitespace.]</span>
---

适用场景：edit 报 old_string not found，或 bash/cargo 修改返回 Diff/exit code 1。
操作步骤：1. 先 read 重读目标文件，依据当前实际内容、换行、缩进和空白重新定位目标；2. 精确构造 old_string，必须与文件逐字符匹配（match exactly including whitespace）；3. 目标多处出现时先缩小上下文或改用唯一定位，不要盲用 replace_all；4. 修改后重新检查 diff。
边界与例外：这是文本匹配失败的处理规则，不适用于预期中的测试失败或编译错误；若文件已发生并发/前序改动，必须以最新 read 内容为准。
复发指纹：[fp:edit|old_string matches locations in make it unique with more context, or set replace]
[fp:edit|old_string not found in — it must match exactly, including whitespace.]
[fp:bash|行动: 处理 edit old_string not found 时必读:先 read 重读文件排版再精确构造—match exactly including ]
