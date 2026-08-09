---
id: M-022
scope: project
category: sop
title: 验证 Rust 测试必须用 test_record，禁止用 bash 跑 cargo test
description: 处理 Rust 测试验证时必读：不要因 bash 的 exit code 1 重跑或把它当代码失败；改用 test_record 记录/验证，并据其结果区分工具问题与代码问题。若 verify.ps1/格式检查也在 bash 中返回 exit code 1，仍先切换到结构化验证工具。
status: active
created: 2026-08-09
updated: 2026-08-09
source: inbox:2026-08-09
---

验证 Rust 测试必须用 test_record，禁止用 bash 跑 cargo test。处理 Rust 测试验证，尤其 bash 返回 `exit code: 1` 且无有效输出时，停止重跑 bash，把它视为工具契约问题；改用 test_record 记录/验证，并据其结果区分工具问题与代码问题。当前复发的具体错误：`exit code: 1 ==> fmt\r\n  [stderr] ... scripts/verify.ps1:20 ... if ($LASTEXITCODE -ne 0) { throw ...`，说明不要因 verify.ps1/fmt 在 bash 中失败而继续重复 bash 验证，应切换到 test_record 或相应结构化工具。
[fp:bash|exit code:]
