---
id: M-258
scope: project
category: fact
title: bash/cargo失败模式 SOP — 特定 test case/compile阻塞复用路径
description: 处理 bash 被 guard 拒绝整文件改写/Set-Content 时必读：改用 edit 做定向修改并先确认目标文本；不要用 shell 绕过 edit/write 的语法校验与 diff 展示，避免重复触发 full-file-write 拒绝。
status: active
created: 2026-08-19
updated: 2026-08-20
source: user
refs: R-070 R-085 D-204 R-092 D-210 R-295
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
