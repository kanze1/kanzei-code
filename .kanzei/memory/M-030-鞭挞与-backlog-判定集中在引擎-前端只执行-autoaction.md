---
id: M-030
scope: project
category: fact
title: 鞭挞与 backlog 判定集中在引擎，前端只执行 autoAction
description: 处理自动运行鞭挞、backlog 或继续文案改动时必读：判定逻辑只改 harness/kanzei-tools 单源，桌面端转发，前端仅执行 autoAction；不要在前端重复判定或维护旧继续文案。
status: active
created: 2026-08-10
updated: 2026-08-10
source: memory-manager
refs: R-169 R-170
---

R-169/R-170 落地后的架构事实：①鞭挞判定全在 `kanzei-harness/src/auto_run.rs`（`AutoRunState`/`AutoRunCtx`/`BacklogStatus`/`nudge_prompt`，12 个单测）；桌面端 `kz:done` 事件携带 `autoAction`，前端 `07-events.js` 只执行 `Continue`/`Nudge`/`Stop`/`NoContinue`，不判定。②backlog 判定单源为 `kanzei_tools::tracker::backlog_status`（三态单测）；`kanzei-app/src/auto_run.rs` 转发复用，CLI `main.rs` 轮末消费，消除 D-229 架构债。③继续文案默认为「继续推进，规则按系统提示执行。」；`LEGACY_CONTINUE_PROMPTS`、`applyCadenceSettings`、`cadenceVerificationText`、开发重心拼接已删除，规则归 system prompt 与 harness。D-241 仅为只含标题的残缺观察条目，待补正文。
