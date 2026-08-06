# Defects

## D-010 桌面端重启后历史对话不可恢复 [open] (high)
- 原因: run_task 只将消息保存在 AppState.conversation 内存中；SQLite 目前仅写入 prompt/run 边界事件，没有保存消息内容，也没有历史会话列表/加载 command。
- 复现: 在 kzapp 中发送对话并关闭应用，再次打开同一项目；消息区域为空，无法查看或继续之前会话。
- 影响: 违反 R-009/R-013 的重启恢复和历史会话加载验收。
- 验收: 重启后可看到项目会话列表，打开任意会话可恢复消息并继续对话；存储读取失败需明确提示。

## D-023 多个 pending steer 被一次性提升但仅消费第一条，导致后续 steer 丢失 [fixed] (high)
- 修复计划: 将 steer 提升改为每次仅提升 FIFO 第一条，并补充连续 steer 与 queue 的回归测试。
- 复现: 同一会话 admission 两条 steer 后连续调用 promote_next_input；第一条返回，第二条已标记 promoted 但不再返回。
- 影响: 运行中 steer 调度会丢失用户输入，破坏 R-003 的 steer 优先与输入可靠性。
- 验证: SessionStore 新增 promote_next_steer，每次仅提升一条 steer；新增 drain_依次提升全部_steer_再取_queue 回归测试。cargo test -p kanzei-core 与 cargo test --workspace 全部通过。
