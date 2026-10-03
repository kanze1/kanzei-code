# 巨石度量基线快照（2026-10-03 发布审查）

来源：当前已编译 `target/debug/kz.exe metrics --top 500`，前30名按生产行排序。
metrics_format_version: v1
口径：`crates/kanzei/src/cli/metrics.rs`；生产行数=总行数−cfg(test)块行数，`tests.rs`、`_tests.rs`及`tests/`目录为外挂纯测试文件，函数复杂度只统计生产码。新修复只补遗漏的标准tests.rs归属，不更换版本或词法算法。
阈值保持原值：单文件最多增加100生产行；Top30中生产行>1200的巨石最多增加1个。参数>7、最大函数>400仍是观察值。本快照是当前数据，不是未审文件的PASS证书。

## 当前Top30

| # | 文件 | 总行 | 生产 | 测试 | 函数 | 最大fn | >7参 |
|---|---|---:|---:|---:|---:|---:|---:|
| 1 | crates/kanzei-tools/src/team/mod.rs | 1612 | 1612 | 0 | 58 | 291 | 0 |
| 2 | crates/kanzei-memory/src/memory/mod.rs | 3077 | 1550 | 1527 | 56 | 88 | 1 |
| 3 | crates/kanzei-core/src/store/typed.rs | 3596 | 1461 | 2135 | 47 | 226 | 0 |
| 4 | crates/kanzei-core/src/runner/drive.rs | 1967 | 1320 | 647 | 8 | 427 | 4 |
| 5 | crates/kanzei-tools/src/research_runner.rs | 1916 | 1299 | 617 | 32 | 625 | 2 |
| 6 | crates/kanzei-tools/src/work.rs | 2862 | 1274 | 1588 | 24 | 2084 | 1 |
| 7 | crates/kanzei-core/src/store/session.rs | 1899 | 1195 | 704 | 42 | 107 | 0 |
| 8 | crates/kanzei-tools/src/git.rs | 2990 | 1154 | 1836 | 27 | 129 | 0 |
| 9 | crates/kanzei-app/src/commands/run.rs | 1475 | 1149 | 326 | 23 | 51 | 0 |
| 10 | crates/kanzei-app/src/run/events/mod.rs | 1648 | 1114 | 534 | 36 | 300 | 1 |
| 11 | crates/kanzei-memory/src/memory/store.rs | 3744 | 1083 | 2661 | 28 | 134 | 4 |
| 12 | crates/kanzei-tools/src/tracker.rs | 5578 | 1081 | 4497 | 27 | 200 | 0 |
| 13 | crates/kanzei-tools/src/symbols.rs | 1688 | 1037 | 651 | 20 | 162 | 0 |
| 14 | crates/kanzei-core/src/runner/subagent.rs | 1383 | 1000 | 383 | 22 | 458 | 0 |
| 15 | crates/kanzei-core/src/research.rs | 1089 | 969 | 120 | 26 | 169 | 1 |
| 16 | crates/kanzei-app/src/run/assembly.rs | 1847 | 962 | 885 | 17 | 323 | 2 |
| 17 | crates/kanzei-tools/src/test_record.rs | 2284 | 947 | 1337 | 26 | 107 | 3 |
| 18 | crates/kanzei-app/src/settings.rs | 1931 | 943 | 988 | 29 | 112 | 0 |
| 19 | crates/kanzei-app/src/preview/pane.rs | 1003 | 936 | 67 | 45 | 60 | 0 |
| 20 | crates/kanzei-app/src/processes/lifecycle.rs | 1415 | 935 | 480 | 24 | 138 | 2 |
| 21 | crates/kanzei-tools/src/refgraph/memory_graph.rs | 911 | 911 | 0 | 20 | 603 | 1 |
| 22 | crates/kanzei-llm/src/protocol/anthropic.rs | 1176 | 910 | 266 | 21 | 267 | 0 |
| 23 | crates/kanzei-harness/src/permission.rs | 1380 | 907 | 473 | 43 | 149 | 0 |
| 24 | crates/kanzei-app/src/docs.rs | 1007 | 906 | 101 | 26 | 288 | 2 |
| 25 | crates/kanzei-app/src/mobile.rs | 2289 | 901 | 1388 | 27 | 218 | 2 |
| 26 | crates/kanzei-app/src/state.rs | 1043 | 887 | 156 | 35 | 75 | 0 |
| 27 | crates/kanzei-tools/src/tracker/scheduling.rs | 1121 | 867 | 254 | 40 | 43 | 1 |
| 28 | crates/kanzei-tools/src/tracker/actions.rs | 865 | 865 | 0 | 10 | 433 | 0 |
| 29 | crates/kanzei-tools/src/palette.rs | 1266 | 864 | 402 | 34 | 85 | 0 |
| 30 | crates/kanzei-tools/src/shell.rs | 848 | 848 | 0 | 35 | 786 | 1 |

