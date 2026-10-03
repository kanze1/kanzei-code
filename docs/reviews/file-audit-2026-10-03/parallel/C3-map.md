# C3：提交确认与工具执行边界

首次只读基线：C2 b9f3ae24（统一源码基线 5b2686e0）。正式实施分支 kanzei/audit-c3 从 f0dd9d57 建立，再 cherry-pick A2 95b7b99d 为 abd7b130；保留 A2 source/CAS 合同。全文审查 drive.rs / drive/history.rs / drive/serial_tools.rs 原始 1603 / 164 / 240 行，含测试/失败路径。8 个授权源码改动已完成，验证基线为 abd7b130（f0dd9d57 + A2）。runner4/core408/CLI events2/app events19 全通过，4包 all-targets check/Clippy/fmt/diff 通过；负对照 2pass2fail 证明实际工具/请求窗口，逐字恢复 hash+mtime 后正确代码再编译。root 后续 M2/D1 集成另验。

## 修复前依赖与状态 owner

```text
SessionStore / typed checked batch（C2：SQLite 事实权威）
  ↓
TypedSessionWriter assistant/tool commit
  ↓
app TypedEventSink / CLI event handler / scheduler event handler
  ↕ RunEvent 同步回调（当前无失败回传）
history commit_* → runtime messages / round_messages receipt
  ↓
drive StepMessageOutcome::Proceed
  ↓
run_subagent_calls（含后台 spawn） → execute_tool_calls（serial/parallel）
  ↓
tool result commit → finalize_step → 下一 provider 请求
```

DB 事实由 store 拥有；writer.errors 只在 writer 本地；runtime messages 和 round_messages 由 drive/history 拥有。三个 durable sink 的 callback 目前为 void，不能证明 DB 接受了其消息。

## 完整调用方清单与实际写范围

| 文件 / API | 当前 caller 或用途 | 实际范围 |
| --- | --- | --- |
| drive/history.rs commit_assistant_message | commit_step_messages | 全文；确认持久成功后才 push |
| drive/history.rs commit_tool_results | commit_step_messages 停止出口、drive finalize_step、serial_tools 停止/拒绝出口 | 全文；失败传播所有出口 |
| drive/history.rs record_round_message | drive 的外层 event 包装；本域与 drive 测试 | 只在同步 sink 返回且确认成功后收集 receipt |
| drive.rs commit_step_messages / finalize_step | run_once_with_parts | 全文；Result 传播到 runner Err，不伪装用户停止 |
| drive/serial_tools.rs | execute_tool_calls 串行分支；两处提前提交 tool results | 全文；两处用 ? 传播失败 |
| store/typed.rs assistant_committed / tool_results_committed | app events/mod.rs:207/213；CLI run/events.rs:34/38；tools schedules/executor.rs:449/453；其它全部为测试 | 仅两个方法切片；返回实际 batch 成功 bool，早期 terminal/source-step 拒绝也 false |
| runner/event.rs 两个 MessageCommitted variant | history 生产 constructor；drive/history、drive、subagent、memory_chat 的读取；直接 fixture/test constructor | 只 receipt 和两 variant 切片，不改 work-context source |
| app run/events/mod.rs | TypedEventSink → event arms:728/819 | 同步返回实际 typed commit 结果 |
| CLI run/events.rs | make_event_handler | 同步回填 receipt |
| tools/schedules/executor.rs | Prompt step 的真实 typed sink | 同步回填 receipt；schedule session=project#run_id，不能混成 CLI 同会话交错证据 |
| runner/subagent.rs、app memory_chat.rs 及测试 constructor | 消费消息和轨迹，不拥有此主 session 的 typed writer | 必要 pattern 加 .. / constructor 补默认 receipt，不改变执行策略 |
| runner/mod.rs 的 event 类型 re-export | runner 内统一类型入口 | 既有 pub use event::* 已覆盖 receipt；无需修改此文件 |

正式改动前已再次搜索全部 public API、事件构造/匹配及直接 writer caller；改动后再次全仓 Rust 搜索核对，不依赖编译错误发现范围。

## 确认的 P1 可达链

