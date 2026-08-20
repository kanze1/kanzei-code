# 巨石度量基线快照(R-258 批2)

来源:`cargo run -p kanzei -- metrics --top 30`（R-300 B6 后复跑；输出与用户安装位 `kz metrics --top 30` 一致）。
口径:`crates/kanzei/src/cli/metrics.rs`——生产行数 = 总行数 − cfg(test) 块行数
(cfg(test) 块按大括号配平识别,外挂声明 `#[cfg(test)] mod x;` 不算测试块;
`_tests.rs` 后缀与 `tests/` 目录的外挂测试文件整文件算测试行);函数度量只统计生产码;
参数 > 7 沿用 clippy too_many_arguments 默认阈值。
阈值(conventions §9.2):生产行数 > 1200 巨石;参数 > 7 函数 ≥ 4 处失控;最大函数 > 400 行。

## Top-30 榜单(按生产行数降序, 2026-08-21 发布线与自举线合并(7f42d074)后复跑)

| # | 文件 | 总行 | 生产 | 测试 | 函数 | 最大fn | >7参 |
|---|---|---:|---:|---:|---:|---:|---:|
| 1 | crates/kanzei-memory/src/memory/mod.rs | 2778 | 1399 | 1379 | 51 | 91 | 0 |
| 2 | crates/kanzei-core/src/runner/drive.rs | 1446 | 1215 | 231 | 12 | 274 | 5 |
| 3 | crates/kanzei-core/src/store/typed.rs | 2411 | 1202 | 1209 | 39 | 210 | 0 |
| 4 | crates/kanzei-tools/src/work.rs | 2256 | 1194 | 1062 | 24 | 1522 | 1 |
| 5 | crates/kanzei-memory/src/memory/store.rs | 3335 | 1011 | 2324 | 27 | 121 | 2 |
| 6 | crates/kanzei-tools/src/git.rs | 2288 | 947 | 1341 | 24 | 94 | 0 |
| 7 | crates/kanzei-tools/src/test_record.rs | 2194 | 937 | 1257 | 26 | 107 | 3 |
| 8 | crates/kanzei-tools/src/plot_tool.rs | 1096 | 924 | 172 | 28 | 139 | 0 |
| 9 | crates/kanzei-tools/src/tracker.rs | 4100 | 920 | 3180 | 23 | 212 | 0 |
| 10 | crates/kanzei-app/src/settings.rs | 1714 | 917 | 797 | 29 | 106 | 0 |
| 11 | crates/kanzei-core/src/runner/subagent.rs | 1136 | 909 | 227 | 18 | 413 | 0 |
| 12 | crates/kanzei-app/src/docs.rs | 914 | 897 | 17 | 22 | 203 | 2 |
| 13 | crates/kanzei-app/src/mobile.rs | 1497 | 867 | 630 | 20 | 180 | 1 |
| 14 | crates/kanzei-tools/src/tracker/actions.rs | 867 | 867 | 0 | 9 | 290 | 0 |
| 15 | crates/kanzei-tools/src/palette.rs | 1266 | 864 | 402 | 34 | 85 | 0 |
| 16 | crates/kanzei-app/src/state.rs | 961 | 832 | 129 | 32 | 74 | 0 |
| 17 | crates/kanzei-tools/src/tracker/scheduling.rs | 1064 | 827 | 237 | 39 | 43 | 1 |
| 18 | crates/kanzei-memory/src/memory/index.rs | 1542 | 824 | 718 | 35 | 82 | 0 |
| 19 | crates/kanzei-core/src/replay.rs | 861 | 798 | 63 | 26 | 57 | 2 |
| 20 | crates/kanzei-app/src/phase_pipeline.rs | 933 | 796 | 137 | 25 | 82 | 1 |
| 21 | crates/kanzei-app/src/run/assembly.rs | 879 | 789 | 90 | 15 | 377 | 0 |
| 22 | crates/kanzei-base/src/atomic_file.rs | 1198 | 757 | 441 | 22 | 102 | 0 |
| 23 | crates/kanzei-tools/src/symbols.rs | 1140 | 731 | 409 | 12 | 126 | 0 |
| 24 | crates/kanzei-memory/src/memory/manager.rs | 1433 | 729 | 704 | 33 | 82 | 0 |
| 25 | crates/kanzei-llm/src/protocol/openai.rs | 775 | 704 | 71 | 17 | 131 | 0 |
| 26 | crates/kanzei-tools/src/bash.rs | 1378 | 698 | 680 | 24 | 268 | 0 |
| 27 | crates/kanzei-harness/src/orchestration.rs | 985 | 695 | 290 | 26 | 113 | 1 |
| 28 | crates/kanzei-tools/src/cross_tree.rs | 1409 | 693 | 716 | 17 | 217 | 0 |
| 29 | crates/kanzei-harness/src/permission.rs | 1148 | 675 | 473 | 33 | 59 | 0 |
| 30 | crates/kanzei-core/src/store/work.rs | 807 | 664 | 143 | 19 | 183 | 0 |

## 读数

- 当前 Top-30 覆盖全仓 255 个 `.rs` 文件;生产行数 > 1200 的巨石 3 个(memory/mod.rs、drive.rs、typed.rs),较上版 -1。
- 2026-08-21 合并复跑基线更新:`memory/mod.rs` 1263→1399(+136)来自 R-284 体验持久事实接入,经合并审计的合法增长,同时晋升为后续拆解首要目标;`git.rs` 1450→947(-503)来自远端 git 四域拆分收编跌出巨石;`work.rs` 1169→1194 与 `store/work.rs` 新入榜(664)来自 R-317 Work Unit 底座;`tracker.rs` 868→920 来自 R-248 prior-art 门禁。
- `drive.rs` 生产行 1215 越过巨石阈值(上版 1189 贴线),与 memory/mod.rs 一并列为拆解目标。
- 参数 > 7 函数 ≥ 4 处的文件仍为 1 个:`drive.rs`(5 处)。
- 最大函数 > 400 行:`runner/subagent.rs`(413)与 `work.rs`(1522,R-317 收编产物,待拆);此前 `drive.rs`、`profiles.rs`、`cli/run.rs` 已降出该阈值。
- 本次快照覆盖 Rust 度量;前端拆分冒烟证据沿用 T-1786922726432、T-1786922726433。

## 回涨闸门

- `scripts/metrics-regression-gate.ps1` 由 `scripts/verify.ps1` 的 `crate_sync` 步骤真实调用。
- 对基线中仍出现在 Top-30 的文件，生产行允许最多比基线增加 100 行（宽松起步，防止测试/生成口径微调误伤）；超过即失败。
- Top-30 中生产行超过 1200 的巨石数量允许最多比基线增加 1 个；超过即失败。
- 本次重跑结果：`cargo run -p kanzei -- metrics --top 30` 产出 30 行、巨石 3 个（2026-08-21 合并后复跑，触发原因是 memory/mod.rs +136 超过 100 行回涨额度——增长来自已交付的 R-284,按有意识重基线路径处理）。
