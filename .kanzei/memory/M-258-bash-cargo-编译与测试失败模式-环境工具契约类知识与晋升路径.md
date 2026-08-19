---
id: M-258
scope: project
category: fact
title: bash/cargo失败模式 SOP — 特定 test case/compile阻塞复用路径
description: 处理 bash/cargo编译与测试失败：特定 test case/conversation::tests::latest_segment_recovers_completed_compaction_surface 复用;强化"判据补全→修正再重试"决策链
status: active
created: 2026-08-19
updated: 2026-08-19
source: user
---

【复发检测键】[fp:bash|test conversation::tests::latest_segment_recovers_completed_compaction_surface .] & [fp:bash|error: unexpected closing delimiter:] & [fp:req|R- 的手写批次是 Git 提交历史标记数为 ;请先核对并更新批次字段后再关闭。] & [fp:work|permission denied by ruleset: work on .] & [fp:write|permission denied by ruleset: write to .kanzei/memory] & [fp:bash|assert_eq!(report.deprecated, low_value_ids);] & [fp:bash|Compiling thiserror v..] & [fp:bash|test result: ok. passed; failed; ignored; measured; filtered out; finished in .s] & [fp:bash|permission denied by guard : is blocked: whole-file rewrites via shell bypass th]

【适用场景】cargo 编译/测试失败时必读，出现特定 test case 或编译过程报错。
- conversation::tests::latest_segment_recovers_completed_compaction_surface tests::FAILED (单次触发仍复发)
- assert_eq!(report.deprecated, low_value_ids);断言错别字(测试用例)
- Compiling proc-macro2/thiserror v..包依赖编译时阻塞(环境污染)

【操作步骤】
1.定位错误源：cargo 输出行号 + 文件名
2.判据判断：test case 失败需看 test result;编译阻塞需检查依赖链
3.修正:断言错别字直接改代码;依赖编译阻塞执行 cargo clean 

【已知坑位/边界】
- R-012类似场景:多{}嵌套的 proc-macro 扩展需逐行 trace
- 编译器缓存污染需先 cargo clean
