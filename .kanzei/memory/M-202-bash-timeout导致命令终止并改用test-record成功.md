---
id: M-202
scope: project
category: fact
title: bash timeout导致命令终止并改用test_record成功
description: 处理 bash/timeout类任务时必读——识别可复用错误模式与一次性噪声的关键标准
status: active
created: 2026-08-17
updated: 2026-08-17
source: user
subject: bash超时失败
---

错误现象：bash命令超时终止 (timeout: true — command did not finish within 600000 ms and was killed)
纠正措施：改用 test_record 执行并成功
复发情况：第3次(跨轮计数)，判定为环境/工具契约类的可复用知识

判断标准：同一错误模式跨轮复发且带修复成功证据 → 建条目；仅为本次意外无复用价值 → NOOP。
适用场景：bash执行超时或被kill时，应先尝试用test_record替代
