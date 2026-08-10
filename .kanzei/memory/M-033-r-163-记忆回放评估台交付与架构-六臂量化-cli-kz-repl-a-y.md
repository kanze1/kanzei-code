---
id: M-033
scope: project
category: fact
title: R-163 记忆回放评估台交付与架构：六臂量化 CLI kz repl a y-eval
description: 处理 R-163/ReplayDecider/MemoryContextProvider/KzReplayEval 四批交付特征、显式执行模型与判据实现时必读
status: candidate
created: 2026-08-10
updated: 2026-08-10
source: memory-manager
refs: R-163
---

R-163 记忆回放评估台已交付:六臂对照量化记忆决策价值,CLI `kz replay-eval`可重复执行。

①replay.rs 回放数据层(parse_trace_payload按id配对 run.trace tool.started/completed + recorded_tool_results不真执行工具)。
②Arm六臂枚举+MemoryContextProvider/ReplayDecider trait+run_arms落 memory_eval。
③J判据 score_decision(has_action/repeats_failed_tool/retry/tokens)+ render_report(NoMemory→Current增量/Current→Oracle上界差距)。
④kanzei-tools/replay_eval.rs(ReplayMemoryProvider接FailureRecallPolicy+LlmDecider真调) + kz replay-eval [--limit N] CLI。commit 028307a/48e3634/6d5fc8f/3e61663/19a02db。

ReplayDecider用显式 BoxFuture而非async_trait(core不加主依赖);MemoryContextProvider接收&ReplayCase(Oracle臂要case才能自动合成事后正确做法)。
