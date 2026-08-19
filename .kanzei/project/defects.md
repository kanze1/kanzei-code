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
- 停车: 
- 对账: 2026-08-20 对账:停车条件(R-242 建立真实 shadow 验证窗口)已满足——T-1786922726248 共 30 真实 turn、unknown=0、typed_write_errors=0,停车解除;剩余动作=在含 compaction 的真实会话再跑一次 kz shadow --mismatches,确认压缩后 legacy surface 计入 expected(compacted_snapshot)而非 unknown 后关闭

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
- 阻塞: 
- 对账: 2026-08-20 对账:用户已关闭 kzapp 窗口,阻塞解除;剩余动作=重新启动安装位 kzapp 回读持久化 auto state 完成真实重启验收(桌面窗口空闲期执行,CLI 循环亦可承接);其余验收已通过(8f490d92)

## D-552 桌面 UIA 停止 E2 未能定位生产发送按钮 [fixed] (medium)
- refs: R-101
- 复现: 运行 `pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\ui-desktop-uia.ps1 -RunStopTest`，B2 视图切换与 prompt ValuePattern 通过，但 Wait-KzButtonReady @('发送','Send') 超时并以非零退出。
- 影响: 真实停止 E2 无法触发生产 run_prompt，不能验证 `#stop → stop_run → kz:stopped` 链路；默认 B2 不受影响。
- 来源: self-found：R-101 B3 首次真实停止 E2。
- 标签: 流程
- 进展: 2026-08-20 真实停止 E2 通过:用户关闭 kzapp 窗口后,agent 修复 D-564 冷启动轮询并执行 pwsh -File .\scripts\ui-desktop-uia.ps1 -RunStopTest——stop_test_requested=true、stop_requested=true、stop_settled=true,发送/停止按钮均按生产 AutomationId 定位成功,process_owned_by_test=true,截图 464972 bytes。此前 Wait-KzButtonReady 超时的根因已由 d1cc0006(AutomationId 优先+名称回退+逐轮重取节点)修复,本轮为其真实链路核销
- 优先级: P2
- observed_head: a8e75106b629441cc19963dd5667aee07a74339a
- observed_worktree_hash: fnv1a64:00ea97ae7b316f67
- recorded_at: 1787168102269
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-552 [tracker integrity degraded] D-555: invalid defect lifecycle [done]
- 阻塞: 

## D-563 package.ps1 发布进度总数与实际步骤不一致 [open] (low)
- 复现: 在发布树执行 `.scripts\package.ps1 -Ack 14 -Publish -VerificationPath <verification.json>`，输出步骤会出现 `[9/8]` 和 `[10/8]`。
- 影响: 发布功能本身仍可完成，但活动面板/终端进度对用户显示错误总数，无法准确表达发布阶段和完成比例。
- 来源: self-found：本次 build-85d7123d 云端发布实测。
- 标签: 发布
- 进展: 已登记，暂不回改已发布构建；根因候选为 `scripts/package.ps1:15` 将 Publish 总数设为 8，但实际还调用了自动安装验证和 GitHub Release 两个步骤。
- 验收: `scripts/package.ps1` 的 stepTotal 与实际 Step 调用数一致；发布输出不再出现当前总数之外的步骤编号；非 Publish 与 Publish 两条通道都需覆盖。
- 优先级: P2

## D-564 ui-desktop-uia 冷启动一次性查找 prompt 控件,脚本自拉起 kzapp 必失败 [fixed] (medium)
- refs: R-101 D-552
- 复杂度: 小
- 复现: 关闭 kzapp 后执行 pwsh -File .\scripts\ui-desktop-uia.ps1 -RunStopTest:脚本 Start-Process 自拉起应用(process_owned_by_test=true 路径首次真实执行),窗口句柄出现后仅 Start-Sleep 500ms 即一次性 Find-KzPrompt(ui-desktop-uia.ps1:151-156),WebView2 内容冷启动渲染晚于顶层句柄就绪,报「UIA 未找到生产 prompt 编辑控件」退出 1。历史全部通过记录均为附着已运行进程(process_owned_by_test=false),冷启动路径从未被验证
- 影响: R-101/D-552 的解除动作「用户关闭 kzapp 后由 agent 执行 -RunStopTest」在真实窗口期不可执行;停止 E2 与后续 B4 被脚本自身缺陷卡住
- 标签: 流程
- 验收: prompt 查找带截止时间轮询(复用 TimeoutSeconds),冷启动自拉起路径真实跑通 -RunStopTest 或至少通过默认 B2;附真实运行证据
- 优先级: P2
- 进展: 修复:scripts/ui-desktop-uia.ps1:153-163 prompt 查找改为复用 TimeoutSeconds 的截止时间轮询(250ms 间隔),冷启动注释点名 D-564。验证:关闭 kzapp 后真实执行 -RunStopTest 全链路通过——process_owned_by_test=true(冷启动自拉起路径首次真实跑通)、input_marker_round_trip=true、prompt_retained_after_view_switch=true、stop_requested=true、stop_settled=true,截图 .kanzei/research/r302-desktop-e2/kzapp-uia.png(464972 bytes);脚本结束自行收尾自有进程,无残留
- observed_head: a8e75106b629441cc19963dd5667aee07a74339a
- observed_worktree_hash: fnv1a64:00ea97ae7b316f67
- recorded_at: 1787168091762
