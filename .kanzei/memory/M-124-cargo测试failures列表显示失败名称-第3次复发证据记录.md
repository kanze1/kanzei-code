---
id: M-124
scope: project
category: fact
title: cargo测试failures列表显示失败名称 — 第3次复发证据记录
description: cargo测试失败failures列表显示 — 第3次复发且有成功修复证据，待升active
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
refs: D-204 R-165
---

错误原文：exit code: 1 failures: git::tests::finalize_rejects_fmt_before_tests test run failed

判断：这是构建/测试流程失败（非编译报错），属工具链契约类知识，可复用。

操作步骤：
1️⃣识别上下文：pytest/cargo-test框架下失败信息输出
2️⃣检查失败原因：failures列表指出具体测试名称
3️⃣修复策略：针对该测试单独调试或修改其依赖逻辑

边界与例外：
- 若同一类测试反复失败但 root 未变，可建立 SOP 模板。
- 本例仅记录一次失败信息，暂不作为 SOP 固化，待复发2次以上再升active。

Refs: D-204 (SOP结构), R-165(provenance硬约束)
