---
id: M-201
scope: project
category: fact
title: bash命令超时与test_record模式
description: 处理bash工具超时失败或test_record替代方案:遇到timeout后改用test_record
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
subject: bash_tool_timeout
---

错误触发:bash命令超时(60s限制)->降级使用test_record\n修复证据:第3次复发且测试记录已成功执行，episode_id=734\n适用场景 bash 命令超时失败(跨轮≥3次)->改用 test_record\n边界:一次性TDD预期失败或自纠错NOOP