## 2026-10-03 对话模型统一复测

本次重新执行 metrics --top 30：去掉第一条对话的生命周期与执行策略特例，主要生产文件行数下降；新测试归入测试行。表中保留相同 v1 口径和阈值。

## 2026-10-03 有意识更新的原因

首轮Full门禁原始失败与旧基线保存于 `output/release-2026-10-03/verify-880c7135.log` 和 `metrics-baseline-before.md`，旧Git版本可从 `40bb0616:docs/design/metrics_baseline.md`恢复。旧表格巨石实际4个，正文曾写3个；这次不沿用错误正文。

当前真实CLI还将12个tests.rs全部误算生产（team/tests.rs 2698行、50函数），该P1已修：9项定向单测通过，含新增真实文件树收集回归；旧/新独立CLI对全部422个Rust文件比较，12个测试文件恢复全测试、0生产/0函数，其他生产指标保持不变，巨石错误读数7→6。不是通过放宽阈值隐藏测试文件。

下列超100行差额按已经审查并验证的交付接受；差额包含旧基线历史偏差，不能全部宣称本轮源码新增：
- `crates/kanzei-harness/src/permission.rs`：675 → 907（+232）。D3 资源权限例外；最大函数149行，保留同一权限 owner，发布收尾不再拆分正确的判定。
- `crates/kanzei-app/src/run/assembly.rs`：789 → 981（+192）。C4/C6执行owner、实际input/current writer失败收尾和准入ACK；最大函数323行，保留已经实测的启动状态转换。
- `crates/kanzei-core/src/runner/drive.rs`：1215 → 1320（+105）。C3持久化ACK先于运行历史/副作用、协议思考回放；本次保留已验证控制流，后续再动427行函数时按职责收敛。
- `crates/kanzei-core/src/store/typed.rs`：1202 → 1461（+259）。C2及C4/C5/C6同一事务核验与终态/input/stage提交；最大函数226行，事务边界不能在发布收尾随意切开。

对应证据见 [D3](../reviews/file-audit-2026-10-03/parallel/D3.md)、[C2](../reviews/file-audit-2026-10-03/parallel/C2.md)、[C3](../reviews/file-audit-2026-10-03/parallel/C3.md)、[C4](../reviews/file-audit-2026-10-03/parallel/C4.md)、[C5](../reviews/file-audit-2026-10-03/parallel/C5.md)、[C6](../reviews/file-audit-2026-10-03/parallel/C6.md)。生产状态合同优先，本次不为了度量好看重写已正确的模块。

新榜单是完整重测：team/mod.rs 1612生产行由B2/D2审查的worker/attempt owner合同构成；research_runner.rs与work.rs仍显示1299/1274生产行，未据此新增全文PASS。它们及其他旧版既有文件的度量存在不能被新基线遮掉，后续按依赖地图继续审查。原始每行数值、相对上一发布是否改源记录于 `metrics-baseline-provenance.json`；基线没有修改任何这些生产文件。当前六个巨石为team/mod、memory/mod、typed、drive、research_runner、work。

下一次新增量仍按100行/1个门禁判定；当前完整快照保留实际大文件与最大函数读数，后续重开它们必须先核对职责、caller和状态owner。没有新第三方依赖或持久化格式变化。

## 基线变更记录

抬基线是**有意识的动作**,不是让门禁闭嘴的手段。每次改行都要写清增长来自哪条
交付、为什么不该拆。没有理由的抬升等于把回涨闸变成摆设。

- 2026-08-21 三处一并抬,增长来自 R-313 需求发现门禁与 R-315 验收开放度分级
  (自举批次)。**这次抬得不情愿,已登记 D-682**:
  - `tracker.rs` 生产行 920 → 1202(+282),**越过单文件巨石阈值 1200**;
    `>7 参数` 从 0 变 1,函数数 23 → 36;
  - `tracker/actions.rs` 867 → 1003,且**最大函数 290 → 373 行** —— 单个函数
    373 行本身就该拆,它不是「文件大」而是「一个函数在做太多事」;
  - `memory/store.rs` 1056 → 1167,`>7 参数` 2 → 3。

  抬基线只是为了不阻断本次发版。**下一次动这三个文件之前先拆**:tracker 一族
  已经有 `tracker/actions.rs` 这个分文件先例,继续按动作族切;373 行的函数按
  R-253~R-258 的口径属于必须处理的对象,不该再往里加分支。

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
