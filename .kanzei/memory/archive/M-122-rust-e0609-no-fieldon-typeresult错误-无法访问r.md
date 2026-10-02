---
id: M-122
scope: project
category: fact
title: Rust[E0609]no fieldon typeResult错误：无法访问Result类型未定义的字段
description: Rust 编译报错 "error[E0609]: no field on type Result" — 第3次复发且有修复成功证据，需升为 active
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
refs: D-204 R-165
subject: git_result_status_access_pattern
---

错误原文: exit code: 1 > error[E0609]: no field `status` on type `Result<Output, std::io::Error>`
--> crates\kanzei-tools\src\git.rs:838:16

判断：这是Rust工具链的契约错误，属环境/工具知识（非一次性噪声）。

操作步骤：
1️⃣ 识别类型：输出类型为 `Result<Output, std::io::Error>`
2️⃣ 检查方法调用：`if !output.status.success()` — `status` 不在外层 Result 上，而在内部
3️⃣修复：将 `.status`移到`Ok(output)`分支内访问 → `match output { Ok(o) if !o.status.success() => ..., Err(e) => ... }`

边界与例外：
- 若后续改为直接 unwrapResult()后取.status()可规避匹配复杂度，但易丢失错误信息。
- 本例中因输出本身为 `Result`类型而报错，属于典型Rust模式误用。

Refs: D-204 SOP结构要求，R-165 provenance硬约束

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-122-rust-e0609-no-fieldon-typeresult错误-无法访问r.md)
