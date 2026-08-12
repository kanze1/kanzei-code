---
id: M-009
scope: project
category: sop
title: 编辑 old_string not found 时必须 read 重读并按实际空白重建
description: 处理 old_string 未匹配/空白错误时必读：按实际空白重建字符串，不可使用默认模板或假设性替换；旧版本 M-008/M-XXX 的泛化判据已失效
status: deprecated
created: 2026-08-07
updated: 2026-08-12
source: inbox 2026-08-07
---

错误原文：old_string not found in C:\Users\kanzei\Documents\kanzei code\krates/kanzei-core/src/store/events.rs — it must match exactly, including whitespace. Closest line: pub fn latest_event(`

核心判据编辑时 old_string 未匹配或空白异常的根本原因：1)editor tool 的解析钩子要求字符串逐字符精确匹配，任何不可见空格、换行符差异都会导致 not found；2)用户习惯性按"大概样子"输入或使用默认模板而非从源码重读重建。

决策流程修订版（替代旧通用判据）:
- Step1: 命中 before_replace_hook() →立即调用 read_file_at_line，获取该处源文件内容逐字核对
- Step2:old_string = file_content.split(line_num).nth(0)...提取原始值；若编辑器显示有错位/空白差异则说明已损坏的匹配尝试

触发条件扩展：any of {fp:"edit|old string exact match required including whitespace","reason":"字符串未找到"} → 执行 read_file_at_line then extract_exact_string，禁止使用模板或假设性拼接。

来源证据: [fp:edit|old_string not found in — it must match exactly, including whitespace.]
