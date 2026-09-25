---
id: M-268
scope: project
category: fact
title: bash runner 混排输出且 exit code=1 时先核对完整块并改用 test_record
description: 处理 bash 测试输出出现 exit code: 1 且 running/ok 结果混排、尤其无法确认失败是否来自测试断言时必读：先读取并核对完整 stdout/stderr 与完整测试块；若只见截断/混排而缺少可归因失败证据，停止重试 bash，改用 test_record 记录验证结果并保留原始错误文本。
status: active
created: 2026-08-20
updated: 2026-09-25
source: user
---

适用场景：bash 执行批量测试时出现 `exit code: 1`，输出与 `running`/`ok` 多行混排或被截断，导致不能直接判断是代码测试失败还是 runner/环境契约问题。

决策判据与操作：
1. 先读取完整 stdout/stderr 和完整测试输出块，核对是否存在明确失败测试、断言错误或环境契约错误；不得仅凭末尾 `exit code: 1` 下结论。
2. 若输出仍是混排/截断且没有可归因的测试失败证据，视为 runner 记录不可靠：停止重复 bash，改用 `test_record` 记录本次验证，并保留原始错误文本与测试目标。
3. 只有在完整块明确显示代码测试失败时才进入修复流程；不要把 runner 的退出码本身当作代码失败。

本轮复发的原始错误片段：`exit code: 1  running 276 tests ... test agent_directory::tests::preview_is_bounded ... ok test`，该片段未提供完整失败归因。

[fp:bash|TypeError: Assignment to constant variable.]
[fp:bash|test runner::metrics::tests::failure_kind_多行bash批次_优先取pathspec根因行 ... ok]
[fp:bash|test agent_directory::tests::invalid_agent_frontmatter_is_visible_as_configurati]
[fp:bash|行动: 处理 runner 出现 exit code 与多行 混排，或同类失败复发时必读：即使 exit code 为 ，也先读取完整原始 ok 行直]
[fp:bash|行动: 处理 runner 出现 exit code 与多行 混排，或同类失败复发时必读：先完整读取 + 最终结束标记 + ok 带偏、直接重试或修改代码。]
[fp:bash|行动: 处理 runner 批量测试失败且输出混杂多行、尤其出现 pathspec 或路径错误时必读:先从 exit code 当成根因，也不要盲目重试。]
[fp:bash|行动: 处理 runner 批量测试失败且输出混杂多行、尤其出现 pathspec、路径错误或具体 stderr exit code test_record 获]
[fp:bash|行动: 处理 runner 批量测试输出含多行、exit code exit code、并行回归通过行或被截断的末尾摘要重试或改代码。]
[fp:bash|行动: 处理 runner 批量测试输出混杂多行、看到 exit code: 与大量 行时必读：先截取并核对完整 根因行及测试结束标记，再决定修复；不要把首个 ]
[fp:bash|行动: 处理 runner 输出出现 、running 进度与多行 混排，或同类失败再次复发时必读：即使 exit code 为 ，也先读取完整原始 ok 行直]
