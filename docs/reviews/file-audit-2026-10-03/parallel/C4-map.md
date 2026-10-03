# C4：同数据库/会话执行 owner

实施基线 0e5355c6；分支 kanzei/audit-c4-session-owner。先全文读取新专用 primitive 和真实进程 fixture；typed/inbox/CLI/app 已审文件仅重开必要切片，不重复统计全文覆盖。主树 M3 未提交源码没有复制，本包证据来自自己的 CLI binary。

## 依赖和状态 owner

FileLock（系统文件锁、线程亲和）→ canonical DB/session 执行 guard → typed Immediate 恢复 + inbox 原子 orphan claim 收尾 → CLI/app 输入 promotion/start → owned TypedSessionWriter → C3 commit receipt/runner → typed terminal + input 收尾 → 最后 Arc Drop 释放并 join worker。

- DB session_events/session_inputs 是持久事实源；runtime.running/lifecycle 仅保护同 AppState。
- 原 recovery 的 Immediate 保证检查/写入原子，但不能证明另一进程已死亡。
- 新 guard 不持 SQLite 事务跨 await，也不替代队列或 code-tree lease。
- key：canonical 完整 DB 路径拼接 .session-owners，子文件名为 session_id UTF8 的 SHA256；底层 FileLock 使用 .lock。新目录是锁运行态，不改 schema/持久事件格式。GC/备份只扫其原候选，不会删锁目录。

## 写前完整 caller 清单

| API / 状态边界 | 真实 caller | 本包处理 |
| --- | --- | --- |
| prepare_typed_session | CLI run、app assembly、rewind_plan facts 为空时；其余测试 | 执行入口用长期 guard.prepare；公开兼容入口短锁，活跃 owner 时明确失败 |
| recover_interrupted_session_facts | prepare；直接其它均回归夹具 | 短锁防活跃恢复，私有 owned helper 在同 Immediate 原子闭合 typed 和 orphan 输入 |
| seed_latest_legacy_snapshot | prepare；inbox mobile receipt；work-context/fixture | 保留 C2 source/floor 原合同；owner prepare 在执行前 seed |
| admit/promote_next_queue/start_input | CLI run；app commands Saved/wake/drain → assembly | idle 执行入口先 acquire；已有本地 running 的 queue/steer 仍沿既有分支返回 |
| PromotedInput/AdmittedInput 五字段 | input_id、session_id、prompt、delivery、admitted_at | CLI 使用实际队首 prompt；不把新 argv 的 autonomous 策略套给旧 input |
| input_work_item/input_is_execution_batch | 桌面工作执行/批次；CLI 可能提升它们 | CLI 无对应 assembly，明确失败旧 claim，保留本次 argv pending，不假执行 |
| TypedSessionWriter::new | CLI/app、scheduler 独立 run session、测试 | CLI/app 改 guard.writer；其它原 new API 保留 |
| writer user/start/assistant/tool/finish/Drop | CLI event/flush/finalize；app event/flush/finalizer | writer Arc 持 owner 至 terminal/晚回调；C3 bool/receipt 合同不变 |
| RoundRequest | 唯一生产 constructor commands/run | 增加 owner Arc；assembly 校验 canonical DB/session 和当前 input |
| finalize/finish_input | CLI finish_run/Ctrl-C；app assembly失败和 finish_round | CLI Err 原来漏 failed 收尾，成功不再吞 finish_input 错误；app 原收尾仍在 outer owner 生命周期内 |

写前全仓 rg 搜索上述公开 API、构造、prepare/promote/start/finalize/Drop；没有靠编译错误发现 caller。手机 M2 当前只提交 receipt/user fact，不启动 runner；scheduler session 包含独立 run ID；team 子执行归 D2 独立包。

## 确认的三个 P1 根因

