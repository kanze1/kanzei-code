# 巨石文件拆解 第二轮(app/run.rs · memory/store.rs · app/processes.rs)

- 状态:设计基线(2026-08-15 用户提供第二轮巨石扫描 + 本仓机器复核)
- 日期:2026-08-15
- 关联需求:R-253(run.rs)、R-255(MemoryStore)、R-254(processes.rs);后续 R-256(Desktop/CLI 共用 RunService)、R-257(第二梯队)、R-258(度量口径)
- 关联缺陷:D-365(worktree 转发壳)、D-366(Store/Index 检索边界)、D-367(主根/工作树根不变式)、D-364(托管文档并发写丢条目,影响本轮登记纪律)
- 前轮:[monolith_decomposition.md](monolith_decomposition.md)(R-153/R-154/R-155,已交付;R-202/R-204 为其"另立条目"承诺的收尾)

> **行号基准:commit c164609(2026-08-15)。** 执行时一律以**符号名**定位,行号只作导航参考。本仓自举与外部 agent 会并发提交(D-364 现场),动手前 `git log` + `kz lock status` 核对,行号明显对不上就先重新读码再改。

## 为什么有第二轮

第一轮(2026-08-09)拆的是 `app/main.rs` 6413 行、`ui/main.js` 7020 行、`core/runner.rs` 3240 行、`core/store.rs` 1972 行。那一轮的产物之一 `app/src/run.rs` 当时只是"运行主链路";六天之内,memory 预检索、scout、review/fixup、phase pipeline、write lease、子代理 runtime、autonomous 调度逐个叠了进去,它重新长成了 attractor。

这说明第一轮解决的是**文件级模块化**,没解决**分层**:application service 树整棵住在一个文件里,新能力只会继续往树根堆。第二轮拆的是层,不是文件。

### 行数口径(重要)

本文所有行数都是**生产行数**——总行数扣除 `#[cfg(test)]` 块(按大括号配平计算,不是"找到第一个 `cfg(test)` 就一刀切")。这个口径本身是 R-258 的内容:

| 文件 | 总行 | 测试行 | **生产行** | 说明 |
| --- | ---: | ---: | ---: | --- |
| `kanzei-app/src/run.rs` | 3268 | 383 | **2885** | 本轮 P0 之一 |
| `kanzei-memory/src/memory/store.rs` | 4085 | 2012 | **2073** | 本轮 P0 之一 |
| `kanzei-core/src/runner/drive.rs` | 2058 | 232 | **1826** | R-257 队首,本轮不动 |
| `kanzei-app/src/processes.rs` | 1651 | 23 | **1628** | 本轮 P1(真测试在同级 `worktree_tests.rs` 2437 行) |
| `kanzei/src/main.rs` | 2216 | 626 | **1590** | R-256,前三条稳定后再动 |
| `kanzei-memory/src/docstore.rs` | 2471 | 1054 | **1417** | R-257 |
| `kanzei-tools/src/git.rs` | 2318 | 1061 | **1257** | R-257 |
| `kanzei-harness/src/config.rs` | 2937 | 1719 | **1218** | R-257 |
| `kanzei-tools/src/tracker.rs` | 3253 | 2593 | **660** | **不是巨石**,勿动 |

反例两条,都要记住:`tracker.rs` 页面上 3253 行但生产码只有 660 行(R-204 已拆过);`drive.rs` 页面上 2058 行不起眼,生产码 1826 行、7 处 `too_many_arguments`。**用 `wc -l` 做门禁会同时误伤前者、放过后者。**

## 目标与非目标

- 目标:**分层**——按变更理由切开 application service;**零行为变更**;**零对外契约变更**(Tauri command 名与入参返回结构、`kanzei_tools::memory::*` 再导出路径、CLI 命令面全部一字不动);每批独立编译、可单独提交回滚。
- 非目标:不做 Desktop/CLI 合流(R-256,前三条稳定后再开);不动 `drive.rs`/`docstore.rs`/`git.rs`/`config.rs`(R-257);不改召回排序权重、不改 SQLite schema、不改进程编号与 `session_id` 推导;不引入新的 async 抽象层或 trait 体操——普通函数与结构体能表达就不要 trait。

## 执行纪律(三条共用)

