---
id: M-153
scope: project
category: fact
title: memory_stale 失败模式：未知 ID与 malformed disk error
description: 处理 memory_stale("unknown memory id"/"malformed disk") 失败时必读：先 verify ID exist，再记录真实错误而非编造 FP
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: user:R-216验收3次轮回
---

【R-216 验收】M-037 stale 执行失败 - 
- Step1 memory_search(M-037): (no memory matched) → ID 不存在或未注册; 说明 M-037 并未作为 project entry 存在，可能是已删除、从未入库或 ID 格式错误。
- Step2 memory_stale(M-037): "unknown memory id M-037" → 确认内存不存在。
- Step3 database disk image is malformed → SQLite底层存储异常（可能因并发访问或非正规写入导致）。
【结论】当执行 memory_stale 收到 "unknown memory id" 或 "malformed disk" 时：
  (1) 先通过 memory_search 确认 ID 是否真实存在;
  (2) 若不存在，不可强行退役，应记录此 failure 模式供下次避免错用 ID。
