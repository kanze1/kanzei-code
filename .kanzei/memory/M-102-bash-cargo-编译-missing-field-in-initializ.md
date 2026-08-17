---
id: M-102
scope: project
category: fact
title: bash/cargo 编译 missing field in initializer of 类型报错必读
description: 处理 Structured init 字段缺失的编译器报错 — 初始化构造时必填域缺失导致编译失败,跨测试文件复发
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

处理 bash 重试中 "error[E]: missing field ... in initializer of `SubagentRuntime`" 失败时必读：这是 crates/kanzei/tests/task_cancel_parallel.rs:167 处的环境契约故障,跨测试文件复发。第 2 次跨轮计数失败后触发。保留指纹：[fp:bash|> error[E]: missing field in initializer of]。相关类型定义需补充字段或在编译时报错处显式构造。(refs: )
