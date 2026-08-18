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
- 进展: 实现提交 `8f490d92` 与自动化证据已完成。已确认真实安装位 `C:\Users\kanzei\AppData\Local\kanzei\kzapp.exe` 存在且当前进程正在运行；当前窗口显示用户正在使用该应用，按发布规则不得强杀或擅自关闭。因此最后一项“已安装桌面应用退出→重启→读取持久化状态”暂记外部阻塞，待用户关闭窗口后执行真实重启链路；其余验收保持已通过。
- observed_head: 8f490d92856e1e0208efee838b55b18254d6c883
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1787008359348
- 阻塞: 真实重启验收需要关闭当前正在运行的已安装 `C:\Users\kanzei\AppData\Local\kanzei\kzapp.exe`；解除人：用户关闭当前 kzapp 窗口后，由 agent 重新启动同一安装位并回读持久化 auto state。

## D-513 后端静默失败与死抽象批次清理 [fixing] (low)
- 复现: kanzei-core/src/store/session.rs:36,158,187 VACUUM/备份删除 let _ 无痕迹(常年失败库膨胀也无从发现);kanzei-app/src/state.rs:684-703 stop 兜底 detach 线程睡 30s 句柄丢弃且期间重开 SessionStore;kanzei/src/cli/tracker.rs:117 无说明 unreachable!;kanzei-app/src/phase_pipeline.rs:253,475 roster_cap 静默截断角色表无诊断;kanzei-core/src/notification.rs:7 InMemoryBroker 零生产消费方
- 影响: 维护性失败无痕迹;停止不干净无迹可循;死抽象误导
- 来源: 2026-08-18 全库勘察(主会话);InMemoryBroker/roster_cap 为 audit_20260812 遗留项
- 标签: 后端
- 验收: 失败路径留 tracing;stop 兜底可观测;unreachable 带理由;截断有诊断;死抽象删除
- 优先级: P3
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-513
- 批次: 1/4
- 进展: 批1/4 已完成，待提交：`crates/kanzei-core/src/store/session.rs` 已将 `open` 的 housekeeping 失败、housekeeping_at 写入失败、VACUUM 失败、过期迁移备份删除失败、覆盖旧备份删除失败分别改为带 `error`/`path`/上下文的 `tracing::warn!`；首次备份的 `NotFound` 仍不告警，保持既有正常语义。原有备份保留与 VACUUM 回归继续通过，T-1786922726326：`cargo fmt --all -- --check; cargo test -p kanzei-core`，227 passed。下一步批2：读取并修改 `crates/kanzei-app/src/state.rs` stop 兜底 detach 线程与句柄生命周期，补可观测回归。
- observed_head: 3edaa4305fcdb1ddd481fd51c6226471709cc1ba
- observed_worktree_hash: fnv1a64:18c5be4776ae1189
- recorded_at: 1787013682764

## D-525 D-506 多行 Mutex lock unwrap 漏网调用 [open] (medium)
- 复现: D-506 初轮巡检只匹配同一行 `.lock().unwrap()`；复核发现 `crates/kanzei-app/src/run/persistence.rs:215` 与 `state.rs:623/766` 采用换行 `.lock()` + `.unwrap()`，仍会在 poisoned mutex 上 panic。
- 影响: D-506 的热路径恢复策略存在漏网调用，持锁 panic 后仍可能触发级联命令僵死。
- 来源: self-found：D-506 提交前 staged diff 与多行锁调用复核。
- 标签: 后端
- 验收: 目标五个文件不再出现同一行或跨行 `.lock()` 后 `.unwrap()`；D-506 源码巡检与 kanzei-app 定向测试通过。
- refs: D-506
- 优先级: P1
