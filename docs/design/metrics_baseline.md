# 巨石度量基线

metrics_format_version: v1

基线观测日期为 2026-10-17。生产行、测试行和函数的口径由
`crates/kanzei/src/cli/metrics.rs` 定义；本表不是已审查文件清单，也不证明测试已执行。
机器门禁读取 `scripts/metrics-baseline.json`，逐个核对全仓文件，拒绝新增巨石。原有文件的 100 行预算保持既有上限；基线修改必须作为独立审查项。发布叙述保存在 Git 历史和对应发布回执中，不追加进度量数据。

| # | 文件 | 总行 | 生产 | 测试 | 函数 | 最大fn | >7参 |
|---|---|---:|---:|---:|---:|---:|---:|
| 1 | crates/kanzei-tools/src/team/mod.rs | 1674 | 1674 | 0 | 61 | 290 | 0 |
| 2 | crates/kanzei-memory/src/memory/mod.rs | 3084 | 1582 | 1502 | 55 | 105 | 1 |
| 3 | crates/kanzei-core/src/store/typed.rs | 3748 | 1558 | 2190 | 48 | 226 | 0 |
| 4 | crates/kanzei-core/src/runner/drive.rs | 2015 | 1359 | 656 | 9 | 298 | 4 |
| 5 | crates/kanzei-tools/src/work.rs | 2911 | 1309 | 1602 | 24 | 2127 | 1 |
| 6 | crates/kanzei-tools/src/research_runner.rs | 1916 | 1299 | 617 | 32 | 625 | 2 |
| 7 | crates/kanzei-tools/src/git.rs | 3042 | 1197 | 1845 | 28 | 129 | 0 |
| 8 | crates/kanzei-core/src/store/session.rs | 1899 | 1195 | 704 | 42 | 107 | 0 |
| 9 | crates/kanzei-app/src/commands/run.rs | 1541 | 1160 | 381 | 25 | 51 | 0 |
| 10 | crates/kanzei-tools/src/tracker.rs | 5592 | 1141 | 4451 | 29 | 213 | 0 |
| 11 | crates/kanzei-memory/src/memory/store.rs | 3819 | 1126 | 2693 | 32 | 129 | 4 |
| 12 | crates/kanzei-app/src/run/events/mod.rs | 1760 | 1121 | 639 | 36 | 307 | 2 |
| 13 | crates/kanzei-tools/src/symbols.rs | 1732 | 1037 | 695 | 20 | 162 | 0 |
| 14 | crates/kanzei-core/src/runner/subagent.rs | 1435 | 1018 | 417 | 23 | 458 | 0 |
| 15 | crates/kanzei-tools/src/tracker/actions.rs | 995 | 995 | 0 | 10 | 534 | 0 |
| 16 | crates/kanzei-app/src/settings.rs | 1975 | 983 | 992 | 29 | 117 | 0 |
| 17 | crates/kanzei-core/src/research.rs | 1089 | 969 | 120 | 26 | 169 | 1 |
| 18 | crates/kanzei-app/src/processes/lifecycle.rs | 1501 | 950 | 551 | 25 | 132 | 2 |
| 19 | crates/kanzei-tools/src/test_record.rs | 2284 | 947 | 1337 | 26 | 107 | 3 |
| 20 | crates/kanzei-app/src/preview/pane.rs | 1008 | 941 | 67 | 45 | 60 | 0 |
| 21 | crates/kanzei-app/src/run/assembly.rs | 1744 | 925 | 819 | 17 | 300 | 2 |
| 22 | crates/kanzei-harness/src/permission.rs | 1396 | 923 | 473 | 45 | 149 | 0 |
| 23 | crates/kanzei-llm/src/protocol/anthropic.rs | 1177 | 911 | 266 | 21 | 268 | 0 |
| 24 | crates/kanzei-app/src/docs.rs | 1073 | 910 | 163 | 25 | 277 | 2 |
| 25 | crates/kanzei-app/src/conversation.rs | 1038 | 900 | 138 | 27 | 69 | 0 |
| 26 | crates/kanzei-tools/src/tracker/scheduling.rs | 1142 | 888 | 254 | 40 | 55 | 1 |
| 27 | crates/kanzei-app/src/mobile.rs | 2323 | 886 | 1437 | 25 | 261 | 2 |
| 28 | crates/kanzei-app/src/state.rs | 1041 | 886 | 155 | 36 | 76 | 0 |
| 29 | crates/kanzei-tools/src/webfetch.rs | 1345 | 877 | 468 | 27 | 651 | 0 |
| 30 | crates/kanzei-app/src/projects.rs | 1314 | 869 | 445 | 37 | 56 | 0 |
