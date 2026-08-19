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

## D-552 桌面 UIA 停止 E2 未能定位生产发送按钮 [open] (medium)
- refs: R-101
- 复现: 运行 `pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\ui-desktop-uia.ps1 -RunStopTest`，B2 视图切换与 prompt ValuePattern 通过，但 Wait-KzButtonReady @('发送','Send') 超时并以非零退出。
- 影响: 真实停止 E2 无法触发生产 run_prompt，不能验证 `#stop → stop_run → kz:stopped` 链路；默认 B2 不受影响。
- 来源: self-found：R-101 B3 首次真实停止 E2。
- 标签: 流程
- 进展: 待复核真实 UIA Button 的 Name、AutomationId、IsOffscreen、ControlType，改用实际生产属性定位；不得用静态 DOM 或仅按钮存在作为替代证据。
- 优先级: P2

## D-553 kz:done 耗时用未初始化的本页 runStart 计算,打出纪元级秒数 [open] (small)
- refs: R-101
- 复现: 2026-08-20 00:11 R-101 停止链路实测(marker R101_UIA_STOP_20260819161104335):手动停止并取消 2 条排队输入后,运行日志打出「运行完成: 6 轮, 耗时 1787155867.5s」——该值恰等于当时的 Date.now()/1000,即 runStart=0。根因: `crates/kanzei-app/ui/07-events.js:423` 的 kz:done 处理器用模块级 `runStart`(`03-shell.js:433` 初值 0)算耗时,而它只在 `08-compose.js:314` sendPrompt 路径经 startElapsed() 赋值;本页实例未经 sendPrompt 启动该轮时(页面/webview 重载后接管在跑会话、后端排队派发或鞭挞续跑的轮次)必现。后端 `kz:done` 载荷(`crates/kanzei-app/src/run/persistence.rs:488-503`)不带时长字段,前端无可信来源可退。
- 影响: 运行日志耗时失真为 17.9 亿秒;长会话/停止链路 E2 无法以日志耗时作观测证据。仅显示层,不影响运行本身。
- 来源: 用户截图(2026-08-20,R-101 停止链路 E2 现场);代码对照 self-confirmed。
- 标签: 前端
- 验收: kz:done 的耗时来源可信——后端载荷携带 elapsedMs(推荐,后端知道真实起点)或前端在 runStart=0 时退化为只报轮数不打绝对时长;补「页面重载后接管在跑会话」场景回归;运行日志不再出现纪元级耗时。
- 优先级: P3

## D-554 ps1_bom 门禁红:ui-desktop-uia.ps1 无 BOM 入库,提交侧闸门漏拦 [open] (small)
- refs: R-101 D-408
- 复现: 发布树 ff 至 3c123bd5 后跑 scripts/verify.ps1,ps1_bom 步骤失败:scripts/ui-desktop-uia.ps1 含 374 个中文字符缺 UTF-8 BOM。该文件由 cd4b6013(R-101 B2)新增入库,提交侧结构化 git 闸门未拦——疑因门禁跑在安装版 kzapp(123d0952 之前构建)上,不含 R-300 B2 0abdef53 修复后的 BOM/扩展路径检查(verify 侧与提交侧清单未真正对齐)。
- 影响: dev 过不了 verify,发版链被卡(verify 不产出 verification.json,package 无从执行);该脚本在 Windows PowerShell 5.1 下会解析报错。
- 注意: 主树该文件当前有 R-101 B3 未提交 WIP(-RunStopTest/Find-KzAutomationId 定位,修 D-552);BOM 修复应并入该线下次提交,不要在发布现场单独动这个文件,避免同文件两线冲突。
- 来源: 2026-08-20 发版预检,verify 实测(发布树,commit 3c123bd5)。
- 标签: 流程
- 验收: 脚本重存 UTF-8 with BOM 后 verify ps1_bom 步骤绿;核对提交侧闸门为何漏拦新增 .ps1(gate_checklists_align 守护是否覆盖),给出拦截或豁免结论。
- 优先级: P1
- 状态: open

## D-555 metrics 回涨闸对零改动的 phase_pipeline.rs 误报涨 127 行,基线口径漂移 [open] (medium)
- refs: R-300
- 复现: 发布树 ff 至 3c123bd5 后跑 scripts/verify.ps1,报 metrics regression gate failed: crates/kanzei-app/src/phase_pipeline.rs production lines grew 127 (baseline 796, current 923, allowance 100)。该文件在 build-123d0952..3c123bd5 区间零改动(最后触碰 ec6f6970);docs/design/metrics_baseline.md:31 基线行记 总 933/生产 796/测试 137,新口径量出生产 923——差值 127 与测试行数量级吻合,疑 R-300 B5(f28c8dc2「修复 metrics 生命周期口径并更新基线」)改口径后基线全表未按新口径重生成,该文件测试行被计入生产。
- 影响: dev 自锁:文件没动却过不了自家闸门,发版链被卡;回涨闸在口径漂移下的数字不可信,真回涨与假回涨无法区分。
- 来源: 2026-08-20 发版预检,verify 实测(发布树,commit 3c123bd5)。
- 标签: 流程
- 验收: 修口径(测试行识别)或按新口径重生成基线全表并逐文件说明差异;phase_pipeline.rs 零改动时 verify 绿;补「基线与量测同口径」守护(基线生成器与闸门共用同一计数实现),防止再漂。
- 优先级: P1
- 状态: open
