# D6 Harness 剩余三文件依赖地图

基线 `5cdfe5a3787697c19a27d50562de103fd3b101bd`，分支 `kanzei/audit-d6-harness-rest`，复用干净 A 树。全文顺序 areas→auto_run→tool_pipeline，含全部测试，三文件均 PASS。只有这三文件计本包全文；下列 caller、module/Cargo 与历史合同是必要只读切片，不计新全文。Root 历史 pipeline 预读未计覆盖，本包独立确认。源码/API 未修改，无新增依赖/框架/持久格式。

## 实际依赖与 owner

harness/Cargo.toml 内部生产依赖只有 base；三个模块由 lib.rs 公开，HarnessIntensity/IntensityPolicy 另作 reexport。areas 的真源是项目 Cargo 清单及代码目录，AreaRegistry 每次扫描构建派生索引，不写文件。auto_run 是无 IO 的状态机，AutoRunController 在 app 的 session→controller Mutex 中拥有状态，轮末读真实 backlog/进展/事件后同步 decide；持久 session/事件不由此模块写入。pipeline 只拥有一次 future 的前置 halt/progress scope/结果顺序，运行中 cancel 由 runner select/drop；真正进程清理与数据库终态由各实际 caller 承接。

## areas 全 public API 与实际 caller

- Area/AreaKind/as_str、AreaRegistry scan/all/get/crate_of/crate_deps/resolve_token/nearest/watch_dirs：memory/manager.rs resolve_areas（memory_add/update 192/364）；app/memory.rs resolve_area_tokens→memory_entry_save（535）；tools/refgraph/memory_graph.rs path_areas、explicit/ref/tool 区域与图构建（181/374/402/463/551/571/612/623/641/653/705），collect_inputs_from（761），input_fingerprint_from（907）。内置测试与 tools/refgraph/tool_areas.rs/tests.rs 有真实测试 caller。
- workspace_crate_deps：app/docs.rs build_workspace_graph（898/899）→docs workspace projection（870）；script_stem、BANDS 只在本模块及其测试使用；is_empty 当前没有外部生产 caller。AreaRegistry 的排序后索引、crate alias、最长依赖深度及 cycle guard 均核对。
- graph cache：app/memory.rs memory_graph_with（307）每次先计算 fingerprint（312），按 root+fingerprint 命中，只缓存派生图；input_fingerprint_from 每次重新 scan，再对 watch_dirs 的条目名和清单 stat 取指纹。watch 包含根、member/src、crate 前端和顶层代码目录；新增目录能在下一次扫描被纳入。
- resolve_token 的 canonical area 未知模块拒绝；仓内 Rust 文件/Rust path 未知模块回退 crate 是现有 resolve_token 测试明确的合同。未知区域整体拒绝且 nearest 只作提示。读不到扫描输入降级空 registry 是 scan 的明确只读合同，未见以空集合覆写持久事实的路径。

## auto_run 全 public API 与实际 caller

- AutoRunState new/reset/decide、AutoRunCtx、RoundFailure、BacklogStatus、AutoRunAction/AutoStopReason：app/auto_run.rs AutoRunController/decide_auto_run（39/135）；app/run/coordinator.rs 失败 ctx（415）与成功 ctx（569）；app/research_auto.rs decide（80/122）。CLI 当前不直接组装 AutoRunState/AutoRunCtx，不能按旧模块注释推断共用循环。
- WorkPriority、SelectedItem、NudgeFacts/nudge_prompt：tools/work.rs 与 work/context.rs 的取活选择、app/auto_run.rs 事实/载荷、CLI cli/work.rs 与 cli/run.rs、ToolCtx；verify_prompt 在本模块和 app 转发调用。has_progress_tools/NON_PROGRESS_TOOLS 由 FSM 使用，app events 内置测试验证子工具上卷；app/run/events/mod.rs TaskProgress（933）收集子工具，coordinator（512）并入本轮工具画像。
- HarnessIntensity as_str/parse/policy/IntensityPolicy：ToolCtx 与 core runner config/runtime、app run/memory_chat/subagents、CLI run；core/runner/drive/assembly.rs（201）按 redundancy_hints 建 RedundancyWatch，FSM 用 engine_nudge/backlog_stops_loop/verify_rounds。阈值常量主要由 FSM 和 app 失败/零输出文案调用；完整索引保留于 ignored evidence。
- 成功/失败决策从当前 controller 取暂停/目标意图；completion 由 handoff.bound_scope 校验当前 input/goal 且 pending inputs 不存在（coordinator 590）；GoalMet/GoalUnreachable 由 caller 清目标（628）。Research 只在 workflow runnable 后放行，改 paired+goal_active，并关闭 completion/verify 插队。
- backlog→profile→halt→用户暂停/本轮后停→失败→bound completion→awaiting_user→目标/无动作→真实签名→核查的优先级与现有测试/设计一致。失败三次停止；成功清失败计数；签名基线后连续三次不变停；verify=0 关闭。max_rounds 为旧配置兼容字段不再停机，测试明确。stop_after_round 保留字段由 caller 消费：UI 08-compose-runtime.js 后台 StopAfterRound（243）对所属 session 清一次意图，启动不持久化（906）；FSM 不擅自改变控件意图。

