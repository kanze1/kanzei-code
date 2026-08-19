# Defects

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
- 停车: 排队(defect-first 序):待 D-486 收口后恢复,执行重启安装位回读持久化 auto state 的真实重启验收;恢复人:agent

## D-563 package.ps1 发布进度总数与实际步骤不一致 [open] (low)
- 复现: 在发布树执行 `.scripts\package.ps1 -Ack 14 -Publish -VerificationPath <verification.json>`，输出步骤会出现 `[9/8]` 和 `[10/8]`。
- 影响: 发布功能本身仍可完成，但活动面板/终端进度对用户显示错误总数，无法准确表达发布阶段和完成比例。
- 来源: self-found：本次 build-85d7123d 云端发布实测。
- 标签: 发布
- 进展: 已登记，暂不回改已发布构建；根因候选为 `scripts/package.ps1:15` 将 Publish 总数设为 8，但实际还调用了自动安装验证和 GitHub Release 两个步骤。
- 验收: `scripts/package.ps1` 的 stepTotal 与实际 Step 调用数一致；发布输出不再出现当前总数之外的步骤编号；非 Publish 与 Publish 两条通道都需覆盖。
- 优先级: P2
