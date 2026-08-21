# 巨石度量基线快照(R-258 批2)

来源:`cargo run -p kanzei -- metrics --top 30`（R-300 B6 后复跑；输出与用户安装位 `kz metrics --top 30` 一致）。
metrics_format_version: v1
口径:`crates/kanzei/src/cli/metrics.rs`——生产行数 = 总行数 − cfg(test) 块行数
(cfg(test) 块按大括号配平识别,外挂声明 `#[cfg(test)] mod x;` 不算测试块;
`_tests.rs` 后缀与 `tests/` 目录的外挂测试文件整文件算测试行);函数度量只统计生产码;
参数 > 7 沿用 clippy too_many_arguments 默认阈值。
阈值(conventions §9.2):生产行数 > 1200 巨石;参数 > 7 函数 ≥ 4 处失控;最大函数 > 400 行。

## Top-30 榜单(按生产行数降序, R-309 B4 口径 v1 复跑)

| # | 文件 | 总行 | 生产 | 测试 | 函数 | 最大fn | >7参 |
|---|---|---:|---:|---:|---:|---:|---:|
| 1 | crates/kanzei-memory/src/memory/mod.rs | 2915 | 1460 | 1455 | 52 | 91 | 0 |
| 2 | crates/kanzei-core/src/runner/drive.rs | 1446 | 1215 | 231 | 12 | 274 | 5 |
| 3 | crates/kanzei-core/src/store/typed.rs | 2411 | 1202 | 1209 | 39 | 210 | 0 |
| 4 | crates/kanzei-tools/src/work.rs | 2256 | 1194 | 1062 | 24 | 1522 | 1 |
| 5 | crates/kanzei-tools/src/git.rs | 2694 | 1180 | 1514 | 31 | 125 | 0 |
| 6 | crates/kanzei-memory/src/memory/store.rs | 3652 | 1056 | 2596 | 29 | 134 | 2 |
| 7 | crates/kanzei-tools/src/test_record.rs | 2200 | 940 | 1260 | 26 | 107 | 3 |
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
| 18 | crates/kanzei-memory/src/memory/index.rs | 1544 | 826 | 718 | 35 | 82 | 0 |
| 19 | crates/kanzei-core/src/replay.rs | 861 | 798 | 63 | 26 | 57 | 2 |
| 20 | crates/kanzei-app/src/phase_pipeline.rs | 933 | 796 | 137 | 25 | 82 | 1 |
| 21 | crates/kanzei-app/src/run/assembly.rs | 879 | 789 | 90 | 15 | 377 | 0 |
| 22 | crates/kanzei-base/src/atomic_file.rs | 1198 | 757 | 441 | 22 | 102 | 0 |
| 23 | crates/kanzei-memory/src/memory/manager.rs | 1516 | 746 | 770 | 33 | 99 | 0 |
| 24 | crates/kanzei-tools/src/symbols.rs | 1620 | 1020 | 600 | 19 | 154 | 0 |
| 25 | crates/kanzei-llm/src/protocol/openai.rs | 775 | 704 | 71 | 17 | 131 | 0 |
| 26 | crates/kanzei-tools/src/bash.rs | 1378 | 698 | 680 | 24 | 268 | 0 |
| 27 | crates/kanzei-harness/src/orchestration.rs | 985 | 695 | 290 | 26 | 113 | 1 |
| 28 | crates/kanzei-tools/src/cross_tree.rs | 1409 | 693 | 716 | 17 | 217 | 0 |
| 29 | crates/kanzei-harness/src/permission.rs | 1148 | 675 | 473 | 33 | 59 | 0 |
| 30 | crates/kanzei-core/src/store/work.rs | 807 | 664 | 143 | 19 | 183 | 0 |

## 读数

- 当前 Top-30 覆盖全仓 256 个 `.rs` 文件;生产行数 > 1200 的巨石 3 个(memory/mod.rs、drive.rs、typed.rs)。
- R-309 B4 以 metrics format v1 重新采集当前工作树读数，更新 `memory/mod.rs`、`memory/store.rs`、`git.rs`、`test_record.rs`、`memory/manager.rs` 等已发生结构性变化的基线值；回涨闸仍保持单文件最多增加 100 行，不因基线更新放宽阈值。
- 参数 > 7 函数 ≥ 4 处的文件仍为 1 个:`drive.rs`(5 处)。
- 最大函数 > 400 行:`runner/subagent.rs`(413)与 `work.rs`(1522,R-317 收编产物,待拆);此前 `drive.rs`、`profiles.rs`、`cli/run.rs` 已降出该阈值。
- 本次快照覆盖 Rust 度量;前端拆分冒烟证据沿用 T-1786922726432、T-1786922726433。

## 回涨闸门

- `scripts/metrics-regression-gate.ps1` 由 `scripts/verify.ps1` 的 `crate_sync` 步骤真实调用。
- 对基线中仍出现在 Top-30 的文件，生产行允许最多比基线增加 100 行（宽松起步，防止测试/生成口径微调误伤）；超过即失败。
- Top-30 中生产行超过 1200 的巨石数量允许最多比基线增加 1 个；超过即失败。
- 本次重跑结果：`cargo run -p kanzei -- metrics --top 30` 产出 30 行、巨石 3 个（2026-08-21 合并后复跑，触发原因是 memory/mod.rs +136 超过 100 行回涨额度——增长来自已交付的 R-284,按有意识重基线路径处理）。

## 基线变更记录

抬基线是**有意识的动作**,不是让门禁闭嘴的手段。每次改行都要写清增长来自哪条
交付、为什么不该拆。没有理由的抬升等于把回涨闸变成摆设。

- 2026-08-21 `crates/kanzei-tools/src/symbols.rs` 生产行 881 → 1020(+139,再次超出
  每文件 100 行允许量)。增长来自 R-324 把符号索引扩到 JS/ESM:
  `parse_js_symbol_line` 与 `js_identifier`/`js_looks_like_arrow` 两个判定辅助,
  外加扩展名收集与目录跳过。这是该条目的交付主体——本仓受跟踪文件里 257 个 `.rs`
  对 139 个 `.js`/`.mjs`,`crates/kanzei-app/ui/` 一处就 26 文件 16k 行,此前完全
  没有符号索引。最大函数长度未变(154),仍在单文件巨石阈值(生产行 1200)以下。
  **下次再涨要先想拆**:1020 距 1200 只剩 180 行,再加一门语言就该按语言分文件,
  而不是继续在同一个文件里堆判定分支。
- 2026-08-21 `crates/kanzei-tools/src/symbols.rs` 生产行 731 → 881(+150,
  超出每文件 100 行允许量)。增长来自 R-310 B3 的代码地图能力
  (`crate → module → public symbol` 按需查询,设计见
  `r310_repo_map_design.md`),是该条目的交付主体,不是无关堆积。
  最大函数长度同步 126 → 154,仍在单文件巨石阈值(生产行 1200)以下。
  **发现方式**:本次发版跑 verify 时回涨闸报红——R-310 B4 关闭时没跑门禁,
  基线欠账留到了发版才暴露(D-664)。
