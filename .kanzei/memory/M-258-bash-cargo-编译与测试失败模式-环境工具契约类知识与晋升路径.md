---
id: M-258
scope: project
category: fact
title: bash/cargo失败模式：先核对完整结束标记与实际失败行再定位根因
description: 处理 bash/cargo 输出局部显示测试通过但命令 exit code 非零或后续有 Diff 时必读：不要被 `test result: ok` 误导；核对完整输出、退出码、结束标记及实际失败行（尤其摘要之后的 diff/断言失败），再定位根因；未定位前不盲目重试或改代码。
status: active
created: 2026-08-19
updated: 2026-09-27
source: user
refs: R-070 R-085 D-204 R-092 D-210 R-295
---

决策规则：bash/cargo 测试输出中，`test result: ok` 片段不能单独证明命令成功；若完整输出显示 `exit code: 1`，或结束标记/实际失败行/退出码互相不一致，必须先保留并核对完整 stdout/stderr，定位真实失败测试，再判断根因和后续动作。不要因局部 ok、截断输出或单个 exit code 直接判定成功、重试或改代码。

本轮复发证据：`exit code: 1`，输出含 `running 2 tests`、两条测试行显示 `... ok`，随后出现截断的 `test result: ok. 2 pa...`；该片段不足以确认完整结束状态。

历史复发标记（必须保留）：
[fp:bash|Compiling thiserror v..]
[fp:bash|assert_eq!(report.deprecated, low_value_ids);]
[fp:bash|error: unexpected closing delimiter:]
[fp:bash|permission denied by guard : is blocked: whole-file rewrites via shell bypass th]
[fp:bash|test conversation::tests::latest_segment_recovers_completed_compaction_surface .]
[fp:req|R- 的手写批次是 Git 提交历史标记数为 ;请先核对并更新批次字段后再关闭。]
[fp:work|permission denied by ruleset: work on .]
[fp:write|permission denied by ruleset: write to .kanzei/memory]
[fp:bash|test result: ok. passed; failed; ignored; measured; filtered out; finished in .s]
