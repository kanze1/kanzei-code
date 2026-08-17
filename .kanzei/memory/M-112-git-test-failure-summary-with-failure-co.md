---
id: M-112
scope: project
category: fact
title: Git test failure summary with failure count - 处理failures:标记显示重复失败测试时的跨轮排查 — 检查前置依赖和测试环境一致性
description: 何时遇到 failures: git::tests 跨轮复发提示：检查测试前置条件与环境一致性
status: active
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

[fp:bash|failures:]
跨轮复发的测试失败总结模式。Cargo 测试输出末尾出现"failures:\n     git::tests::finalize_rejects_fmt_before_tests" 时，表示存在前置依赖缺失或测试环境异常导致多轮仍重复失败。
操作步骤：1.检查 test/fixtures/config.toml 是否配置了 missing field；2.核对 CI 环境变量 RUST_BACKTRACE=1 和 TEST_THREADS 设置；3.验证 .kanzei/test_record.json 中对应 test_entry_id 是否存在前置条件记录。边界：若仅单轮失败且无重复 pattern，不套用本 SOP。
