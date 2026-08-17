---
id: M-106
scope: project
category: fact
title: bash edit|- assert!(失败:插入误做替换,需检查 allow_deletion 标志
description: 处理 bash+edit 插入失败疑似执行为替换操作 — 新文覆盖旧文时必读
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

bash+edit 插入失败疑似执行为替换操作，new_string 多了 5 行却顶掉了 old_string 内容（如 old_text + new_text 被直接覆盖），断言 `assert!()` 失败。判据：检测到 "[fp:edit|- assert!(]" 或 "new_string 新增加 N 行但目标内容未保留"时触发。操作步骤：检查 `edit` 的 `allow_deletion=true` 标志——该标志控制是否允许从文件中删除旧行；若为 false 则执行 insert-on-match 逻辑（多出新行），应置 true 以覆盖匹配到的文本。确认 target lines should be copied to new_string 而不替换它们。例外：确实要删掉整段内容时保留默认行为。关键教训：默认 edit 是"insert-or-ignore-on-error"而非"replace-on-error"，用户常误以为会覆盖旧行。[fp:edit|- assert!()] — 复发检测键
