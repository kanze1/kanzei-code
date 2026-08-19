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

## D-552 桌面 UIA 停止 E2 未能定位生产发送按钮 [fixing] (medium)
- refs: R-101
- 复现: 运行 `pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\ui-desktop-uia.ps1 -RunStopTest`，B2 视图切换与 prompt ValuePattern 通过，但 Wait-KzButtonReady @('发送','Send') 超时并以非零退出。
- 影响: 真实停止 E2 无法触发生产 run_prompt，不能验证 `#stop → stop_run → kz:stopped` 链路；默认 B2 不受影响。
- 来源: self-found：R-101 B3 首次真实停止 E2。
- 标签: 流程
- 进展: 已复核当前环境：`Get-Process -Name kzapp` 仍发现真实安装位 `C:\Users\kanzei\AppData\Local\kanzei\kzapp.exe`，PID 50360、MainWindowHandle=1180626、HasExited=false。D-552 的代码修复已在 `scripts/ui-desktop-uia.ps1:106-131,213-216`，默认 UIA 回归由 T-1786922726472 覆盖；本轮无法安全执行 `-RunStopTest`，没有新增真实停止链路证据。
- 优先级: P2
- observed_head: d1cc00060b8e2540bd1c0309faa5d62d0efcfa26
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1787160690005
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-552 [tracker integrity degraded] D-555: invalid defect lifecycle [done]
- 阻塞: 真实 `-RunStopTest` 会向当前安装位 kzapp 发送测试 prompt 并改变用户会话；PID 50360 仍为用户进程，agent 不得强行接管或停止。解除人：用户关闭当前 kzapp 窗口，或提供可控的独立 kzapp 窗口后，由 agent 执行 `pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\ui-desktop-uia.ps1 -RunStopTest` 并核销真实 send→stop→stop_run→kz:stopped。

## D-555 metrics 回涨闸对零改动的 phase_pipeline.rs 误报涨 127 行,基线口径漂移 [done] (medium)
- refs: R-300
- 复现: 发布树 ff 至 3c123bd5 后跑 scripts/verify.ps1,报 metrics regression gate failed: crates/kanzei-app/src/phase_pipeline.rs production lines grew 127 (baseline 796, current 923, allowance 100)。该文件在 build-123d0952..3c123bd5 区间零改动(最后触碰 ec6f6970);docs/design/metrics_baseline.md:31 基线行记 总 933/生产 796/测试 137,新口径量出生产 923——差值 127 与测试行数量级吻合,疑 R-300 B5(f28c8dc2「修复 metrics 生命周期口径并更新基线」)改口径后基线全表未按新口径重生成,该文件测试行被计入生产。
- 影响: dev 自锁:文件没动却过不了自家闸门,发版链被卡;回涨闸在口径漂移下的数字不可信,真回涨与假回涨无法区分。
- 来源: 2026-08-20 发版预检,verify 实测(发布树,commit 3c123bd5)。
- 标签: 流程
- 验收: 修口径(测试行识别)或按新口径重生成基线全表并逐文件说明差异;phase_pipeline.rs 零改动时 verify 绿;补「基线与量测同口径」守护(基线生成器与闸门共用同一计数实现),防止再漂。
- 优先级: P1
- 状态: done
- 进展: 2026-08-20 根因实锤:闸门原用安装版 ~/.cargo/bin/kz.exe(旧口径,phase_pipeline.rs 量出生产 923/测试 10),基线由源码构建 kz 生成(新口径 796/137)——同文件两把尺。修复提交 0212db2b:metrics-regression-gate.ps1 改为 cargo build 当前工作树的 kz(target/debug/kz.exe)并在 $Root 下量测,闸门与基线生成器从此共用同一计数实现,「同口径守护」由此结构性成立。主树实测 30 行全过、巨石 5/5;发布树 verify 全绿,证据 dist/verification.json 绑定 55caf824。
