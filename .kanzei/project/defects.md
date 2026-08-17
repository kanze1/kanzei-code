# Defects

## D-486 R-242 shadow 比较器将压缩后 legacy surface 误判为 unknown mismatch [fixing] (medium)
- 复现: 真实项目执行 `cargo run -p kanzei -- shadow --project-root (Get-Location).Path --mismatches`：最新窗口出现 `typed_write_errors=[]` 但 `projected_messages=151`、`legacy_messages=13`、`first_mismatch=1`、`expected_mismatch=false`；该窗口在事件日志中包含多轮 typed facts 与一次 `conversation.updated`，legacy 是压缩后的短 surface。现有 `classify_mismatch` 只识别 legacy 为空、legacy 为 projection 前缀和失败 diagnostics，不识别压缩后的 legacy surface。
- 影响: R-242 的 shadow gate 将可解释的 surface compaction/快照重建差异计为 unknown mismatch，真实窗口无法区分投影写入错误与 compaction 尚未事件化，阻碍建立有效的 30 turn typed_write_errors=0 统计窗口。
- 来源: self-found：R-242 真实 shadow 诊断；项目 state.db 最新 shadow 事件与 `crates/kanzei-core/src/store/typed.rs:1453-1483` 代码对照。
- 标签: 核心
- 验收: 新增回归覆盖 legacy 是 projection 的有效尾部/压缩后 surface 时标为 expected_mismatch（compacted_snapshot），仍保留真正中间内容不一致为 unknown；`cargo test -p kanzei-core` 通过；真实 shadow 输出不再把该类差异计入 unknown。
- refs: R-242
- 优先级: P1
- 状态: fixing
- 进展: 已实现并验证分类修复：`crates/kanzei-core/src/store/typed.rs:1478-1488` 在 legacy 精确等于 projection 尾部时标记 `compacted_snapshot`，中间不一致与 legacy 反超仍返回 unknown；回归位于 `typed.rs:2239-2252`，T-1786922726218（kanzei-core 222 passed）通过。真实 state.db 诊断已确认触发场景：最新 shadow 事件 projected=151、legacy=13、typed_write_errors=[]，但历史事件不会自动重写；待下一次真实 shadow turn 产生新事件后复核统计，再满足“真实 shadow 输出不再计入 unknown”后关闭。
- observed_head: dcf6e11c4a0557ad9283234084a431bf61f3e083
- observed_worktree_hash: fnv1a64:b5a0bda6129c84a4
- recorded_at: 1786996867134
- 停车: 代码修复与 `cargo test -p kanzei-core` 已完成；本轮先让位给 R-242 建立真实 shadow 验证窗口，待新 shadow 事件产生后恢复并复核 unknown 统计。

## D-504 鞭挞配置双真源与 autoRounds 双计数器,四副本靠手工互拷同步 [fixing] (medium)
- 复现: crates/kanzei-app/ui/08-compose.js:1088-1097 lineAutoConfig 活动线读 DOM 复选框、其他线读 processAutoState Map;同状态另存 localStorage(kz-process-auto-state) 与后端 ui_prefs/auto_state_update(:1014-1021,:1057);autoRounds 全局(:4)与 state.auto_rounds(:337,:380) 靠 07-events.js:439/449/465 手工互拷,:1078 切线再读回
- 影响: 四副本两条同步路径,漏一处即显示 0/10 实际下一轮撞上限;历史已翻车两次
- 来源: 2026-08-18 全库勘察(主会话);D-290/D-353 历史翻车点
- 标签: 前端
- 验收: 收敛单一真源(Map/state),DOM 只做投影;切线/后台线/重启回归用例;冒烟覆盖
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-504
- 进展: 本批已提交：`8f490d92 D-504 收敛自动推进轮次与线路配置真源`。①单一真源：`08-compose.js:8-15,97-100,1084-1098` 以 `sessionState(sessionId).auto_rounds` 与 `processAutoState` 为真源，DOM 仅投影；②后台线：`07-events.js:375,381,389,439,441,449,454,465,470,477` 统一按 `currentAutoRounds(p.sessionId)` 写入，不再使用活动线镜像；③切线/后台回归：`scripts/ui-runtime-smoke.mjs:1404-1415,5098-5103,5136-5140,5161-5166` 覆盖 Map 优先、后台甲、后台连跑第二轮和切线回显；④重启初始化：`scripts/ui-runtime-smoke.mjs:1510-1513` 覆盖 `ui_prefs` 启动恢复；T-1786922726295 当前暂存源码六项前端冒烟通过，T-1786922726296 `cargo test -p kanzei-app` 为 205 passed。验收缺口：尚未有真实已安装桌面应用退出→重启→读取持久化状态的可重放证据；因此 D-504 保持 fixing，下一步建立真实重启链路或明确验收降级，不关闭。
- observed_head: 8f490d92856e1e0208efee838b55b18254d6c883
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1787008227966

