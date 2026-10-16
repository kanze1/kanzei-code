---
id: M-292
scope: project
category: fact
title: CLI 与桌面运行装配共用一个能力入口
description: 修改工具目录、runner 限制或失败策略时核对公共 build 函数
status: active
created: 2026-10-07
updated: 2026-10-07
source: memory-manager
refs: crates/kanzei-tools/src/run.rs crates/kanzei/src/cli/run.rs crates/kanzei-app/src/run/assembly.rs
subject: contract:kanzei-tools/run:shared-entry
area: kanzei-tools/run kanzei/cli/run kanzei-app/run/assembly
---

build_harness 和 build_runner_config 在 kanzei-tools/run 为两个入口共享；入口提供 ResolveCtx、模型 route、身份、事件与桌面资源适配。FailureRecallPolicy、limits、profile 与 catalog 的共用行为不能只在 app 或 CLI 内修补；Tool/ToolOutput 改动同步所有具体工具与 runner。先定位公共装配，再核对各入口特有参数，公共共享不表示两个入口的所有环境和事件完全相同。

核验方式：按 refs 读取当前实现和消费者；图用于定位，不替代源码核验。
源码审计：docs/reports/2026-10-07-boundary-knowledge.md；外部审计 episode 1335。
