# 巨石度量基线快照(R-258 批2)

来源:`kz metrics --top 30`(2026-08-18,R-300 B1 复跑实测)。
口径:`crates/kanzei/src/cli/metrics.rs`——生产行数 = 总行数 − cfg(test) 块行数
(cfg(test) 块按大括号配平识别,外挂声明 `#[cfg(test)] mod x;` 不算测试块;
`_tests.rs` 后缀与 `tests/` 目录的外挂测试文件整文件算测试行);函数度量只统计生产码;
参数 > 7 沿用 clippy too_many_arguments 默认阈值。
阈值(conventions §9.2):生产行数 > 1200 巨石;参数 > 7 函数 ≥ 4 处失控;最大函数 > 400 行。

## Top-30 榜单(按生产行数降序)

| # | 文件 | 总行 | 生产 | 测试 | 函数 | 最大fn | >7参 |
|---|---|---|---|---|---|---|---|
| 1 | crates/kanzei-tools/src/background.rs | 2113 | 2091 | 22 | 76 | 130 | 1 |
| 2 | crates/kanzei-core/src/runner/drive.rs | 2158 | 1910 | 248 | 15 | 526 | 6 |
| 3 | crates/kanzei-core/src/store/typed.rs | 2839 | 1630 | 1209 | 51 | 210 | 1 |
| 4 | crates/kanzei-memory/src/docstore.rs | 2710 | 1526 | 1184 | 48 | 73 | 0 |
| 5 | crates/kanzei-tools/src/git.rs | 2777 | 1435 | 1342 | 40 | 148 | 0 |
| 6 | crates/kanzei-tools/src/tracker/actions.rs | 1356 | 1356 | 0 | 27 | 243 | 0 |
| 7 | crates/kanzei-tools/src/test_record.rs | 2550 | 1319 | 1231 | 43 | 107 | 3 |
| 8 | crates/kanzei-memory/src/memory/mod.rs | 2612 | 1259 | 1353 | 46 | 91 | 0 |
| 9 | crates/kanzei-harness/src/config.rs | 2937 | 1220 | 1717 | 45 | 104 | 1 |
| 10 | crates/kanzei-tools/src/work.rs | 1787 | 1118 | 669 | 25 | 257 | 1 |
| 11 | crates/kanzei-tools/src/profiles.rs | 2025 | 948 | 1077 | 6 | 532 | 0 |
| 12 | crates/kanzei-app/src/phase_pipeline.rs | 933 | 923 | 10 | 33 | 82 | 1 |
| 13 | crates/kanzei-core/src/phase.rs | 974 | 910 | 64 | 37 | 293 | 0 |
| 14 | crates/kanzei-core/src/runner/subagent.rs | 1136 | 909 | 227 | 18 | 413 | 0 |
| 15 | crates/kanzei-memory/src/memory/store.rs | 3120 | 891 | 2229 | 25 | 121 | 2 |
| 16 | crates/kanzei-app/src/docs.rs | 889 | 872 | 17 | 22 | 203 | 2 |
| 17 | crates/kanzei-tools/src/tracker.rs | 3854 | 868 | 2986 | 22 | 209 | 0 |
| 18 | crates/kanzei-app/src/settings.rs | 1528 | 858 | 670 | 25 | 107 | 0 |
| 19 | crates/kanzei-memory/src/memory/index.rs | 1542 | 824 | 718 | 35 | 82 | 0 |
| 20 | crates/kanzei-app/src/state.rs | 952 | 823 | 129 | 31 | 74 | 0 |
| 21 | crates/kanzei-app/src/mobile.rs | 1229 | 807 | 422 | 19 | 178 | 1 |
| 22 | crates/kanzei-app/src/run/assembly.rs | 879 | 789 | 90 | 15 | 377 | 0 |
| 23 | crates/kanzei-base/src/atomic_file.rs | 1198 | 757 | 441 | 22 | 102 | 0 |
| 24 | crates/kanzei-harness/src/auto_run.rs | 772 | 753 | 19 | 30 | 66 | 0 |
| 25 | crates/kanzei-core/src/replay.rs | 805 | 746 | 59 | 24 | 300 | 1 |
| 26 | crates/kanzei-tools/src/symbols.rs | 1111 | 724 | 387 | 12 | 126 | 0 |
| 27 | crates/kanzei-llm/src/protocol/openai.rs | 775 | 704 | 71 | 17 | 131 | 0 |
| 28 | crates/kanzei-harness/src/orchestration.rs | 985 | 695 | 290 | 26 | 113 | 1 |
| 29 | crates/kanzei-tools/src/bash.rs | 1371 | 691 | 680 | 24 | 261 | 0 |
| 30 | crates/kanzei/src/cli/run.rs | 678 | 678 | 0 | 1 | 652 | 0 |

## 读数

- 全仓 210 个 .rs 文件;生产行数 > 1200 的巨石 9 个(前 9 名)。
- 相比 2026-08-16 基线,background.rs 生产行 +72、drive.rs +58、typed.rs +283、docstore.rs +58、git.rs +136、tracker/actions.rs +127、test_record.rs +58、memory/mod.rs +41;这些回涨目标进入 R-300 后续拆解批次。
- 参数 > 7 函数 ≥ 4 处的文件 1 个:`drive.rs`(6 处)。
- 最大函数 > 400 行:drive.rs(526)、profiles.rs(532)、cli/run.rs(652);cli/run.rs 仍为单函数文件且无测试,列为后续拆解目标。
- 对照基准:tracker.rs 生产 868 行(总 3854);raw 行数会把测试/实现混合误诊,本快照继续使用生产行口径。
- 本次快照只覆盖 Rust 度量;前端回涨文件与 `06-agent-panel.js`/`06-activity.js` 合流在 R-300 B3 处理。

## 用途

后续拆解批次以本快照为前后对照基线:拆解后重跑 `kz metrics`,同一文件的生产行数应下降,回涨文件不得超过基线而无说明,巨石数量应减少。