## D-505 收活合并门禁用 CSS class 当闸门状态 [open] (medium)
- 复现: crates/kanzei-app/ui/20-lines.js:788 postMergeStep.classList.contains(confirmed) 决定能否回写 tracker,是 R-222 前置(合并后全量通过)的唯一判据
- 影响: 任何重渲染或样式重构都能抹掉或伪造闸门状态
- 来源: 2026-08-18 全库勘察(主会话)
- 标签: 前端
- 验收: 闸门状态入 JS 状态对象,class 只做展示;回归覆盖重渲染场景
- 优先级: P2

## D-506 桌面端热路径 15 处 std Mutex lock().unwrap(),一处持锁 panic 即毒化级联应用僵死 [open] (medium)
- 复现: crates/kanzei-app/src/state.rs:198/233/576/671/737、processes/registry.rs:65/262、run/coordinator.rs:53/61/101、run/persistence.rs:175/487、mobile.rs:341/369/424 均 .lock().unwrap();仓内已有正确写法未铺开(orchestration_trace.rs:41-44、kanzei-core/src/store/mod.rs:69 用 into_inner 恢复)
- 影响: 任一处持锁 panic 把锁永久毒化,之后每个 Tauri 命令跟着 panic,整个应用僵死
- 来源: 2026-08-18 全库勘察(主会话)
- 标签: 后端
- 验收: 统一改 into_inner 恢复(或等效策略);15 处全覆盖;防回归手段(clippy lint 或巡检)
- 优先级: P2

## D-507 记忆遥测口径批次:injected 恒真/promotion_gaps 漏查/Tier0 无 hits/23% recall 悬空 [open] (medium)
- refs: R-235
- 复现: crates/kanzei-memory/src/memory/tools.rs:107-114 memory_search 无条件 injected=true(precision 恒 1.0);crates/kanzei-app/src/memory.rs:53-59 promotion_gaps 用 source/refs 空判冒充 provenance 检查不查 memory_sources(28 条 source=user 零证据 active 不计入);index.rs:300-310 Tier0 指纹命中直接 return 不记 record_hits 且 SearchHit 空;kanzei-core/src/store/telemetry.rs:136-147 episode 回填仅限 append_episode 成功,803/3537 行 recall_events 悬空
- 影响: 漏斗对 memory_search 无信息量;控制面缺口数偏低;指纹通道画像恒 0;23% 召回无法 join episodes
- 来源: 2026-08-18 全库勘察(主会话)
- 标签: 后端
- 验收: 四处口径各自修正并有测试;生产数据可复算;控制面数字与库中一致
- 优先级: P2

## D-508 工具事件落库每事件新开 SessionStore 连接(D-374 未铺到 record_live_trace_at_path) [open] (low)
- 复现: crates/kanzei-app/src/state.rs:372 record_live_trace_at_path 每次 SessionStore::open,7 处调用点
- 影响: 每事件约 4.3ms 白烧,长会话工具密集时可感
- 来源: 2026-08-18 全库勘察(主会话);audit_20260812_eight_dimensions.md:32 曾建议顺 D-297 做,D-297 已关闭未做
- 标签: 后端
- 验收: 复用连接;修后耗时对比留档
- 优先级: P2

