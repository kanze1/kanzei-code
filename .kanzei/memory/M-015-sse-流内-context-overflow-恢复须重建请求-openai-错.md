---
id: M-015
scope: project
category: fact
title: SSE 流内 context overflow 恢复须重建请求,OpenAI 错误分类须同查 type/code
description: 调试 kanzei-core runner SSE 流内 context overflow 恢复、压缩后仍发超长历史、或 OpenAI context_length_exceeded 未被识别时必读
status: active
created: 2026-08-08
updated: 2026-08-13
source: inbox 2026-08-08
---

crates/kanzei-core/src/runner/(原 runner.rs 已拆分,恢复循环在 runner/mod.rs、压缩在 runner/compaction.rs):OpenAI/Anthropic/Responses 可在 HTTP 200 之后通过 SSE error 事件产生 LlmError::ContextOverflow,因此恢复逻辑不能只包住 stream_with_retry_notice(...).await 的返回——必须在消费 stream 的 stream_error 分支进入同一个有界恢复循环。且 LlmRequest 构造必须放进重试循环内部:否则 messages 虽被压缩,旧 request 的 clone 仍会发送原超长历史,压缩无效。回归测试:crates/kanzei/tests/context_overflow_recovery.rs。

另:OpenAI Chat 的 error.type=invalid_request_error 会遮蔽 error.code=context_length_exceeded,错误分类必须同时检查 type 和 code 两个字段;限流类 kind 判定优先于 overflow。相关: M-008(首请求 prior 历史清洗 filter_message_history)。