1. 桌面 turn 已由真 TypedSessionWriter 写 user/start，仍在运行。
2. 同 session 的 CLI prepare 调用 recovery，真实独立连接追加 failed terminal。app 也在 writer lease 获取之前调用 prepare，租约尚不能排除此恢复入口。
3. 旧活跃 writer 的 assistant + tool_called batch 被 C2 terminal guard 整批拒绝。
4. writer 只记录 errors；三个 sink 均 void。history 仍把 assistant push 进 runtime，drive 仍 Proceed。
5. drive 先运行/派发子代理，再执行普通工具，产生无对应 durable ToolCalled 的副作用。
6. tool result 持久化失败同样不会阻止下一 provider 请求，runtime history 可继续使用未提交结果。

不因为工具可能写文件就升级 P0。设计依据 deepseek_harness_upgrade.md:122：tool_called 必须先于工具副作用落库。D-655 的 round receipt 也不能把失败消息计成已提交事实。

## 已实施的最小合同

- 不改 run_once / run_once_with_parts / FnMut(RunEvent) 公开签名。
- event owner 提供 MessageCommitReceipt（Arc<Mutex<Option<String>>>）。默认允许既有 stateless sink；真实 durable sink 同步提交并写失败，首个错误不能被后续成功覆盖。
- assistant/tool-results event 携带 commit receipt。writer 两方法返回那一批提交是否成功的 bool；sink 不按 errors 数量变化猜测结果。false 使用既有实际错误或明确拒绝原因。
- history 先发同步事件、查 receipt，再 publish messages；drive 先让 sink 回来，再记录 round receipt。
- private commit_step/finalize/tool-results 提交改 Result，所有提前收尾出口传播 Err；不发下一个 provider 请求、不派发任务、不执行工具，不把持久化失败标成用户取消。
- schema、数据格式、已有取消占位和 tool call/result 配对保持不变；恢复排他不混入本包。

## 真实夹具与验证安排

drive.rs 的 commit_ack_tests 已实现真 runner 夹具（ignored output/C3 保留原草稿作历史证据）：

- 临时 SQLite + 真 TypedWriter + 本地 fake HTTP/SSE + CountingTool（只 AtomicUsize，无外部副作用）。
- TurnStart 真写入后独立 recovery 闭合 turn，随后真 assistant sink 被拒；旧代码预计执行计数 1、DB ToolCalled 0，修复需 Err / 执行计数 0 / provider 请求 1。
- 临时 DB 持久 SQL trigger 拒绝 tool result；修复需真实 effect 仅 1 次、DB call 1 / result 0、下一请求不发生，不重执行已发生工具。
- 正常 durable 路径需 effect 1、请求 2、DB call/result 各 1，round receipt 与实际 messages 一致。
- 先收集事实和计数、清理临时目录，再断言；旧实现负例不遗留 fixture。
- receipt、writer bool、三个真实 sink、所有私有出口和 constructor 已适配；完成 rustfmt、静态 caller、真实正负对照、core全包和三个caller的检查。新增默认 stateless reader 实链正常路径。
- 负对照保留新 API 与新回归，只移除 history 两处 receipt 检查以等价恢复旧的“回调拒绝仍 push/Proceed”；实际两错误路径都多发请求，assistant拒绝仍执行工具。日志与 SHA256 恢复记录保存在 output/C3，恢复后正确代码重新编译全通过。

## 恢复 owner 的独立开放项

生产 prepare 入口：CLI run.rs:291；app run/assembly.rs:436；core rewind_plan:104（仅 facts 为空才调用）。scheduler 不使用 prepare，拥有独立 run session。

app prepare 在 acquire_plain_lease_if_needed:547 之前；CLI 的 promote/start_input 只约束本 input，不排除其它 running input；现有 coordinator 是 code-tree writer 范围，CLI 不参与桌面共享仲裁。持久 session.status=running 在 crash 后也可能残留，不能独自证明活跃或死亡。

此包只阻止已拒绝提交后继续执行。活跃恢复 owner 排他需要 root 分配独立实际入口范围；不增加新 persistence/lease 框架。