## D-509 启动步骤等 37 处中文字面量绕过 i18n,i18n 冒烟结构性盲区 [open] (medium)
- 复现: crates/kanzei-app/ui/18-startup.js:40,47,59-63 七个 label 经 :35 toastError 直出中文;16-settings.js:755 回环、08-compose.js:196,293 线路已关闭等 JS 侧共 37 处中文字面量未包 t() 也不在词表;scripts/ui-i18n-smoke.mjs:10-12 只校验 t(key) 的 key 在词表、:16-26 只扫 index.html
- 影响: 英文态启动失败时唯一可见信息是中文;冒烟绿不等于覆盖
- 来源: 2026-08-18 全库勘察(主会话)
- 标签: 前端
- 验收: 37 处入词表走 t();冒烟新增 JS 中文字面量未包 t() 的检查;i18n 冒烟通过
- 优先级: P2

## D-510 verify 步骤空集假绿与提交门禁只报首个失败 [open] (medium)
- refs: docs/design/ci_release_evidence_chain.md
- 复现: scripts/verify.ps1:25 Step-With-Timing 靠 LASTEXITCODE 判定,:44-49 ui 目录为空时 ForEach-Object 一次不执行沿用上一步 cargo test 的 0 直接 pass;crates/kanzei-tools/src/git.rs:893 fmt/clippy 已并行跑却在 :894-899 只返回第一个 Err
- 影响: 假绿风险;提交阶段聚合报告缺位
- 来源: 2026-08-18 全库勘察(主会话)
- 标签: 流程
- 验收: 空集显式失败;git.rs 侧聚合全部失败一次报出;守护测试(git.rs:1896)不回归
- 优先级: P2

## D-511 CDP 退役残留清理:e2e-smoke.mjs 与 probe-webview-cdp.mjs [open] (low)
- refs: R-101
- 复现: scripts/e2e-smoke.mjs:1,44 仍是 chromium.connectOverCDP;scripts/probe-webview-cdp.mjs 整份仍在
- 影响: 退役路线代码残留误导后续维护
- 来源: 2026-08-18 全库勘察(主会话);R-101 技术路线 2026-08-17 已宣布 CDP 退役
- 标签: 流程
- 验收: 删除或按新路线改造;verify/文档无 CDP 引用残留
- 优先级: P3

## D-512 前端死代码与孤儿引用批次清理 [open] (low)
- 复现: 零调用函数四个:crates/kanzei-app/ui/15-views-misc.js:698 renderConversationList、08-compose.js:64 phasePipelineOn、05-chat-render.js:303 toolIconId、06-agent-panel.js:42 agentToolType;03-shell.js:290-296,356,366 三处 #sidebar-toggle 残留(元素已删,真身是 #rail-sidebar-toggle);06-agent-panel.js:372 与 16-settings.js:423 kz:fast-setup 双订阅;22-neural-flow.js:391 全仓唯一 window 挂载符号配 24 处 ?. 噪声守卫
- 影响: 死代码误导维护;双订阅每事件多跑一遍路由前置
- 来源: 2026-08-18 全库勘察(主会话,487 个顶层函数跨文件引用计数)
- 标签: 前端
- 验收: 清理后重生成 ui-lint-globals;kz:fast-setup 单订阅;neuralFlowEmit 改顶层声明或统一口径;六冒烟全绿
- 优先级: P3

## D-513 后端静默失败与死抽象批次清理 [open] (low)
- 复现: kanzei-core/src/store/session.rs:36,158,187 VACUUM/备份删除 let _ 无痕迹(常年失败库膨胀也无从发现);kanzei-app/src/state.rs:684-703 stop 兜底 detach 线程睡 30s 句柄丢弃且期间重开 SessionStore;kanzei/src/cli/tracker.rs:117 无说明 unreachable!;kanzei-app/src/phase_pipeline.rs:253,475 roster_cap 静默截断角色表无诊断;kanzei-core/src/notification.rs:7 InMemoryBroker 零生产消费方
- 影响: 维护性失败无痕迹;停止不干净无迹可循;死抽象误导
- 来源: 2026-08-18 全库勘察(主会话);InMemoryBroker/roster_cap 为 audit_20260812 遗留项
- 标签: 后端
- 验收: 失败路径留 tracing;stop 兜底可观测;unreachable 带理由;截断有诊断;死抽象删除
- 优先级: P3