1. **每批 = 一次提交**,批内定向验证:R-253/R-254 → `cargo test -p kanzei-app` + 四条前端冒烟;R-255 → `cargo test -p kanzei-memory` + `cargo check -p kanzei-tools -p kanzei-app -p kanzei`(外部再导出面断言)。全量 `cargo test --workspace` 只在条目关闭前与发版前跑。
2. **搬迁 = 剪切粘贴 + 最小可见性调整**,diff 里只允许 move + use + 可见性,出现逻辑 diff 即回退重来。这条比第一轮更严:第二轮的目标是分层,分层动作与行为修改必须分开提交,否则回归无法归因。
3. **三条不得互相并发,也不得与任何其他源码条目并发**。动手前 `git log` + `git status` + `kz lock status`;桌面端自举轮在跑时不要同时动同一批文件(D-364 现场)。
4. **分文件 ≠ 换所有者**。Rust 允许同一类型的 `impl` 块分散在同 crate 的多个文件里。第一刀一律用这个手法——把 `impl MemoryStore { ... }` 按域切进不同文件,**调用点零改动、行为零变更**;等边界稳定了第二刀才做真正的所有权迁移(独立类型、独立入口)。把两件事塞进一批是第二轮最容易翻车的地方。

---

## A. `kanzei-app/src/run.rs`(R-253)

### 现状符号地图(c164609,生产码 L1–2883)

| 行段 | 符号 | 性质 |
| --- | --- | --- |
| 1–39 | imports、`TRACE_INPUT_KEEP_CHARS`、`compaction_input_tokens` | 杂项 |
| 41–72 | `RunAssembly`(**28 字段**) | 装配产物 |
| 74–432 | `assemble_run`(359 行) | 装配 |
| 434–445 | `subagent_round_tool` | 事件辅助 |
| 447–759 | `build_event_handler`(**310 行 giant reducer**) | 事件 |
| 761–820 | `build_ask_handler` | 事件 |
| 822–903 | `build_subagent_runtime` | 子代理 |
| 905–1051 | `run_execution_loop` | 执行 |
| 1053–1241 | `persist_round_outcome` | 持久化 |
| 1243–1446 | `finalize_round`(204 行) | 持久化 |
| 1448–1771 | `run_task`(**324 行 Round Coordinator**) | 协调 |
| 1772–1786 | `app_info` command、`now_ms` | **IPC / 杂项** |
| 1787–1869 | `maybe_push_after_commit`、`push_ollama_models`、`emit_stage`、`build_model_route` | 杂项 |
| 1870–1966 | `run_review_and_fixup`(97 行) | 执行(复合阶段) |
| 1967–2024 | `WriterLeaseTrace` + `impl` + `Drop` | 装配/持久化 |
| 2025–2258 | `build_runner_config`、`new_llm_client`、`auth_stage_detail`、`resolve_reasoning_override`、`resolve_proxy`、`append_dev_guidance`、`build_run_harness`、`TrackerWritePolicyComponent`、`work_priority_guidance`、`cadence_guidance`、`resolve_profile`、`normalize_work_priority`、`report_config_warnings` | 装配辅助 |
| 2259–2342 | `models_list` command | **非编排 IPC** |
| 2343–2406 | `pending_asks_get`、`persist_always_allow`、`answer_ask` | **IPC(询问)** |
| 2407–2458 | `fast_summarize`、`summarize_chat` command | **非编排 IPC** |
| 2459–2572 | `stop_run`、`stop_task` command | **IPC(停止)** |
| 2573–2650 | `parse_delivery`、`admit_input`、`promote_next_input`、`report_persistence_failure`、`append_run_notification`、`code_root_for` | 输入准入/杂项 |
| 2651–2863 | `run_prompt` command(**213 行,外层 scheduler**) | **IPC(入口)** |
| 2864–2883 | `run_metrics` command | **IPC** |

`#[allow(clippy::too_many_arguments)]` 九处:L78 `assemble_run`、L451 `build_event_handler`、L826 `build_subagent_runtime`、L910 `run_execution_loop`、L1057 `persist_round_outcome`、L1246 `finalize_round`、L1448 `run_task`、L1877 `run_review_and_fixup`、L2650 `run_prompt`。**全仓最高密度。**

### 目标模块划分

```text
crates/kanzei-app/src/run/
├── mod.rs          // mod 声明 + pub(crate) 再导出;≤ 120 行
├── assembly.rs     // RunAssembly 三分 + assemble_run + 装配辅助 + WriterLeaseTrace
├── coordinator.rs  // run_task(Round Coordinator)
├── execution.rs    // run_execution_loop + run_review_and_fixup + build_subagent_runtime
├── persistence.rs  // persist_round_outcome + finalize_round
├── events/
│   ├── mod.rs      // RunEventFanout + build_ask_handler
│   ├── ui.rs       // UiEventSink(emit_event 一族)
│   ├── typed.rs    // TypedEventSink(typed_writer 一族)
│   ├── trace.rs    // TraceSink(record_live_trace_at_path 一族)
│   └── metrics.rs  // MetricsSink(LiveRun 累计 + R-143 commit 检测位 + D-361 子代理工具名)
└── input.rs        // parse_delivery / admit_input / promote_next_input / code_root_for
crates/kanzei-app/src/commands/
├── run.rs          // run_prompt / stop_run / stop_task / run_metrics / pending_asks_get / answer_ask
├── models.rs       // models_list / build_model_route / push_ollama_models
└── summarize.rs    // summarize_chat / fast_summarize
```

