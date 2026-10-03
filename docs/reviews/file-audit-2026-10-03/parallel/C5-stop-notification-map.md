# C5 补包：停止通知依赖图

起点为 C5 原 leaf `19d95cfa47519262f8519f0c63c7d55068c8c238`，分支 `kanzei/audit-c5-stop-notification`。仅重开 `persistence.rs` 必要切片；不增加全文审查数，也不改原 C5 验证记录。

## 状态与调用方向

```text
本轮 Runner + halt token
    ↓
runtime.lifecycle：Stop / 收尾提交串行
    ↓
TypedSessionWriter.finish_with_input_outcome
    ├─ typed terminal：Stopped
    ├─ input：cancelled
    ├─ session：idle
    └─ 既有 run.completed：halted_by_user=true
    ↓ commit 成功并释放 lifecycle
persist_round_outcome_with_effects
    ├─ append_run_notification → SessionStore.append_notification_atomic
    │      → agent_notifications → mobile/sse replay → PWA formatNotice
    ├─ notify_mobile(title, body)：外部手机提醒
    └─ after_success：memory inbox consolidation / candidate reconciliation
```

typed terminal/input/session/result 由 core 原子事务拥有；通知是提交后的派生展示。停止事实不能投影成 `succeeded`，也不能触发 `after_success`。已有部分轨迹 harvest 和 `episode=halted` 保留。

## 真实触发链

1. 本地 provider 返回合法 200 SSE，实际 Runner 完成 assistant，结果最初为 `Ok`。
2. `stop_runtime_and_finalize` 在 persistence 之前抢到 lifecycle，取消同一 token 与 running input。
3. persistence 在 lifecycle 内重核 token，正确提交 Stopped，输入保持 cancelled。
4. 原 C5 通知切片仍无条件记 `succeeded/任务完成`、发送手机“任务完成”并执行 `after_success`，持久事实与外部结果分叉。

合作停止同样可达；不是新增产品语义。此次补包属于 C5 已记录原子 outcome / Stop 收尾根因的遗漏，不重复计一个新根因。

## 写前 caller / consumer 核对

| 文件与入口 | 已有合同 / 本包影响 |
| --- | --- |
| `run/coordinator.rs` → `persist_round_outcome` | 唯一生产收尾 caller；原 Stopped 收据和错误分级保持。 |
| `run/persistence.rs` `OutcomeEffects.after_success` | 生产 closure 合并 memory inbox 并 reconcile candidates；仅正常成功调用。 |
| `run/mod.rs` `append_run_notification` | 薄传递 status/summary，未改 API。 |
| `core/notification.rs` `AgentNotification.status` | 已有 String，无 schema/enum 迁移。 |
| `core/store/notifications.rs` `append_notification_atomic` / `replay_notifications` | 现有状态字符串原样提交和回放；不改序号、事件格式。 |
| `app/mobile/sse.rs` | 原样序列化通知，停止状态无需服务适配。 |
| `app/mobile-pwa/app.js` `NOTICE_GLYPHS` / `formatNotice` | 已支持 `stopped: ■`，正常成功为 `succeeded: ✓`。 |
| `app/ui/07-events.js` `kz:done` | 已按 halted 映射 `stopped`；本包不改 UI。 |
| `app/mobile_notify.rs` `notify_mobile` | 已接受 title/body；不改同步外部调用和生命周期锁边界。 |

`docs/design/r059_mobile_agent_communication.md` 已定义通知用于表达运行结果的既有职责。当前 PWA/UI 支持 Stopped；input 的 cancelled 与运行 stopped 是不同层的已有表示。

搜索 `run.transaction_budget_result` 的实际生产/设计 caller 仅找到 persistence producer 与其单测，没有消费它驱动业务的路径；本包不修改其 `completed` 字段，也不把它单独列为 bug。

## 修复与回归边界

- 改动仅通知/手机文本/成功后处理的发布条件；Stopped 发布 `stopped/任务已停止`，手机明确停止，跳过 after_success。
- 复用真实 SQLite + 本地 fake HTTP + 实际 Runner + 实际持久函数 fixture；扩展已存在的 200→Stop 与合作停止断言。
- 正常 200 与完成先赢继续 `succeeded` / 手机任务完成 / after_success 一次。
- 负对照只恢复原 leaf 的通知/手机/after_success 发布切片，保留新断言，要求真实 Stop fixture 断言失败；随后逐字恢复源码。
- 证据单独存 `output/C5-stop-notification`。独立槽内初次及精确恢复后的 persistence 定向各18项通过；原通知块负例编译成功，两个真实 Stopped 状态断言失败，正常200和完成先赢等8项控制通过。app all-targets check、Clippy -D warnings、fmt、diff均通过。
- 本补包未重复全workspace/core/app全包/native验证；所有结果来自本树实际源码，不借用原C5或root验证。源码恢复逐字一致并核对SHA，再刷新本树Rust mtime重编译。