## pipeline 全 public API 与实际 caller

- ToolGuard：唯一生产实现 tools/bash.rs FullFileWriteGuard/CommandLengthGuard/GitMutationGuard（33/60/87），bash_guards 保持顺序；run_tool_pipeline：tools bash/read/glob/grep/git 的 execute（249/78/51/78/104），body 为单独实现 future，只执行一次。其余四工具 guards 空；全部生产 callers 结果 policies/observers 为空。ToolResultPolicy/ToolObserver/ToolPhase 的实现/引用当前仅模块测试；TimedOut 由 with_timeout 的 Err 匹配使用，未见外部业务状态。
- wrap_execute：core/runner/drive/serial_tools.rs（194）、tool_exec.rs（666）、subagent.rs（503）、drive.rs host wait（372）。串行通过 halted 前置检查+halt select（212），并行由 drive/parallel_tools.rs wave select（184）drop 后返回每 call 一个取消 ToolResult；host wait cancel 调 stop_all。managed_fence::tool_scope 在外层，wrapper 不冒充持久终态。
- with_timeout：tools/bash.rs（468）超时杀树并返回实际部分输出与围栏结果（538）；background.rs send_input（200）明确部分字节可能已发；core compaction.rs（599）限整个流，不完整/错误结果返回 None 交既有 compaction fallback。panic body 在 wrap_execute 内转一次 TOOL_PANICKED 并释放 progress scope；observer panic 被捕获不改 final result。没有生产 result policy/observer panic 可达证据，不把外部回调理论异常记问题。

## 历史合同与验证边界

已读 dependency-map.md、实际 lib/Cargo；相关历史 slice：memory_knowledge_graph.md §4/5、deepseek_harness_upgrade.md Tool Pipeline、context_compaction.md 当前 core 路径修订、model_autonomy_and_harness_intensity.md 资源/任务区别及 B2/B3、continue_prompt_dissection.md 2026-08-21 修订。旧 CLI 共用/MaxRounds 注释不作为当前行为证据。

本包不 Cargo/build/shared target；独立 rustc 直接编译实际 areas.rs，隔离 HOME/USERPROFILE/KANZEI_HOME/AppData/TEMP，CARGO_MANIFEST_DIR 指 A harness：编译0，内置5测试全通过。auto_run 42、pipeline 9 测试全部人工全文读取，本包未执行。root 实际 harness214测试全通过，三 D6 source LF独立比对相等（root-source-association.json）；root 全仓/native/UI尚未收口，完整certificate待root完成，不能算整仓通过。PASS 无代码变更，无负例或新增镜像测试。ignored output/parallel-D6 保留 caller 索引、初始与最终 hash、真实 standalone 日志。

## 全文源码 hash

- `crates/kanzei-harness/src/areas.rs`：925 行，测试全文 5；SHA256 `17ecc2e9826235befbc5aab65e2fd77eaa83454559074dd632791127b44bd10a`；SHA256_LF `deed3769d62452caa9ed19f0c8ee51f0c5abdb0b2abe036ce7aefe091fb73789`。
- `crates/kanzei-harness/src/auto_run.rs`：1766 行，测试全文 42；SHA256 `fe919618a5a83bb84328b71feb803b3ef59cb59c976cc33e1082345ee4708b9e`；SHA256_LF `df970870be88913f29e26cbf5d55b78860bb72fed89caa2a21652726ef125d7a`。
- `crates/kanzei-harness/src/tool_pipeline.rs`：439 行，测试全文 9；SHA256 `a5ee5b83bad24b308a8afbb6f920d93f6a4d9dfa8898d2d8c941f791f967ff26`；SHA256_LF `5c801b7d4524581fd03d41fc1f8280602eef4a8d7502e4ad7bf5ac375b1a5626`。
