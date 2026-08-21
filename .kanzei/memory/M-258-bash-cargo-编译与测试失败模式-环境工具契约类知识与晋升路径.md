---
id: M-258
scope: project
category: fact
title: bash/cargo失败模式：先核对结束标记与目标配置再定位根因
description: 处理 bash/cargo 测试或编译输出异常、尤其 exit code=0 但测试结果与 stderr 混排或输出被截断时必读：先读取完整 `test result:` 结束行，按 `failed` 计数和失败测试名判定是否真的失败；本例 `1 passed; 0 failed` 应判为通过/输出截断，不因首个 exit code、`ok` 或 shell 重试继续排查。确认真实失败后，再核对 stderr 原文、Cargo target 与命令配置，read/grep 上下文后定向修复。
status: active
created: 2026-08-19
updated: 2026-08-21
source: user
refs: R-070 R-085 D-204 R-092 D-210 R-295
---

适用场景：bash/cargo 测试或编译出现阻塞、失败，或输出看似成功但未给出完整结束结果时。
操作步骤：先读取完整 stdout/stderr，确认是否有明确的测试完成摘要；若输出混合通过/失败、被截断，或仅显示 exit code 0 仍处于 running 状态，不得判定成功，先定位失败测试名和 stderr 根因。再用 read/grep 核对目标文件与上下文，用 edit 定向修复，最后重跑对应 cargo test；只有完整结束且结果符合预期才算通过。不要把问题当作 shell 重试，也不要用 shell 全文件改写。
复发判据：本条已出现同类 bash 输出异常，必须优先检查“exit code: 0  running ...”是否只是未完成/截断输出，而不是重复执行命令。
[fp:bash|test result: ok. passed; failed; ignored; measured; filtered out; finished in .s]
[fp:bash|Compiling thiserror v..]
[fp:bash|assert_eq!(report.deprecated, low_value_ids);]
[fp:bash|error: unexpected closing delimiter:]
[fp:bash|permission denied by guard : is blocked: whole-file rewrites via shell bypass th]
[fp:bash|test conversation::tests::latest_segment_recovers_completed_compaction_surface .]
[fp:req|R- 的手写批次是 Git 提交历史标记数为 ;请先核对并更新批次字段后再关闭。]
[fp:work|permission denied by ruleset: work on .]
[fp:write|permission denied by ruleset: write to .kanzei/memory]
原始信号：exit code: 0  running 13 tests test symbols::tests::符号扫描_识别函数结构impl与可见性 ... ok test symbols::tests::符号扫描_关键字前缀的标识符不得误判为声明 ... ok test symbols::tests::符号扫描_处理泛型与pubcrate ... ok test symbols::tests::不