### 装配层的三分(这一刀是本条的关键)

`RunAssembly` 28 个字段不是"字段多",是**三种生命周期被装进同一个容器**。拆成:

```rust
struct RuntimeDeps {   // 本轮不变的依赖:配置解析的产物
    project_root, config, profile, rctx, snapshot, agent, work_priority,
    resolved, proxy, route, client, runner_config, ask_source,
}
struct SessionContext { // 会话事务:开库、准入、typed 写入器
    state_path, store, promoted_input_id, prompt, initial_parts,
    typed_writer, typed_flush_task,
}
struct RoundContext {   // 单轮身份与编排
    run_id, run_started, run_epoch_ms, orchestration_trace,
    pipeline, write_lease, ctx,
}
```

**验收硬约束:严禁做成一个 28 字段的 `RunContext`。** 那只是把 parameter monolith 换成 context monolith,消掉的 `too_many_arguments` 是假的。判据:对每一个新参数组,能说出它属于哪一层生命周期;说不出就是没分对。

### 事件汇的拆法

`build_event_handler` 现在对每个 `RunEvent` 同时做五件事(UI 投影 / typed 持久化 / trace 落库 / 指标累计 / 运行时状态)。拆成:

```rust
trait RunEventSink { fn on_event(&mut self, event: &RunEvent); }
// UiEventSink / TypedEventSink / TraceSink / MetricsSink
struct RunEventFanout { sinks: Vec<Box<dyn RunEventSink>> }
```

**这是全文件唯一允许引入 trait 的地方**,理由是这里确实有"同一输入、N 个独立消费者"的多态需求;别处不许照抄。

注意 `RunEvent` 现在是**按值**传给闭包(`FnMut(RunEvent)`),而 sink 要按引用广播——`RunEvent` 里 `Text(String)`、`AssistantMessageCommitted { message }`、`ToolEnd { preview, display }` 都带所有权数据。fanout 按 `&RunEvent` 广播、需要所有权的 sink 自己 clone,或反过来让最后一个 sink 拿走所有权。**这是本条唯一有性能含义的决定,选哪种都要在批内实测一次大输出轮次的耗时,不接受估算。**

### 批次

- **批0(纯搬迁,零风险,先做)**:`commands/models.rs` + `commands/summarize.rs`——`models_list`/`build_model_route`/`push_ollama_models`/`summarize_chat`/`fast_summarize` 移出。这五个符号与运行编排零耦合,是"文件越界"最干净的证据。main.rs 的 `invoke_handler` 改全路径注册(照抄第一轮 `files_view.rs` 模式)。
- **批1**:`run/input.rs`——`parse_delivery`/`admit_input`/`promote_next_input`/`code_root_for`。四个纯函数,已有定向测试跟着走。
- **批2**:`run/mod.rs` 建目录,`run.rs` 原样改名进 `run/mod.rs`,不切内容。**单独一批**(与第一轮 store S1 同样的拆壳批)。
- **批3**:`run/assembly.rs`——`assemble_run` + L2025–2258 装配辅助 + `WriterLeaseTrace` 整体搬迁,**`RunAssembly` 暂不三分**。
- **批4**:`run/persistence.rs`——`persist_round_outcome` + `finalize_round`。
- **批5**:`run/execution.rs`——`run_execution_loop` + `run_review_and_fixup` + `build_subagent_runtime`。
- **批6**:`run/events/`——`build_ask_handler` 先搬(无争议),`build_event_handler` 原样搬进 `events/mod.rs`,**暂不拆 sink**。
- **批7**:`commands/run.rs`——`run_prompt`/`stop_run`/`stop_task`/`run_metrics`/`pending_asks_get`/`answer_ask` 移出;`run/coordinator.rs` 收 `run_task`。至此 `run/mod.rs` 只剩 mod 声明与再导出。
- **批8(第一个非纯搬迁批)**:`RunAssembly` 三分为 `RuntimeDeps`/`SessionContext`/`RoundContext`,`assemble_run` 返回三元组,`run_task` 解构后按层传参。消掉 `assemble_run`/`run_task`/`persist_round_outcome`/`finalize_round` 四处 `too_many_arguments`。
- **批9(第二个非纯搬迁批)**:`build_event_handler` 拆 sink + fanout。消掉 `build_event_handler` 一处。
- 批8/批9 各自单独提交、单独全量验证。批0–7 全是搬迁,批8–9 是重构,**顺序不可调换**——先把边界摆对,再动结构。

### 危险点

