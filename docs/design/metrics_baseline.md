# 巨石度量基线

metrics_format_version: v1

基线观测日期为 2026-10-24；本次在以 `ff40bbae` 为起点的双修复发布候选树运行 `kz metrics --top 30`。生产行、测试行和函数的口径由 `crates/kanzei/src/cli/metrics.rs` 定义；本表不是已审查文件清单，也不证明测试已执行。
机器门禁读取 `scripts/metrics-baseline.json`，逐个核对全仓文件，拒绝新增巨石。原有文件的 100 行预算保持既有上限；本次只更新观测表，不调整机器基线或阈值。发布叙述保存在 Git 历史和对应发布回执中。

| # | 文件 | 总行 | 生产 | 测试 | 函数 | 最大fn | >7参 |
|---|---|---:|---:|---:|---:|---:|---:|
| 1 | crates/kanzei-tools/src/team/mod.rs | 1728 | 1728 | 0 | 62 | 290 | 0 |
| 2 | crates/kanzei-core/src/store/typed.rs | 3806 | 1616 | 2190 | 50 | 226 | 0 |
| 3 | crates/kanzei-memory/src/memory/mod.rs | 3097 | 1596 | 1501 | 56 | 110 | 1 |
| 4 | crates/kanzei-core/src/runner/drive.rs | 2110 | 1454 | 656 | 9 | 322 | 4 |
| 5 | crates/kanzei-tools/src/work.rs | 3013 | 1396 | 1617 | 25 | 2185 | 2 |
| 6 | crates/kanzei-tools/src/research_runner.rs | 1919 | 1299 | 620 | 32 | 625 | 2 |
| 7 | crates/kanzei-core/src/store/session.rs | 1899 | 1195 | 704 | 42 | 107 | 0 |
| 8 | crates/kanzei-tools/src/git.rs | 3039 | 1176 | 1863 | 28 | 129 | 0 |
| 9 | crates/kanzei-tools/src/tracker.rs | 5624 | 1143 | 4481 | 29 | 213 | 0 |
| 10 | crates/kanzei-app/src/run/events/mod.rs | 1770 | 1131 | 639 | 36 | 317 | 2 |
| 11 | crates/kanzei-tools/src/work/tool.rs | 1125 | 1125 | 0 | 14 | 457 | 0 |
| 12 | crates/kanzei-tools/src/symbols.rs | 1793 | 1055 | 738 | 21 | 165 | 0 |
| 13 | crates/kanzei-core/src/runner/subagent.rs | 1418 | 1017 | 401 | 24 | 474 | 0 |
| 14 | crates/kanzei-memory/src/memory/store.rs | 3764 | 1012 | 2752 | 32 | 132 | 3 |
| 15 | crates/kanzei-tools/src/tracker/actions.rs | 1003 | 1003 | 0 | 10 | 542 | 0 |
| 16 | crates/kanzei-app/src/commands/run.rs | 1428 | 987 | 441 | 21 | 19 | 0 |
| 17 | crates/kanzei-tools/src/browser_tool.rs | 1314 | 976 | 338 | 49 | 135 | 0 |
| 18 | crates/kanzei-core/src/research.rs | 1089 | 969 | 120 | 26 | 169 | 1 |
| 19 | crates/kanzei-app/src/processes/lifecycle.rs | 1523 | 959 | 564 | 25 | 136 | 2 |
| 20 | crates/kanzei-app/src/settings.rs | 1923 | 958 | 965 | 28 | 117 | 0 |
| 21 | crates/kanzei-tools/src/test_record.rs | 2348 | 954 | 1394 | 26 | 107 | 3 |
| 22 | crates/kanzei-app/src/preview/pane.rs | 1006 | 939 | 67 | 45 | 60 | 0 |
| 23 | crates/kanzei-tools/src/read.rs | 1488 | 936 | 552 | 37 | 97 | 0 |
| 24 | crates/kanzei-core/src/store/work.rs | 1058 | 914 | 144 | 21 | 317 | 0 |
| 25 | crates/kanzei-app/src/docs.rs | 1076 | 913 | 163 | 25 | 277 | 2 |
| 26 | crates/kanzei-harness/src/permission.rs | 1389 | 911 | 478 | 45 | 149 | 0 |
| 27 | crates/kanzei-llm/src/protocol/anthropic.rs | 1177 | 911 | 266 | 21 | 268 | 0 |
| 28 | crates/kanzei-memory/src/memory/manager.rs | 1929 | 909 | 1020 | 42 | 118 | 0 |
| 29 | crates/kanzei-app/src/conversation.rs | 1038 | 900 | 138 | 27 | 69 | 0 |
| 30 | crates/kanzei-tools/src/shell.rs | 899 | 899 | 0 | 38 | 796 | 1 |

共 520 个 `.rs` 文件。榜单中的既有巨石不因本次修复宣称已经拆解。
