# 巨石度量基线快照(R-258 批2)

来源:`cargo run -p kanzei -- metrics --top 30`（R-300 B14 后复跑；输出与用户安装位 `kz metrics --top 30` 一致）。
口径:`crates/kanzei/src/cli/metrics.rs`——生产行数 = 总行数 − cfg(test) 块行数
(cfg(test) 块按大括号配平识别,外挂声明 `#[cfg(test)] mod x;` 不算测试块;
`_tests.rs` 后缀与 `tests/` 目录的外挂测试文件整文件算测试行);函数度量只统计生产码;
参数 > 7 沿用 clippy too_many_arguments 默认阈值。
阈值(conventions §9.2):生产行数 > 1200 巨石;参数 > 7 函数 ≥ 4 处失控;最大函数 > 400 行。

## Top-30 榜单(按生产行数降序, R-300 B3 后复跑)

| # | 文件 | 总行 | 生产 | 测试 | 函数 | 最大fn | >7参 |
|---|---|---:|---:|---:|---:|---:|---:|
| 1 | crates/kanzei-core/src/store/typed.rs | 2839 | 1630 | 1209 | 51 | 210 | 1 |
| 2 | crates/kanzei-memory/src/docstore.rs | 2710 | 1526 | 1184 | 48 | 73 | 0 |
| 3 | crates/kanzei-tools/src/git.rs | 2780 | 1435 | 1345 | 40 | 148 | 0 |
| 4 | crates/kanzei-tools/src/background.rs | 1456 | 1431 | 25 | 49 | 68 | 0 |
| 5 | crates/kanzei-core/src/runner/drive.rs | 1521 | 1290 | 231 | 13 | 255 | 6 |
| 6 | crates/kanzei-memory/src/memory/mod.rs | 2616 | 1263 | 1353 | 46 | 91 | 0 |
| 7 | crates/kanzei-harness/src/config.rs | 2937 | 1220 | 1717 | 45 | 104 | 1 |
| 8 | crates/kanzei-tools/src/work.rs | 1787 | 1118 | 669 | 25 | 257 | 1 |
| 9 | crates/kanzei-memory/src/memory/store.rs | 3256 | 954 | 2302 | 26 | 121 | 2 |
| 10 | crates/kanzei-tools/src/test_record.rs | 2158 | 927 | 1231 | 26 | 107 | 3 |
| 11 | crates/kanzei-app/src/phase_pipeline.rs | 933 | 923 | 10 | 33 | 82 | 1 |
| 12 | crates/kanzei-core/src/phase.rs | 974 | 910 | 64 | 37 | 293 | 0 |
| 13 | crates/kanzei-core/src/runner/subagent.rs | 1136 | 909 | 227 | 18 | 413 | 0 |
| 14 | crates/kanzei-tools/src/tracker.rs | 3860 | 874 | 2986 | 22 | 215 | 0 |
| 15 | crates/kanzei-app/src/docs.rs | 889 | 872 | 17 | 22 | 203 | 2 |
| 16 | crates/kanzei-app/src/settings.rs | 1528 | 858 | 670 | 25 | 107 | 0 |
| 17 | crates/kanzei-memory/src/memory/index.rs | 1542 | 824 | 718 | 35 | 82 | 0 |
| 18 | crates/kanzei-app/src/state.rs | 952 | 823 | 129 | 31 | 74 | 0 |
| 19 | crates/kanzei-tools/src/tracker/actions.rs | 813 | 813 | 0 | 9 | 243 | 0 |
| 20 | crates/kanzei-app/src/mobile.rs | 1229 | 807 | 422 | 19 | 178 | 1 |
| 21 | crates/kanzei-app/src/run/assembly.rs | 879 | 789 | 90 | 15 | 377 | 0 |
| 22 | crates/kanzei-base/src/atomic_file.rs | 1198 | 757 | 441 | 22 | 102 | 0 |
| 23 | crates/kanzei-harness/src/auto_run.rs | 772 | 753 | 19 | 30 | 66 | 0 |
| 24 | crates/kanzei-core/src/replay.rs | 805 | 746 | 59 | 24 | 300 | 1 |
| 25 | crates/kanzei-tools/src/symbols.rs | 1111 | 724 | 387 | 12 | 126 | 0 |
| 26 | crates/kanzei-llm/src/protocol/openai.rs | 775 | 704 | 71 | 17 | 131 | 0 |
| 27 | crates/kanzei-harness/src/orchestration.rs | 985 | 695 | 290 | 26 | 113 | 1 |
| 28 | crates/kanzei-tools/src/bash.rs | 1371 | 691 | 680 | 24 | 261 | 0 |
| 29 | crates/kanzei-harness/src/permission.rs | 1148 | 675 | 473 | 33 | 59 | 0 |
| 30 | crates/kanzei-memory/src/memory/manager.rs | 1186 | 662 | 524 | 32 | 70 | 0 |

## 读数

- 当前 Top-30 覆盖全仓 228 个 `.rs` 文件;生产行数 > 1200 的巨石 7 个（typed.rs、docstore.rs、git.rs、background.rs、drive.rs、memory/mod.rs、config.rs）。
- 相比 B14 快照，background.rs 生产行 1747→1431（-316），drive.rs 1489→1290（-199）；`drive.rs` 已从第 4 降至第 5，但仍属于巨石目标并保留 6 处 >7 参数函数。
- 参数 > 7 函数 ≥ 4 处的文件仍为 1 个：`drive.rs`（6 处）。
- 最大函数 > 400 行仅剩 `runner/subagent.rs`（413）；此前 `drive.rs`（526）、`profiles.rs`（532）、`cli/run.rs`（652）已降出该阈值或完成拆分。
- 本次快照覆盖 Rust 度量；前端 `06-agent-panel.js`/`06-activity.js` 合流沿用既有 B8 能力，前端拆分冒烟证据见 T-1786922726432、T-1786922726433。

## 回涨闸门

- `scripts/metrics-regression-gate.ps1` 由 `scripts/verify.ps1:56-60` 的 `crate_sync` 步骤真实调用。
- 对基线中仍出现在 Top-30 的文件，生产行允许最多比基线增加 100 行（宽松起步，防止测试/生成口径微调误伤）；超过即失败。
- Top-30 中生产行超过 1200 的巨石数量允许最多比基线增加 1 个；超过即失败。
- 本次重跑结果：`cargo run -p kanzei -- metrics --top 30` 产出 30 行、巨石 7 个；T-1786922726441 记录 provider-qualified 路径修复前的失败复现，T-1786922726442 记录同一扩展路径 gate 修复后通过（30 rows、7/7、允许回涨 100 行）。