1. 活跃恢复：A 真 CLI 的 provider POST 仍 held，B 同 DB/session 可在 owner 之前 --new/reset/prepare，并给 A 追加 TurnFailed；两入口可同时运行。现 owner 先于 --new/admit/recovery，busy 不修改 DB。OS 在实际进程死亡后自动释放；下一 owner 一次恢复 draft/tool/terminal 和旧 promoted/running 输入。
2. FIFO 身份：旧队首 A pending，新 CLI argv B 入队后 promote 得 A，但原代码把 B user/provider/episode 挂 A input ID。改用 promoted.prompt，B 仍 pending；桌面 work/batch 载荷 CLI 明确拒绝并正确失败 A。
3. CLI 收尾失败语义：provider Err 写 typed/session failure，run_result? 在 finish_input 前返回，输入仍 running；typed finish 拒绝后，CLI 也可继续 input completed/Ok。失败分支先 durable failed；私有 finish_typed_checked 复用 is_terminal 和真实错误，terminal 拒绝正确失败 input/session；Ctrl-C 先 finalize_interrupt 再回传 terminal 错误，成功输入收尾错误也 Err。

## 锁序、Arc 和失败路径

- app runtime.lifecycle → worker try OS lock（零等待）→短 SQLite transaction。CLI nested kz run 同会话立即 busy，不阻塞父工具产生死锁。
- FileLock 始终在独立标准线程 acquire/drop，不跨异步线程迁移；guard 是 Send+Sync。
- worker 只等 release channel，既不持 guard Arc，也不调用 SQLite/runtime/callback。Drop 先关闭 channel 再 join，join 前无需任何 worker 会申请的锁；下一入口不会遇到异步释放的假 busy。
- CLI lexical owner 覆盖整个 run/finalize；event handler、flush、finish 的 writer Arc 同样保留 owner。Ctrl-C terminal/input/flush 收尾后最后释放。
- app commands outer queue loop 保留 owner，RoundRequest/writer 持 clone；assembly Err 仍在 outer owner 内失败输入；late typed callback 的 writer 最后 Drop 后才释放。
- public prepare/recovery 短锁可在同调用线程重入；长期 owner FileLock 在独立 worker，公共入口不能借同线程 reentrancy 绕过它。
- guard.prepare 验证 canonical DB/session，当前 input 必须是本会话 promoted/running；SQL trigger 证明 orphan finish 失败会与 typed terminal 整批 rollback。

## 回归和设计依据

专用 core fixture 四条：last writer Drop/路径别名/public recovery 互斥；错误 DB/session 无修改；恢复和 claim cleanup 原子回滚且保留当前/pending/steer/其它会话；terminal trigger 拒绝后 writer 非终态、owner 保持到失败输入收尾，下一 owner 能恢复。

真实 CLI fixture 五条：双进程 busy --new 与持有 socket；强杀已持久 draft 后恢复一次；旧 A+新 B FIFO/provider/user/episode 身份；HTTP 不完整结果失败输入；无法执行桌面 work 的明确失败。均使用隔离 HOME/KANZEI_HOME、临时项目/SQLite 和 localhost HTTP，无真实凭据。

另有真实 CLI+Python SQL 故障夹具（仅手动验证，不引入 Rust 测试运行时依赖）：首 POST held 后仅拒 session.turn_completed，正常 SSE 完成。正确代码 exit1/input failed/session failed/完成终态0；等价旧行为 exit0/input completed/session idle/完成终态0。CLI 私有 helper 也有真实 durable terminal 拒绝单测。

开放边界已交 root：app persistence::persist_round_outcome 的 finish→idle/run.completed/finish_input(true)/succeeded，scheduler run_steps 的 finish→Ok→execute outcome.ok，均没有 is_terminal 检查；同触发器可拒 terminal 却保留成功状态。scheduler 归 A5，app 另包，C4 未声称已修。

既有设计依据：deepseek_harness_upgrade 的 tool_called-before-effect，D-176 主会话统一、D-173 输入生命周期、D-085 Ctrl-C，C3-map 活跃 prepare/recovery 开放路径。本包不拓展 team/代码树租约，不把缺测试、风格当 bug。实际验证和旧实现负对照详见 C4.md/C4-verification.json。
