# 巨石度量基线快照(R-258 批2)

来源:`kz metrics --top 30`(2026-08-18,R-300 B4 复跑实测)。
口径:`crates/kanzei/src/cli/metrics.rs`——生产行数 = 总行数 − cfg(test) 块行数
(cfg(test) 块按大括号配平识别,外挂声明 `#[cfg(test)] mod x;` 不算测试块;
`_tests.rs` 后缀与 `tests/` 目录的外挂测试文件整文件算测试行);函数度量只统计生产码;
参数 > 7 沿用 clippy too_many_arguments 默认阈值。
阈值(conventions §9.2):生产行数 > 1200 巨石;参数 > 7 函数 ≥ 4 处失控;最大函数 > 400 行。

## Top-30 榜单(按生产行数降序, 2026-08-18 R-300 B4)

| # | 文件 | 总行 | 生产 | 测试 | 函数 | 最大fn | >7参 |
|---|---|---:|---:|---:|---:|---:|---:|
| 1 | crates/kanzei-tools/src/background.rs | 1953 | 1931 | 22 | 68 | 130 | 1 |
| 2 | crates/kanzei-core/src/runner/drive.rs | 2158 | 1910 | 248 | 15 | 526 | 6 |
| 3 | crates/kanzei-core/src/store/typed.rs | 2839 | 1630 | 1209 | 51 | 210 | 1 |
| 4 | crates/kanzei-memory/src/docstore.rs | 2710 | 1526 | 1184 | 48 | 73 | 0 |
| 5 | crates/kanzei-tools/src/git.rs | 2777 | 1435 | 1342 | 40 | 148 | 0 |
| 6 | crates/kanzei-memory/src/memory/mod.rs | 2612 | 1259 | 1353 | 46 | 91 | 0 |
| 7 | crates/kanzei-harness/src/config.rs | 2937 | 1220 | 1717 | 45 | 104 | 1 |
| 8 | crates/kanzei-tools/src/work.rs | 1787 | 1118 | 669 | 25 | 257 | 1 |
| 9 | crates/kanzei-tools/src/tracker/actions.rs | 1043 | 1043 | 0 | 15 | 243 | 0 |
| 10 | crates/kanzei-tools/src/profiles.rs | 2025 | 948 | 1077 | 6 | 532 | 0 |
| 11 | crates/kanzei-tools/src/test_record.rs | 2158 | 927 | 1231 | 26 | 107 | 3 |
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

- 全仓 213 个 .rs 文件;生产行数 > 1200 的巨石 9 个(前 9 名)。
- 相比 2026-08-18 B1 快照,background.rs 生产行 -160、tracker/actions.rs -313、test_record.rs -392;B4 的 background persistent 域拆分已在生产行口径中体现。
- 参数 > 7 函数 ≥ 4 处的文件 1 个:`drive.rs`(6 处)。
- 最大函数 > 400 行:drive.rs(526)、profiles.rs(532)、cli/run.rs(652);cli/run.rs 仍为单函数文件且无测试,列为后续拆解目标。
- 本次快照覆盖 Rust 度量;前端回涨文件与 `06-agent-panel.js`/`06-activity.js` 合流仍需后续专项证据。

## 回涨闸门

- `scripts/metrics-regression-gate.ps1` 由 `scripts/verify.ps1` 的 `crate_sync` 步骤真实调用。
- 对基线中仍出现在 Top-30 的文件,生产行允许最多比基线增加 100 行(宽松起步,防止测试/生成口径微调误伤);超过即失败。
- Top-30 中生产行超过 1200 的巨石数量允许最多比基线增加 1 个;超过即失败。
- 每次 B4 或后续拆解完成后先运行 `kz metrics --top 30` 更新本快照,再运行 gate;闸门只阻止未说明的明显回涨,不替代拆解条目。
