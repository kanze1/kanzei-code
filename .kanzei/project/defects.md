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

## D-499 background.rs 日志泵同步阻塞写+全量重写 O(n^2)+full_output 与注册表无界增长 [fixing] (high)
- 复现: crates/kanzei-tools/src/background.rs:213 tokio::spawn 的日志泵内 :237/:249 调用同步 write_atomic(全同步 create_dir_all+临时文件+fsync+rename,kanzei-base/src/atomic_file.rs:40);:229-239 每 5s/64KiB 把累积全量 buffer 覆写整文件;:68 full_output 只 extend 从不裁剪(对照 output 走 append_bounded :141);:131 全局注册表只有插入(:288/:631)无 remove,:665 list 含已退出;adopt 路径 :597 整份日志一次读进内存
- 影响: 卡 tokio worker;长跑 dev server 日志 100MB 时每次刷盘写 100MB(总写入 O(n^2));常驻进程内存无限增长,每个跑过的后台进程永久驻留
- 来源: 2026-08-18 全库勘察(主会话)
- 标签: 后端
- 验收: 写盘异步化或专线;改增量追加;full_output 设上界;已退出进程可回收;回归测试覆盖
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-499
- 进展: 实现与定向验证已完成，待提交并关闭：①写盘异步化：`crates/kanzei-tools/src/background.rs:141-179,248-281` 使用 `tokio::fs::OpenOptions` + `append_log_chunk`，日志泵只追加 pending chunk，不再调用同步 `write_atomic` 全量重写；②增量追加：同文件 `:248-281` 按 64KiB/2s 节流追加，退出前追加剩余块，磁盘日志保持完整顺序记录；③full_output 上界：同文件 `:23-26,68-70,262` 通过 `append_bounded` 限制为 4MiB 尾部，`process.rs:112-136` 明示内存尾部与磁盘完整日志的区别；adopt 用 `background.rs:181-206,626` 的异步尾部读取，避免整份日志进入内存；④已退出进程回收：`background.rs:302-319` 在注册表插入后启动 wait，终态移除内存条目并清理 persistent registry；adopt pid watcher `:630-645` 同样清理，保留既有 discover/kill 消费链；⑤回归：`background.rs` persistent 日志、full_log 上限、自然退出、stop、discover/adopt/kill 测试均通过；T-1786922726275（background 24 passed）与 T-1786922726276（kanzei-tools 342 passed/1 ignored）。下一步提交后按逐条证据关闭 D-499。
- observed_head: 29653d8381db33f81ed37952de13153536208ea5
- observed_worktree_hash: fnv1a64:62daaa69ba055053
- recorded_at: 1787005970578

## D-500 Embedder::embed 每次新建 tokio Runtime,async 上下文调用直接 panic,且逐条调用浪费批量签名 [open] (medium)
- 复现: crates/kanzei-memory/src/embed.rs:95-98 同步 trait 内 Runtime::new+block_on,async 上下文调用报 Cannot start a runtime from within a runtime;调用方 index.rs:193/404/653 在检索/重建路径;vectorize(index.rs:190-193) 每条 entry 单独调 embed(&[&text]),浪费 &[&str] 批量签名
- 影响: 每次调用起整套 worker 线程+IO driver;hybrid 检索一旦启用(R-294 路线拍板)即引爆;全量 rebuild N 次 HTTP 往返
- 来源: 2026-08-18 全库勘察(主会话)
- 标签: 后端
- 验收: 共享 runtime 或改 async 接口;vectorize 批量化;async 上下文调用有定向测试
- 优先级: P1
- refs: R-294

## D-501 移动端交付游标持久化失败仍前进,重连后重复收事件 [open] (medium)
- 复现: crates/kanzei-app/src/mobile.rs:588-590 let _ = store.set_delivery_cursor(...) 丢弃错误后无条件 cursor = event.sequence
- 影响: 写库失败时内存游标与库中游标分叉,重连后按库中旧游标重放,手机端重复收事件——数据正确性问题
- 来源: 2026-08-18 全库勘察(主会话)
- 标签: 后端
- 验收: 持久化失败不前进内存游标(或重试并告警);故障注入测试覆盖
- 优先级: P1

## D-502 移动端 SSE 每 300ms 轮询与每条事件各开一次 DB 连接 [open] (medium)
- 复现: crates/kanzei-app/src/mobile.rs:578 每 300ms 轮询开一次 SessionStore::open,:588 每条事件再开一次;HTTP 请求路径 :265/:270 一次请求开两条;团队自测一次 open 约 4.3ms(run/events/mod.rs:92-98,D-374 已为 run trace 做连接复用)
- 影响: 每台配对设备一条常驻线程按此频率烧连接,零收益;132MB 库含 migrate+housekeeping 查询
- 来源: 2026-08-18 全库勘察(主会话)
- 标签: 后端
- 验收: 连接复用铺到 mobile 全路径;轮询循环单连接;修后耗时可量化对比
- 优先级: P1

## D-503 设置页 models_list/fast_model_status 失败被 catch return 静默吞 [open] (medium)
- 复现: crates/kanzei-app/ui/16-settings.js:339 catch{return}(models_list 失败模型下拉停旧值),:404 同款(fast_model_status 失败状态行不更新);:393-395 注释自陈全部静默失效而界面毫无线索
- 影响: 后端失败时用户无从判断,模型下拉与安装按钮状态不明
- 来源: 2026-08-18 全库勘察(主会话)
- 标签: 前端
- 验收: 两处失败均有用户可见反馈(toast 或状态行);冒烟断言
- 优先级: P2

## D-504 鞭挞配置双真源与 autoRounds 双计数器,四副本靠手工互拷同步 [open] (medium)
- 复现: crates/kanzei-app/ui/08-compose.js:1088-1097 lineAutoConfig 活动线读 DOM 复选框、其他线读 processAutoState Map;同状态另存 localStorage(kz-process-auto-state) 与后端 ui_prefs/auto_state_update(:1014-1021,:1057);autoRounds 全局(:4)与 state.auto_rounds(:337,:380) 靠 07-events.js:439/449/465 手工互拷,:1078 切线再读回
- 影响: 四副本两条同步路径,漏一处即显示 0/10 实际下一轮撞上限;历史已翻车两次
- 来源: 2026-08-18 全库勘察(主会话);D-290/D-353 历史翻车点
- 标签: 前端
- 验收: 收敛单一真源(Map/state),DOM 只做投影;切线/后台线/重启回归用例;冒烟覆盖
- 优先级: P2

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
