---
id: M-130
scope: project
category: sop
title: [edit] 误插替删须精准构造 new_string — [fp:edit|- assert!(]记录缺失行片
description: 处理 edit 误插替删 — [fp:edit|- assert!(]记录缺失原文，强调 insert vs delete semantics + whitespace precision
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

编辑时误把 insert 当 replace/删除整段 — old_string match 到多行后被替换成插入结果中多余行消失；或 new_string 未保留原文。根因：操作 semantics misinterpretation(新文本覆盖旧 vs 追加) + text diff analysis failure(未对照 old/new 逐行核对)。复发关键标记：[fp:edit|- assert!(]，记录本次具体缺少的原文片段(如 // 默认档位已退出… / assert!… / get(&default_id).map…)。处置序列：1) read 重读文件定位上下文边界 2) 明确 intended operation(insert/delete/replace) 3) 构造 new_string 精准包含要保留的原文片段 + 新内容 4) allow_deletion仅在确要删除时才设 true；5) 用 line-by-line diff 校对。