1. **取根必须在加载配置之前**(`assemble_run` L106–113 的注释,R-177 内容⑧)。worktree 里的 `.kanzei/kanzei.toml` 是 git checkout 出来的分支副本,读它等于让线的行为取决于分支停在哪一代。搬迁时不许调换这两句的顺序。
2. **`project_write_key` 与 `worktree_key` 必须分开取**(L361–389)。前者 = 规范化主根(N 棵树必须相同,否则跨进程单写仲裁被绕过 = lost update),后者 = 代码树(N 棵树必须不同,否则互不相干的树彼此串锁)。"写主根的串行,写代码的并行"——这段注释连同代码一起搬,一个字不改。
3. **`prior` 的恢复必须留在 `run_task` 里**(L1615–1619)。`SessionStore` 非 `Sync`,跨 `await` 持引用会破坏 future 的 `Send` 约束;`conversation::recover_messages` 因此在 `run_task` 同步完成、`run_execution_loop` 只消费 `&[Message]`。**把恢复挪进 execution.rs 会当场 E0277**,不要试。
4. **`on_event` / `ask` 是双 `&mut dyn FnMut` 跨 `await` 重借用**(`run_execution_loop` L925–926)。这与第一轮 `run_once_with_parts` 的危险点是同源问题(见前轮文档 C#2)。批5 只做整体搬迁,任何"顺手抽个函数"的动作都不在批5 做。
5. **`_write_lease` 字段名带下划线但有语义**——它是 RAII guard,`Drop` 补写 `Released` 事件(D-303)。正常路径由 `finalize_round` L1430–1442 显式发 `Released` 并 `mark_released()` 防重复,**且仅非流水线路径发**(流水线路径的租约归编排对象管,再发一条会在轨迹里凭空多出一次释放)。三处配对关系搬散到两个文件后无人可见,`persistence.rs` 顶部要写明。
6. **`typed_flush_task` 是 spawn 出来的弱引用定时任务**,`finalize_round` L1405 `abort()` 它。JoinHandle 从 `assembly.rs` 产出、在 `persistence.rs` 消费,跨模块传递时不能被当成"没人用的字段"删掉——删了就是每轮泄漏一个 250ms tick 的任务。
7. **R-143 的两个 `AtomicBool` 有 swap 语义**(L512–514 置 pending、L547–554 提升 committed)。`round_pending.swap(false)` 是"取并清",不是"读";拆 sink 时这对状态必须整体归 `MetricsSink`,不能一个在 UI sink、一个在 metrics sink。
8. **D-361 的 `subagent_tools` 是跨模块状态**:`build_event_handler` 里边跑边收(L664–666),`run_task` L1691–1696 轮末合并进 `tools_vec` 供鞭挞判定。拆 sink 后 `MetricsSink` 要能把它交出来,否则"整轮把活派给子代理"会被判成空转。
9. `stage` 闭包签名是 `&(dyn Fn(&str, String) + Sync)`,被 `assemble_run`/`run_execution_loop`/`finalize_round`/`maybe_push_after_commit`/`phase_pipeline::start_if_enabled` 五处共用。跨模块后保持这个签名,不要各自改成泛型 `impl Fn`——那会让五处的单态化各来一份。
10. `run.rs` 现有 383 行同文件测试(L2884–3268)按域跟随代码下沉,**不建统一 `tests.rs`**(第一轮结论:统一 tests 会迫使一堆私有项集体 `pub(crate)`,封装白拆)。其中 `cadence指引_全默认空串_显式配置注入`(L3229)跟 `cadence_guidance` 走 assembly.rs。

---

## B. `kanzei-memory/src/memory/store.rs`(R-255)

### 对外契约(已核实)

`memory/mod.rs` L10 是 `mod store;`(私有),L15–18 `pub use store::{...}` 平铺;`kanzei-tools` 再 `pub use kanzei_memory::{memory, ...}`。所以**外部只认 `kanzei_memory::memory::X` / `kanzei_tools::memory::X` 这一层**,把 `store.rs` 切成 `store/` 目录只需改 `mod.rs` 里的 `pub use`,**外部调用点零改动**——与第一轮 R-155 同一条纪律,同样作为每批验收断言(`cargo check -p kanzei-tools -p kanzei-app -p kanzei`)。

外部调用点分布(`MemoryStore::{project,global,open}` 计):`memory/mod.rs` 24、`memory/manager.rs` 11、`memory/tools.rs` 6、`kanzei-app/src/memory.rs` 6、`memory/index.rs` 5、`replay_eval.rs` 1、`kanzei-tools/{profiles,read}.rs` 各 1、`kanzei/src/main.rs` 1。

### 现状符号地图(c164609,生产码 L1–2073)

| 行段 | 内容 | 域 |
| --- | --- | --- |
| 14–110 | `SearchHit`/`RecallRound`/`RecallHit`/`AddOutcome`/`Novelty`/`CandidateReconcileReport`/`decision_weight` | 模型 |
| 111–228 | `MemoryStore` 本体、`open`/`project`/`global`、路径三件、`load_all`、`load_archived_ids`、`archived_count`、`next_id` | **仓储** |
| 229–425 | `add`(**194 行**:枚举校验 → description 必填 → 精确标题去重 → 近似标题判重 → refs 契约 → subject 状态不变量 → 双 scope 指纹探测 → 落盘 → 派生刷新) | **准入** |
| 426–505 | `update`(D-282 两道守卫:description 主题一致性 + CAS `expected_hash`) | 准入 |
| 506–603 | `to_shadow`、`promote`(provenance 硬门禁) | **生命周期** |
| 604–697 | `reconcile_candidates`、`candidate_index_count` | 生命周期 |
| 698–731 | `write_entry`、`archive_dead` | 仓储 |
| 732–863 | `refresh_derived`、`fts_desynced`、`open_db` | 仓储(派生索引) |
| 864–957 | `record_recall`、`mark_recall_fetched`、`recalls` | **检索遥测** |
| 958–1068 | `search`(**BM25 + 决策权重 + active 加权 + 命中计数 + snippet**) | **检索** |
| 1069–1167 | `classify_novelty`、`record_novelty`、`bump_recurrence`、`recurrence_count` | 准入 |
| 1168–1301 | `integrity_issues`、`id_number`、`voided_ledger_file`、`voided_ids`、`void_id` | **完整性/台账** |
| 1302–1421 | `under_git`、`merge`(重复合并 + 墓碑链) | **合并** |
| 1422–1472 | `find_preference`、`upsert_preference` | 偏好(用户直写路径) |
| 1473–1526 | `recall_profile`、`find_by_marker` | 检索遥测 |
| 1527–1568 | `hit_profile`、`hits_map` | **效果画像** |
| 1569–1725 | `read_inbox`/`clear_inbox`/`append_note`/`note_fingerprint_seen`/`pending_note_list`/`discard_note`/`pending_notes` | **收件箱** |
| 1726–1780 | `migrate_legacy` | **迁移** |
| 1781–1950 | `has_tracker_id`、`normalize_title`、`TITLE_DUP_THRESHOLD`、`TITLE_DUP_MIN_COMMON`、`title_tokens`、`title_containment`、`segment_cjk`、`topic_overlap`、`tokens`、`STOP_CHARS`、`is_cjk`、`unsegment_cjk` | 文本原语 |
| 1951–2073 | `intent_query`、`INTENT_BOUNDARY`、`fts_query`、`flush_ascii`、`flush_cjk` | 检索原语 |

### 目标划分

```text
crates/kanzei-memory/src/memory/store/
├── mod.rs           // MemoryStore 本体 + open/project/global + 路径 + load_all + next_id
│                    //   + write_entry + archive_dead + refresh_derived + open_db
├── model.rs         // SearchHit / RecallRound / RecallHit / AddOutcome / Novelty
│                    //   / CandidateReconcileReport / decision_weight
├── admission.rs     // add / update / classify_novelty / record_novelty
│                    //   / bump_recurrence / recurrence_count
├── lifecycle.rs     // to_shadow / promote / reconcile_candidates / candidate_index_count
├── consolidation.rs // merge / find_preference / upsert_preference
├── integrity.rs     // integrity_issues / id_number / voided_* / void_id / under_git
├── retrieval.rs     // search + fts_query + intent_query + CJK 原语
├── telemetry.rs     // record_recall / mark_recall_fetched / recalls / recall_profile
│                    //   / find_by_marker / hit_profile / hits_map
├── inbox.rs         // read_inbox … pending_notes
├── migration.rs     // migrate_legacy
└── text.rs          // normalize_title / title_tokens / title_containment / topic_overlap
                     //   / segment_cjk / unsegment_cjk / tokens / is_cjk / has_tracker_id
```

### 三刀

**第一刀(批 M1–M4,零行为变更、零调用点改动)** —— 全部用纪律 4 的 `impl MemoryStore` 分文件手法。

- **M1**:建 `store/` 目录,`store.rs` 原样改名为 `store/mod.rs`,`memory/mod.rs` 的 `pub use` 路径跟随。**单独一批**,不切内容。
- **M2**:`inbox.rs` + `migration.rs`。这两块与其余部分零共享私有状态,是全文件最干净的迁出面。
- **M3**:`telemetry.rs` + `model.rs` + `text.rs`。
- **M4**:`integrity.rs` + `consolidation.rs`。

**第二刀(批 M5–M6,重构)**

- **M5**:`admission.rs` + `lifecycle.rs` 先按 impl 分文件搬。
- **M6**:准入策略从 `add` 里提成 `MemoryAdmission`——独立可测入口,不经 `add` 也能构造场景。生命周期从 `promote`/`to_shadow`/`reconcile_candidates` 提成 `MemoryLifecycle`。此时 `MemoryStore` 上的 `add`/`promote` 降为薄委托,签名不变(对外契约不变)。

**第三刀(批 M7,需要先定边界 —— 见 D-366)**

- `retrieval.rs` 迁出,并与 `memory/index.rs` 收口。现状是 `MemoryIndex`(检索门面)反过来调 `MemoryStore::search` 取 BM25 层(`index.rs` L222–227,文件头 L14 有说明),排序实现住在 Store 里。**M7 开工前必须先在 D-366 里定下"排序归谁",不允许在批内临时决定。**

### 危险点

1. **`add` 是硬约束的集散地**,不是 CRUD。R-149 的 subject 状态不变量(同 scope+category+subject 至多一条 active,`force` 不可绕)、R-216 的近似标题判重双 scope 探测、refs 来源契约 —— 每一条都有对应的既有测试(`add_近似标题跨状态跨类目判重` L3486、`add_and_note_carry_refs_contract` L3268、`add_拒绝空正文条目` L3464)。M6 抽 `MemoryAdmission` 时这些测试**必须先原样跑绿再重构**,不许"顺手改得更好测"。
2. **`promote` 的证据先落库、后转 active 是硬顺序**(既有测试 `promote_write_evidence_failure_does_not_activate` L2606、`promote_rejects_fabricated_episode_id` L2568、`promote_is_sole_evidence_writer_and_rows_land` L2513)。抽 `MemoryLifecycle` 时顺序不许调换,写证据失败必须阻止激活。
3. **`refresh_derived` 先归档再重建**(L732–774):主目录只留 active/candidate,归档条目不进 `load_all`/FTS/检索,ID 由 `load_archived_ids` 保留永不复用。任何写操作后都要调它。迁出 `archive_dead` 与 `refresh_derived` 时两者必须留在同一文件(`store/mod.rs`),它们是一个原子语义。
4. **`TITLE_DUP_THRESHOLD = 0.55` 与 `TITLE_DUP_MIN_COMMON = 8` 是实测卡出来的**(L1809–1818 注释给了样本:重复的 8 条两两 0.57~0.75,不重复的 M-011/M-012 只有 0.32)。搬进 `text.rs` 时连注释一起搬,**不许顺手调参**——调参是记忆实验,是另一个条目。
5. **CJK 切词在索引侧与查询侧必须同源**(`segment_cjk`/`unsegment_cjk`/`fts_query`,L1856 起的注释)。unicode61 把连续 CJK 当单个整词,两侧切法一旦分家,检索当场退化成"查不到中文"。M3 把 `text.rs` 与 M7 的 `retrieval.rs` 分开时,这组函数的归属要一次定死:**切词原语归 `text.rs`,FTS 查询串构造归 `retrieval.rs`**,`fts_query` 调 `text.rs` 的原语。
6. `open_db`(L817)是唯一的 `Connection` 出口,`fts_desynced` 的"查询失败按未失步处理"是有意的(刚建库的空表不该在检索热路径上制造故障面)。迁出时不要"顺手把错误上报"。
7. **`decision_weight` 的下限 0.6 不清零是有意的**(L100–102:prompt_hints 只注入索引行,"看行即用不拉正文"会被记为未采纳,样本天然有偏)。它是模型层常量,归 `model.rs`,同样不许调参。
8. 2012 行同文件测试按域下沉。`search_ranks_and_records_hits_and_rebuilds_after_db_loss`(L2206)跨 retrieval 与 mod.rs 两域——它验的是"库丢了能重建",归 `store/mod.rs`。

---

## C. `kanzei-app/src/processes.rs`(R-254)

### 现状分区(c164609,生产码 L1–1628;真测试在同级 `worktree_tests.rs` 2437 行)

| 行段 | 内容 | 域 |
| --- | --- | --- |
| 1–18 | **文件头不变式**(`project_dir`/`origin_project` 恒主根) | 见 D-367 |
| 41–239 | `list_pending_inputs`、`cancel_input`、`process_list` | 进程 command |
| 240–489 | `process_create`、`create_process_with_tracker`(L336)、`claim_work_item_for_process`、`create_process_with_work_item` | 进程创建 |
| 490–570 | `bound_error`、`bound_thread_for_worktree`、`stored_bound_thread`、`process_index` | 进程注册表 |
| 571–740 | `register_process`、`next_process_index`、`process_update` | 进程注册表/持久化 |
| 741–918 | `process_close`、`close_process`、`prune_missing_worktree_processes` | **三条生命周期同居** |
| 919–1098 | `reclaim_worktree_on_close`、`worktree_command`、`git_arg_path`、`worktree_key`、`branch_exists`、`rev_parse`、`with_residue`、`worktree_current_branch`、`validate_worktree_path`、`git_worktrees` | **转发壳集中区(D-365)** |
| 1100–1213 | `acquire_project_write_lease`、`write_lease_timeout_error`、`worktree_create` | 工作树生命周期 |
| 1214–1331 | `gate_steps`、`run_gate_step`、`worktree_gate`、`worktree_post_merge_gate` | **集成门禁** |
| 1332–1441 | `parse_harvest_claim`、`tracker_ids_in_text`、`harvest_tracker_candidates_from_messages`、`worktree_harvest_candidates`、`worktree_harvest_writeback` | **tracker 收割** |
| 1442–1651 | `worktree_list`、`worktree_diff`、`merge_worktree`、`worktree_merge_preview`、`worktree_merge`、`merge_worktree_and_release`、`with_idle_bound_process`、`discard_worktree_checked`、`discard_worktree_and_unregister`、`worktree_discard` | **合并/丢弃** |

`too_many_arguments` 四处:L239 `process_create`、L311、L336 `create_process_with_tracker`、L687 `process_update`。

### 目标划分

```text
crates/kanzei-app/src/process/
├── mod.rs
├── registry.rs     // process_index / register_process / next_process_index
│                   //   / bound_* / stored_bound_thread
├── lifecycle.rs    // process_close / close_process / prune_missing_worktree_processes
├── persistence.rs  // process_update 的落库部分
└── commands.rs     // process_list / process_create / list_pending_inputs / cancel_input
crates/kanzei-app/src/workspace/
├── lifecycle.rs    // worktree_create / worktree_list / worktree_diff / reclaim / discard
├── merge.rs        // merge_worktree / merge_preview / merge_and_release / with_idle_bound_process
├── gate.rs         // gate_steps / run_gate_step / worktree_gate / worktree_post_merge_gate
└── harvest.rs      // parse_harvest_claim / tracker_ids_* / harvest_candidates / harvest_writeback
crates/kanzei-app/src/identity.rs   // ProjectRoot / WorktreeRoot(D-367)
```

### 批次

- **批P0**:删 19 处 `wt::` 转发壳(D-365),调用点直接用 `kanzei_tools::worktree`。**先删壳再拆文件**——否则等于把中间态一起搬进新目录。若某个壳确有存在理由(桌面侧额外的路径规范化),保留并写明理由,不许"看着像转发就删"。
- **批P1**:`ProcessSpec` —— `create_process_with_tracker` 的 9 个参数(model/profile/reasoning/phase_pipeline/tracker_writes/worktree_name/work_item_id/…)本来就是同一个配置概念,收成 request object 是合理的(**注意这与 A 节 `RunAssembly` 的情况不同**:那 28 个字段属于不同生命周期,收成一个 struct 是错的;这 9 个属于同一个概念,收成一个 struct 是对的)。消掉 `process_create`/`create_process_with_tracker` 两处 `too_many_arguments`。
- **批P2**:`workspace/gate.rs` 迁出。集成门禁与进程管理零共享状态,是最干净的一刀。
- **批P3**:`workspace/harvest.rs` + `workspace/merge.rs`。
- **批P4**:`workspace/lifecycle.rs`,`processes.rs` 只剩进程侧。
- **批P5**:`process/` 目录四分。
- **批P6(D-367)**:`identity.rs` 引入 `ProjectRoot`/`WorktreeRoot` newtype,`ProcessHandle` 与直接调用点改用。范围**只到 processes.rs 与其直接调用点**,不做全仓路径类型统一。

### 危险点

1. **文件头 L3–18 的不变式是四处反推主根逼出来的**,不是风格偏好:`p{n}` 编号按 `project_dir` 分桶(改存 worktree 则每棵树各自从 p1 开始,编号撞车);`process_update`/`process_close` 用 `project_dir` 反推 root 开 `state.db`(改存 worktree 会把库落进工作树,线一关连库一起没);`state.rs` 的 `process_info` 用 `project_dir` 算 `session_id`(改存 worktree 等于换身份串,会话历史集体失联,D-176 红线)。批P6 之前,这段注释必须原样跟着 `ProcessHandle` 走。
2. **`process_close` 同时收三条生命周期**(逻辑进程 / 执行运行时 / 工作区):halt runtime → clear ask → remove auto-run → update process/session → unregister → 后台进程清理 → 工作树清理 → 落库 → 事件。批P4 拆工作区侧时,这个顺序是硬约束——工作树清理必须在 unregister 之后、落库之前,否则崩在中间会留下"注册表里没有但树还在"的孤儿。
3. `worktree_post_merge_gate`(L1316)是**合并后在主根再跑一次全量**,与 `worktree_gate`(线内跑)不是同一件事。迁进 `gate.rs` 后两者要分别留注释说明触发点,别被后人合并成一个函数。
4. `with_idle_bound_process`(L1569)是合并前的"线必须空闲"守卫,`merge_worktree_and_release` 依赖它。两者同批迁移。
5. `worktree_tests.rs` 是 `#[path]` 挂进来的外挂测试模块(processes.rs L468 的 `#[cfg(test)]` 就是它的声明,这也是"找第一个 `cfg(test)` 一刀切"会把 1628 行生产码误报成 467 的原因)。拆文件后它的 `use super::*` 要跟着改,**每批 `cargo test -p kanzei-app` 不可省**(`cargo build` 看不见测试编译错误,第一轮 A#6 同一条教训)。

---

## D. 排期与互斥

```text
R-258(度量口径,P1,中)   ← 先做:它产出的基线快照是 R-253/R-254/R-255 的验收对照
        ↓
R-253(run.rs,P0)  ─┐
R-255(store.rs,P0) ─┼─ 三条**串行**,互不并发,也不与其他源码条目并发
R-254(processes,P1)─┘
        ↓
R-256(Desktop/CLI 共用 RunService,P1)  ← 前三条边界稳定后再开
        ↓
R-257(drive.rs / docstore.rs / git.rs / config.rs,P2)
```

R-258 排在最前不是因为它重要,是因为**没有它,前三条的验收("生产行数 ≤ N")只能靠手工数**。它是中等复杂度、可以一轮做完的条目。

D-365 并入 R-254 批P0;D-366 必须在 R-255 批M7 之前定论;D-367 并入 R-254 批P6。D-364(并发写丢条目)与本轮拆解正交,但**它没修之前,所有条目状态更新都要"写完立刻复核"**。

## 技术选型与取舍

| 选择 | 备选 | 理由 |
| --- | --- | --- |
| 装配层按生命周期三分 | 单个 `RunContext` 收 28 字段 | 后者只是把 parameter monolith 换成 context monolith,`too_many_arguments` 消掉了但耦合没减 |
| 事件汇用 trait + fanout | 继续单个 giant reducer / 按事件类型切文件 | 这里确实是"同一输入 N 个独立消费者";按事件类型切文件解决不了"一个 RunEvent 同时改五处状态" |
| `ProcessSpec` 收成 request object | 保持参数列表 | 那 9 个参数本来就是同一个配置概念——与 `RunAssembly` 的情况相反,判据是"是否同一生命周期/同一概念" |
| 先删转发壳再拆文件(批P0 最先) | 先拆文件再删壳 | 否则等于把 R-207 的中间态原样搬进新目录,债务换了个位置 |
| `impl` 块分文件(第一刀) | 直接抽独立类型 | 前者调用点零改动、可单批回滚;把"分文件"与"换所有者"塞进一批是最容易翻车的地方 |
| 主根/工作树根用 newtype | 继续靠文件头注释 | 后果全是运行时重症(编号撞车、state.db 落错位置、会话身份断裂),注释挡不住,类型可以 |
| 测试随域下沉 | 统一 `tests.rs` | 沿用第一轮结论:统一 tests 迫使私有项集体 `pub(crate)`,封装白拆 |

## 实施边界与调用方

- R-253 触碰 `crates/kanzei-app/src/{run.rs → run/, commands/}` 与 `main.rs` 的 `invoke_handler` 注册区;R-255 触碰 `crates/kanzei-memory/src/memory/{store.rs → store/, mod.rs}`;R-254 触碰 `crates/kanzei-app/src/{processes.rs → process/, workspace/, identity.rs}` 与 `worktree_tests.rs` 的 `use super::*`。
- 全程零行为变更、零对外契约变更;`ui/*.js` 一行不改;发布节奏不受影响(任意批次间可发版)。

## 变更记录

- 2026-08-15 初版:用户第二轮巨石扫描 + 机器复核生产行数 + 逐文件读码,汇总成三条 P0/P1 的批次计划,交自举执行。

## 验证证据

TODO(各条目交付时回填):每批的定向验证记录;条目关闭前的 `cargo test --workspace` 全绿记录;外部再导出面零变更断言;拆解前后**生产行数**对照表(按 R-258 口径,不用 `wc -l`)。

## TODO 与后续风险

- R-258 的度量入口未落地前,本文表格里的行数需要手工复核(计算脚本见本文"行数口径"节的定义)。
- R-253 批9(事件 sink)的 `RunEvent` 所有权取舍需要实测数据,批内不得靠估算拍板。
- R-255 批M7 依赖 D-366 的边界结论,结论未定不得开工。
- `drive.rs` 1826 生产行的处置(拆或不拆)是 R-257 的第一件事,结论出来后回填本文。
