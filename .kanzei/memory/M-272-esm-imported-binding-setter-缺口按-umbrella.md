---
id: M-272
scope: project
category: sop
title: ESM imported binding setter 缺口按 umbrella 迁移族统一修复
description: 处理 classic global→ESM 迁移中消费者写入 imported binding、或 runtime 逐步暴露多个同类 setter 缺口时必读：按一个 umbrella defect 管理，先枚举全部跨模块 export let 写入并一次性修复验证，不按每个首个 symbol 重复登记 D 条目。
status: active
created: 2026-08-24
updated: 2026-08-24
source: memory-manager
refs: R-331
---

适用场景：runtime 迁移从 classic global 到 ESM 后，消费者尝试写入 imported binding，或逐步出现 chatAgentFolds、processItems、activeProcessId、ctxLimit、activePane、dependencyViewOpen、documentsKind、followLatest 等同类 ESM consumer assignment 问题；同时留意 on/defer 事件注册时序问题。

操作步骤：
1. 静态枚举所有跨模块 `export let` 写入及其消费者 assignment；判断依据：同一类“消费者不能写 imported binding”的缺口应进入同一证据清单，而不是按 runtime 首个暴露符号拆分。
2. 建立或保留一个 umbrella defect，并将发现的符号作为证据清单追加；判断依据：根因是 classic global→ESM 后 imported binding 不可写，符号是实例而非独立根因。
3. 一次性修复该迁移族的 setter/写入设计，并验证所有清单符号；判断依据：修复必须覆盖静态枚举结果，不能只修当前首个报错。
4. 检查 `on`/`defer` 事件注册时序并一并验证；判断依据：它是本迁移中可能并存的另一类时序缺口，不能误归为单个 symbol 的 setter 问题。
5. 对 D702/D703 等重复条目合并或标记为重复；判断依据：同一 umbrella 根因只保留一个缺陷记录。

边界与例外：若静态分析证明是不同根因或独立生命周期问题，才拆分 defect；仅新增一个 runtime 首个 imported symbol 不得创建新的同类 D 条目。
