---
id: M-035
scope: project
category: fact
title: R-164 replay-eval 评估台交付与 R-178 manual_models已关闭事实
description: 处理 R-164/kz replay-eval 回放/评判工具调用时必读｜复现四批交付架构与判据细节
status: deprecated
created: 2026-08-11
updated: 2026-08-13
source: memory-manager
refs: R-163 R-178
subject: replay_eval/req_178_status
superseded_by: M-032
---

> 墓碑(2026-08-13):库存合并——R-163 交付事实与 M-032(已并 M-033/M-036)逐句重复,R-178 查重教训与 M-037 重复。
R-163 记忆回放评估台已交付：六臂对照量化记忆决策价值，CLI `kz replay-eval`可重复执行。

四批交付要点:
①replay.rs回放数据层(parse_trace_payload按id配对run.trace tool.started/completed+recorded_tool_results不真执工具);
②Arm 六臂枚举+MemoryContextProvider/ReplayDecider trait + run_arms落memory_eval;
③J判据 score_decision(has_action/repeats_failed_tool/retry/tokens)+ render_report(NoMemory→Current增量/Current→Oracle上界差距)；  
④kanzei-tools/replay_eval.rs(ReplayMemoryProvider接FailureRecallPolicy+LlmDecider真调)+kz replay-eval [--limit N] CLI。

commit 序列：028307a→48e3634→6d5fc8f→3e61663→19a02db
关键实现细节:ReplayDecider用BoxFuture替代async_trait(core无需std依赖)；MemoryContextProvider接收&ReplayCase(Oracle臂需case才能自动合成事后正确做法)。

R-178 已在 dev 完整关闭(processes.manual_models列方案),thread-line工作树重复实现已弃并。教训:动工前先git log查目标是否已被其他线交付——manual_models是表一列,非独立表。
