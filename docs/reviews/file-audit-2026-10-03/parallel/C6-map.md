# C6 依赖与状态地图

开始基线 `28265223acae1788bd550d6621bef636765e9f87`，独立分支 `kanzei/audit-c6-startup-outcomes`，复用 audit-c0 树。当前已完成本树实际Rust/native正反验证，本地图随精确leaf交付。全文对象只有 `app/run/assembly.rs`；baseline/components/owner/state 等按真实依赖读取，必要重开切片不增加全文覆盖。没有修改 schema、持久字段、目录布局、锁原语、全局 coverage；没有读取真实 state.db/凭据，没有 push/release。

## 依赖顺序与状态 owner

`base FileLock / SQLite → SessionStore events/inbox/session → TypedSessionWriter admission/invariant/outcome → app assembly → coordinator execution/persistence → commands scheduler/UI`。

- SQLite、input/status/events 是持久事实 owner；C5 `finish_with_input_outcome` 是 typed terminal/input/session/result 的唯一同事务收尾边界。
- `SessionExecutionGuard` 是同 DB/session 执行 owner；writer/callback 的 Arc 保证真实收尾以前不能恢复其它活跃入口。C6 不另建租约或持久状态。
- `SessionRuntime.lifecycle` 串行实际 Stop 与结果提交；本轮独立 halt token 决定 Stopped，不依赖可能已换代的槽。
- `MemoryCoordinator` 只负责项目/代码树写租约；它的取消排队不是用户事实的持久提交凭据。
- `conversation` 是 durable projection 的缓存，通知/trace/flush 是提交后的副作用，不能证明原子 outcome 成功。

## 写前 public/跨域 caller 图

| 合同 | 真实生产 caller | C6 处理 |
| --- | --- | --- |
| `assemble_run` / `RunAssembly` | `app/run/coordinator::run_task` | 签名与三分组产物保持；Window wrapper 调具体私有 `prepare_session`。 |
| `RoundRequest` / `RunMode` / `RuntimeHandles` | `app/commands/run` 唯一构造；coordinator 消费 | RuntimeHandles 传同一 lifecycle Arc，没有第二份状态锁。 |
| `TypedSessionWriter.user_message` | app assembly、CLI run、schedules executor；其余是测试 | 返回真实 bool ACK，三 caller 都在 provider 之前检查。没有用 sticky errors 长度猜 ACK。 |
| `finish_with_input_outcome` | C5 persistence；C6 app startup 与 CLI admission | 原 API/schema 不改；本次实际 input/session 的 Failed/Stopped 原子结果。 |
| `finish_input` | commands fallback、CLI、coordinator/input 管理 | 已建立 writer 后不单写；无本轮 writer 前只处理实际当前 input。未提交 marker 继续保护原 rollback。 |
| `baseline::prepare` | app assembly 唯一生产 caller | 成功持久 baseline 才有当前 run_id；失败不编造来源、不关闭历史 turn。 |
| `owner.prepare` / `owner.writer` | app assembly、CLI；既有其它执行入口 | canonical DB/session 身份原合同保持；writer Arc 覆盖至收尾、晚 flush Drop。 |
| `acquire_plain_lease_if_needed` | assembly；实际 coordinator.cancel_waiter | 等待不持 lifecycle；真实取消后在当前 outcome 边界收尾。 |
| `WriterLeaseTrace` | assembly/trace 测试；persistence mark_released | 原 RAII 与审计 release 合同保持。 |
| scheduler `run_steps` | public schedules::execute → CLI schedule run | 第一 Prompt=user；后续 Prompt=唯一 input_id 的 steering；source step 重新从1开始由 writer 转单调 logical step。 |

其余 assembly 跨文件 API：`build_run_harness` 被 agent_directory/model_config 与测试调用；`resolve_profile` 被 model_config 调用；`resolve_proxy` 被 manual_compact 调用；模式判定被本文件和 phase_pipeline_tests 调用。签名与行为保持。完整搜索原文在 `output/C6/user-callers.log`、`startup-callers.log`；不靠编译器发现 caller。

既有 app coordinator/CLI events 的 steering 消费为后包读取范围，C6 不仅因忽略 bool 宣称新 bug。根代理已确认 M6 为 DocStore，不覆盖此 ACK；后包需要实际拒绝/合法 retry 与 provider/input owner 实链证明。

## 明确可达启动失败链

