# 巨石度量基线快照(R-258 批2)

来源:`kz metrics --top 30`(2026-08-16,R-258 批1 工具落地后首次快照)。
口径:`crates/kanzei/src/cli/metrics.rs`——生产行数 = 总行数 − cfg(test) 块行数
(cfg(test) 块按大括号配平识别,外挂声明 `#[cfg(test)] mod x;` 不算测试块;
`_tests.rs` 后缀与 `tests/` 目录的外挂测试文件整文件算测试行);函数度量只统计生产码;
参数 > 7 沿用 clippy too_many_arguments 默认阈值。
阈值(conventions §9.2):生产行数 > 1200 巨石;参数 > 7 函数 ≥ 4 处失控;最大函数 > 400 行。

## Top-30 榜单(按生产行数降序)

| # | 文件 | 总行 | 生产 | 测试 | 函数 | 最大fn | >7参 |
|---|---|---|---|---|---|---|---|
| 1 | crates/kanzei-tools/src/background.rs | 2041 | 2019 | 22 | 71 | 132 | 1 |
| 2 | crates/kanzei-core/src/runner/drive.rs | 2083 | 1852 | 231 | 14 | 516 | 6 |
| 3 | crates/kanzei-memory/src/docstore.rs | 2624 | 1468 | 1156 | 45 | 73 | 0 |
| 4 | crates/kanzei-core/src/store/typed.rs | 2051 | 1347 | 704 | 42 | 210 | 1 |
| 5 | crates/kanzei-tools/src/git.rs | 2467 | 1299 | 1168 | 36 | 155 | 0 |
| 6 | crates/kanzei-tools/src/test_record.rs | 2456 | 1261 | 1195 | 39 | 107 | 3 |
| 7 | crates/kanzei-tools/src/tracker/actions.rs | 1229 | 1229 | 0 | 23 | 238 | 0 |
| 8 | crates/kanzei-harness/src/config.rs | 2937 | 1220 | 1717 | 45 | 104 | 1 |
| 9 | crates/kanzei-memory/src/memory/mod.rs | 2557 | 1218 | 1339 | 45 | 91 | 0 |
| 10 | crates/kanzei-tools/src/work.rs | 1678 | 1074 | 604 | 25 | 222 | 0 |
| 11 | crates/kanzei-core/src/phase.rs | 974 | 910 | 64 | 37 | 293 | 0 |
| 12 | crates/kanzei-app/src/phase_pipeline.rs | 880 | 870 | 10 | 31 | 63 | 1 |
| 13 | crates/kanzei-app/src/settings.rs | 1528 | 858 | 670 | 25 | 107 | 0 |
| 14 | crates/kanzei-core/src/runner/subagent.rs | 1031 | 826 | 205 | 16 | 369 | 0 |
| 15 | crates/kanzei-tools/src/profiles.rs | 1597 | 788 | 809 | 6 | 524 | 0 |
| 16 | crates/kanzei-app/src/run/assembly.rs | 852 | 785 | 67 | 15 | 377 | 0 |
| 17 | crates/kanzei-memory/src/memory/index.rs | 1394 | 776 | 618 | 35 | 82 | 0 |
| 18 | crates/kanzei-core/src/replay.rs | 797 | 738 | 59 | 24 | 297 | 1 |
| 19 | crates/kanzei-memory/src/memory/store.rs | 2740 | 728 | 2012 | 22 | 110 | 2 |
| 20 | crates/kanzei-tools/src/tracker.rs | 3483 | 712 | 2771 | 16 | 166 | 0 |
| 21 | crates/kanzei-app/src/state.rs | 797 | 698 | 99 | 27 | 49 | 0 |
| 22 | crates/kanzei-harness/src/orchestration.rs | 985 | 695 | 290 | 26 | 113 | 1 |
| 23 | crates/kanzei-harness/src/permission.rs | 1147 | 675 | 472 | 33 | 59 | 0 |
| 24 | crates/kanzei/src/cli/run.rs | 663 | 663 | 0 | 1 | 637 | 0 |
| 25 | crates/kanzei-tools/src/bash.rs | 1334 | 654 | 680 | 24 | 224 | 0 |
| 26 | crates/kanzei-memory/src/memory/manager.rs | 1164 | 650 | 514 | 32 | 59 | 0 |
| 27 | crates/kanzei-harness/src/auto_run.rs | 656 | 638 | 18 | 29 | 51 | 0 |
| 28 | crates/kanzei-app/src/docs.rs | 611 | 611 | 0 | 14 | 146 | 1 |
| 29 | crates/kanzei-tools/src/tracker/scheduling.rs | 596 | 596 | 0 | 28 | 40 | 0 |
| 30 | crates/kanzei-app/src/commands/run.rs | 591 | 591 | 0 | 11 | 51 | 0 |

## 读数

- 全仓 193 个 .rs 文件;生产行数 > 1200 的巨石 9 个(前 9 名)。
- 参数 > 7 函数 ≥ 4 处的文件 1 个:`drive.rs`(6 处)——与 R-257 拆解计划的
  `drive.rs` 高度重叠,拆解后应回落。
- 最大函数 > 400 行:drive.rs(516)、profiles.rs(524)、cli/run.rs(637)。
- 对照基准:tracker.rs 生产 712 行(总 3483)——raw 行数 3483 会误诊巨石,度量口径
  下的真实生产规模 712 行,与 R-258 验收①「报生产 660 而非 3253」同源(文件
  2026-08-15 后随 scheduling/actions 拆分增长)。

## 用途

后续拆解条目(R-257 等)以本快照为前后对照基线:拆解后重跑 `kz metrics`,
同一文件的生产行数应下降、巨石数量应减少。
