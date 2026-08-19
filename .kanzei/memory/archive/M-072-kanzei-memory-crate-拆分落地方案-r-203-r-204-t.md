---
id: M-072
scope: project
category: fact
title: kanzei-memory crate 拆分落地方案(R-203/R-204):tools 生产依赖零 core,workable_titles 调度链双份须同源演进
description: 改 memory/docstore/embed/replay_eval/scheduling 或 kanzei-tools 再导出路径时必读:crate 边界与双份调度链陷阱
status: deprecated
created: 2026-08-16
updated: 2026-08-18
source: inbox 2026-08-14
refs: R-203 R-204
---

2026-08-16 R-203 落地。crates/kanzei-memory 包含 memory(分级记忆)、docstore(文档引擎)、embed(向量通道)、replay_eval(六臂评估)、scheduling(workable_titles 调度链副本);kanzei-tools 经 lib.rs `pub use kanzei_memory::{memory,docstore,embed,replay_eval}` 再导出,外部调用点 kanzei_tools::memory::* 仍可用。kanzei-tools 生产依赖已无 kanzei-core(dev-deps 保留仅供 write.rs runner 集成测试)。workable_titles 调度链双份存在:kanzei-tools/src/tracker.rs(原版)+ kanzei-memory/src/scheduling.rs(逐字副本,供 prompt_hints 自主轮检索键),两份必须同源演进,R-204 抽独立调度模块时统一——只改一边会漂移。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-072-kanzei-memory-crate-拆分落地方案-r-203-r-204-t.md)