1. 新 Prompt 的 commands `initial_input=None`，assembly 内部 admit/promote/start。
2. 当前 typed user 成功、session running 后，running notification 或 status event SQL 拒绝使 assembly Err。
3. 旧 commands 捕获的 promoted_id=None，所以实际内部 input 仍 running；Saved/队列路径则只单写 input failed，typed 仍 open/session running。
4. UI runtime idle 不能替代持久状态静止；实际 storage cleanup/delete 会因 active input/running session 拒绝。

最小收口：私有 prepare_session 掌握实际 store/promoted input/当前 writer，fallible tail 用一个 Result 范围；writer 建立后 Err 在短 lifecycle 内调用 C5 Failed/Stopped 事务，成功后才写弱通知。事务拒绝返回具体 uncommitted marker，commands 不孤立改单个 input。建立 writer 前的附件/baseline Err 只收尾当前 input，不恢复或关闭旧历史 turn，不假称失败 baseline 成功。

## 用户事实准入与多 Prompt 合同

原 user_message 返回 `()`，SQLite 拒绝仅记录 errors；三个生产 caller 会继续 provider。C6 返回本次 durable append 的 bool，拒绝后提交现有 Failed 结果并 Err；如果 Failed 也拒绝，不伪装 typed Stopped。app 对应完整 input/session 原子结果，CLI 输入已实际提升/running 且持 owner，scheduler 的 typed 内部输入没有 session_inputs，复用其既有 finish(Failed)/失败 receipt。

同一 schedule 多 Prompt 共用一个 run writer，原每次重复 UserMessageCommitted 违反“一 turn 一 user”不变量。后续 Prompt 应是唯一 input_id 的 SteeringMessageCommitted；其 ACK 同样要检查。writer.turn_started 对每次 runner 局部 step1 分配 logical1、logical2，不能修改底层 invariant 来容忍重复初始 user。必须用真实2POST、两 assistant facts steps1/2 和 Completed 控制验证。

## 并发与失败边界

- lifecycle 仅在 startup Err 收尾时短持，里面 token 再核与原子 outcome；不跨 baseline/lease await，不覆盖通知/外部请求。
- Stop先赢：真实 finalize_interrupt 将当前 input cancelled，C5 Stopped 保留 cancelled/session idle；错误 source 保留并附既有停止 marker。
- 失败事务拒绝：typed/input/session/result 和 writer 内存不推进，具体 marker 保护 commands fallback，不把 open/running 说成持久 failed 成功。
- 成功 startup 后才 spawn 原弱250ms flush；失败没有遗留 weak任务。writer/callback Arc 与 execution owner 原释放边界不变；FileLock worker 不持 SQLite、typed/runtime mutex，不在持 lifecycle 时 join 回调 worker。
- baseline 的 Git失败/截断原本正确记 unknown/partial，捕获代码不改；只修它调用方的 input Err出口。

## 夹具与实际验证

- app 7个真实 prepare_session/临时SQLite/Counting HTTP测试：内部/已保存 input、running状态拒绝、user拒绝、Failed事务各写点拒绝及实际commands fallback、writer前baseline/附件错误、真实 MemoryCoordinator排队+Stop/cancel_waiter、正常200控制。
- core 1个真实SQL user ACK：拒绝false、无半事实、删除trigger后同一writer精确retry true，旧diagnostic error保留不影响ACK；terminal后callback false。
- 新 `scripts/startup-admission-smoke.py` 通过实际CLI正常200先建真实schema，然后安装临时 user/steering SQL trigger，检查0新增POST、失败receipt、输入/typed状态；正常CLI与2Prompt/2POST/steps1,2合法完成控制。脚本是新回归fixture，不计业务全文审查。
- 等价旧 startup Err边界、等价旧无ACK/重复User生产控制保持新夹具；构建成功后的真实断言失败才证明回归。原字节备份SHA、精确restore+mtime与恢复后的正确验证必须齐全。

## 保留独立探针范围

FIFO 附件候选：已有 pending A 时新B附件仍从 request(B)取，实际 promoted prompt=A，随后B的 next_attachments 已 take。当前只准备独立实际请求探针；没有改附件/持久格式，没有把静态路径当实测完成。需提供首请求/后续payload证据与不丢载荷最小方案后根代理再定源码边界。


## 验证收口

八条新Rust回归、七个actual native场景均通过。旧startup六真实断言失败/正常控制通过，旧准入六native断言失败，旧Drop-only捕捉Stopped误写；每组精确finally恢复。四相关完整包1890通过/0失败/3原有忽略，最终check/Clippy/fmt/diff全0。两个等价lint修正后重新startup7及稳定source build/native，源→build→binary绑定见 `output/C6/final-stable-binary-manifest.json`。C7/M7地图未计coverage、不进C6 leaf；完整记录见C6.md/C6-verification.json与raw index。
