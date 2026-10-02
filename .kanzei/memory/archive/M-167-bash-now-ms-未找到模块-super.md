---
id: M-167
scope: project
category: fact
title: bash now_ms 未找到模块 super
description: 处理 bash now_ms 未找到模块错误时必读：如何判断是命名空间冲突还是一次性误报
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
refs: R-042
---

[fp:bash|error[E]: cannot find function in module] (from inbox)

1. 触发场景：cargo build 报 "cannot find function `now_ms` in module `super`" 时
2. 判据：检查模块 super 是否存在于 crate 顶层命名空间，或是否从 extern crate 导出
3. 解决：确认 now_ms 函数正确置于 pub fn 且作用域不受 trait/const 影响

适用场景：Cargo build 失败且错误提示找不到函数在指定模块时
操作步骤:
- work + read：定位 crates/kanzei-memory/src/memory/lifecycle.rs:224 行代码上下文
- bash：执行 `cargo update -p kanzei-memory && cargo clean && cargo build`判断依据：强制刷新依赖后再次编译验证修复效果
- test_record：运行完整 test suite 确保修复不引入 regressions（judge by test_report=pass）
- grep：搜索 project-wide now_ms usage points，确认定义与调用位置匹配（match count≠0, no shadowing warning）
- defect：若仍报错则生成缺陷报告定位命名空间冲突源（defect.json contains namespace_incompatibility tag）
- req：检查是否需求变更导致依赖版本变化而非代码问题（req ID 需在现有 valid list 中可映射）
- collaboration_status：确认多方协作中无分支合并导致的命名冲突（status=merge_conflict → 执行 git revert）
- git：修复后 commit 并回滚至上一级稳定态验证恢复方案有效（commit diff shows only namespace_fix, log traces cleanly）

例外:若编译期找不到函数但运行时报错——此为环境/运行时约束，不纳入本 SOP 范围

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=true，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-167-bash-now-ms-未找到模块-super.md)
