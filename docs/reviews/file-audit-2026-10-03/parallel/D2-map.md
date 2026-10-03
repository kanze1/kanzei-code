# D2：每 child 的跨进程 owner

基线 `b298329218d32ec8b4ab405a0a89fc423bbf367c`，既有 A 工作树、分支 `kanzei/audit-d2-team-owner`。C4 已验证 leaf `07c0d3738f97ec85f9eba7e5c39c1c3236908ae4` 合入为本地 `8d0ebb4b`，未读取或复制 dirty 源码。原生旧行为证据已由 root 入库；当前正在执行本包验证，逐层结果见 D2-verification.json，不把旧 binary 实证当新源码正例。

## Scope 与既有 coverage

共八个必要 source：`crates/kanzei-tools/src/team/{mod.rs,store.rs,tests.rs}` 与 `background.rs`、`background/{lifecycle.rs,registration.rs,persistent.rs,monitor.rs}`。team 三文件已由 B2 全文覆盖，仅重开 owner/recovery/claim/worker/checkpoint；后台五文件只检查实际 completion/restore/caller 与构造必要切片，不据此计全文覆盖。persistent.rs 只适配构造字段；monitor.rs 只适配既有测试构造。app/agent_team.rs 重开两个 caller、app/run/coordinator.rs 只看 attach，均未修改。无新增生产文件/dependency/schema/Tauri command/UI 合同；core typed/inbox 等归 C5，不修改。

## 依赖与 key

`base FileLock → C4 core/store/session_execution::try_acquire → TeamStore DB path → child owner → active worker/回调 → terminal cleanup/handoff`。

C4 协调提供的 API：`try_acquire(state_path: &Path, session_id: &str) -> io::Result<SessionExecutionGuard>`，Guard Send+Sync，可放 Arc；独立 blocking worker 持既有非 Send FileLock，last Drop 释放并 join。当前不使用 typed prepare/writer。

team 使用 canonical projectDB + `team:<serde_json((TeamStore.owner, childID))>`，与实际 PK (owner,id) 一致且元组无歧义，排除 attempt；与父 round/session 分离。provider call_id 只在父 session 内唯一，同DB不同parent同call_1合法并行。C4 负责 canonical 完整 DB 身份，D2 不造第二套 normalization/lock-worker/lease/TTL/PID 猜测。不同 child 或不同 owner 下相同 childID 不共享 key，可并行。持久表仍是 (owner,id)，schema 不改。

## 实际 caller 全表

| 真入口 | caller | 行为与本包边界 |
| --- | --- | --- |
| Tauri/native IPC | main.rs:310；agent_team.rs:12/60 | absent list/get 的唯一 store_for_inspection caller :76；attach :159，签名不改 |
| UI 面板 | ui/27-agent-team.js:20 get、:128 list；3s/视图切换轮询 :137–138 | 远端忙只读，不能写 interrupted；无需 UI 改动 |
| UI 操作 | ui/27-agent-team.js:15 agentCommand，:55 restart、:76 adopt、:84 message；05-subagents.js:1132 stop | 本地 worker 共用其 guard；远端 busy 在任何 child state 修改前返回明确错误 |
| parent run | app/run/coordinator.rs:303 attach、:323 host | 第二个生产 attach 点；父 session 与 child key 分离，不改 coordinator |
| 模型 task | core/runner/subagent.rs:507 host.execute；drive.rs:376 wait | Team DelegationHost → command；wait/collect 在无本地 active 时也 probe 同一 tuple owner，remote busy 沿既有 DB/Notify/500ms 等待，不误报 orphan；ack 仅更新 reported 元数据 |
| 持久子问题回复 | durable_questions.rs:115 → agent_team.rs:42/49 | question_reply → queue_message；以 child owner 约束启动，保留原已回答/取消合同 |
| 子任务间消息 | tools/team/tools.rs:102–109 → team.message | 同一 claim/admission；不新增用户授权或自动复活 |
| runtime 兼容 probe | runtime_service/compatibility.rs:113 | 调真实 list；同恢复边界，默认复现不依赖旧协议 |
| 停止/关闭 | commands/run.rs:211/278；processes/lifecycle.rs:844/943 | find/stop_all，只停止本地 active worker；owner 持到 stop/kill/finish 完成 |
| 状态辅助读取 | collaboration.rs:498；side_question.rs:259/264；runtime_service/compatibility.rs:145 | find/list 或直接 TeamStore::open/list，保持只读，无误恢复路径 |

全仓搜到生产 AgentTeam::attach 仅上述两点，store_for_inspection 生产 caller 仅 agent_team.rs:76。CLI/mobile 无直接 attach 或 agent_team_command caller，不扩大已复现范围。

## 生命周期与最新状态合同

1. recovery 只把 list 当候选。逐 child try_acquire；WouldBlock 则不写、不报 interrupted。拿锁后 get 最新 DB 状态，再在 Immediate transaction 内重新判 active；terminal 记录不改 revision。
2. spawn 在 insert/emit/launch 之前 claim 新 child，避免已发布 queued 行但未持 owner 的恢复窗口。
3. resume/message/stop/restart/adopt 先解析稳定 id，取得本地 active guard 或跨进程 tryclaim，再从 DB 重读。busy 不提前修改 message、state、question 或工作树。失败带 child id 和明确原因。
4. active 值是 ChildWorker（CancellationToken + Arc<Guard>）；guard 不挂常驻 TEAMS。不同 child 可独立开始。
5. WorkerRegistration RAII 根据同一 Arc 身份移除原 active。worker/select 与 settle 的 finish/continuation 分别 catch_unwind；异常先取消/kill，等待 child.wait/最后 mailbox callback 与 detached managed guard 的 Shared join，再做真实 Result final restore，才 quiet 保存 failed/stopped 并释放。kill失败的活进程明确写失败说明，owner等待其真实退出，不把 attempted count/PID退出当全部完成，不另造轮询框架。persistent 仍按原合同跳过 child cleanup。
6. LLM await、依赖等待、用户问答、stop 后进程 reap 都保持 guard；没有 lifecycle mutex 跨 await。finish 的终态/清理/显式队列 handoff 共用原 guard，不留可被外进程抢走的 attempt 间隙。
7. transcript provider/sink 保留 guard；sink 在 local lifecycle 下核对当前 worker Arc 和 attempt，再 checkpoint，旧 callback 不覆盖下一 attempt 的 history。最后 callback/worker Arc 释放后 idle team 可被另一进程接管。
8. attach 清空原 parent runtime 的 transcript sink/provider，child worker 会自行安装两个 callbacks。局部 runtime 从 Inner 克隆后只存在 worker stack，没有写回 Inner；core run_subagent 借用它，subagent.rs:596/612/869 同步调用 sink，不能在常驻 TEAMS 形成强 Arc 环或 pin 父 round guard。

## 后台 completion 与真实 writer map

真实路径是 `bash::execute → register_with_mailbox → spawn_guard → reconcile_result/reconcile_restore → quarantine_and_restore`。生产 register caller 仅 bash；spawn_guard 另有 persistent adopt caller，后者不加入 team 非persistent等待。另一个 writer 生命周期是 registration 的 child.wait/readers/最终 mailbox 发布；observer 只同步更新 baseline，monitor 只发布已可关闭 mailbox，不写 team job。

原 spawn_guard 在 was_running=false 后仍先最后 reconcile 再退出；旧 PID 收敛不能证明该写任务结束。现有 BackgroundProcess 构造时已建立 guard Pending/NotRequired 与 exit Pending；registry 发布后不能看到可误判完成的空槽。spawn 后同步填同一个 Shared JoinHandle；多个 waiter clone 同一结果，不 take，不因一个 waiter取消取消原 task。wait 在锁外 await，Pending明确等待；同步显式操作的 retry 对 Pending/running 明确 Err。真实受控回归卡住最终 detached reconcile，PID已死但 owner仍busy，放行/join后才可接管。

reconcile_restore 的实际 quarantine/restore 错误现在保留 Result；原内部 Option wrapper/API保留，guard 的 Result 与 D2 final restore 使用真实失败。旧 join失败只证明旧task终结；仍持owner补偿，成功可继续，不让 cached JoinError永久拒绝。失败保留候选/输出/quarantine，并在 job.latest 用集中私有精确前缀“后台清理未完成：”标记；不新增schema。所有 latest 读写已搜：mod执行/恢复写、store字段、team tools/UI/side_question展示，side_question 的改动只修改读出副本不落DB。

背景非persistent私有filter/kill/join/retry使用既有真实 run_id `owner:childID:attempt`，除 process_id 匹配还要求固定owner/id前缀后的全尾部可解析u32；同parent跨attempt可清理，不会误杀/误join不同parent同call_1。原public kill_process和持久格式不改，persistent原语义不扩大。真实双parent模型/PID回归验证只停A，B状态/请求/PID持续；8条负例中未scope逻辑会真实失败。

新claim读fresh job。marked仍active不提前准入；有旧nonpersistent记录必须实际 joined+restore成功才去标记、写消息；新profile空registry没有旧baseline，不猜已恢复，具体Err零准入。真实双process失败回归覆盖该情况。crash/restart后原记录不可得仍需实际处理，未宣称自动恢复完成。

## 回归与负对照准备

- 新跨进程回归运行真实第二个 Rust test 进程，不共享 TEAMS，使用自建项目和隔离 home、loopback 模型。验证 remote list 保持 running/revision、resume/stop busy 不改 DB/不加模型请求、兄弟 child 并行、idle takeover 保留历史。
- unwind/late-owner 回归验证 active 退出不等于晚 callback 已释放，最后引用消失后才可恢复/再 claim。
- finisher 续派事件 panic 回归登记真实 test-exe 后台 PID，观察 callback 时 owner/PID 都活着，最终 PID 退出、Weak owner 消失才允许再 claim；不是只检查 active 移除。
- parent transcript callback 回归把真正 keyed guard 放入传入 attach 的 callbacks，确认 cached team 清空它们且同 parent key 能重新 acquire。跨进程实跑回归同时确认原 worker/provider/sink 的 Weak owner 消失后才 idle takeover。
- 既有错误写入/stop/handoff/timeout 回归继续保留；fixture token 现在持 guard，模拟 process death 必须真正释放最后 worker 引用，不能只 remove registry。
- 旧 B3 native 双 profile 结果是旧行为控制，不算新源码正例。后续需构建本分支 binary 后再执行默认 native 场景。
- ignored output/parallel-D2/negative.py 执行八个等价逻辑控制：旧恢复、旧进程内准入、旧只读wait、childID-only owner、未按parent筛选background、只看PID不等guard、吞restore错误、空remote registry误判已补偿。保留新支架，每例必须真实回归断言失败且全原字节/hash恢复；不等同完整旧源码。执行状态以 JSON 为准。
- ignored native-child-owner.mjs 正例先验证 B busy 查询/续做/wait 不写 A、不多请求，再 A stop/首 socket 关闭/owner释放后 wait 返回真实终态，显式 B takeover。执行以 verification 为准。

历史依据：B2.md 的显式 resume、不自动重放写入、stop 后回调不复活合同；docs/reports/2026-10-02-harness-repair.md 的 R-369 记录。subagent_management.md 是历史方案，不作为新增业务行为授权。

## 当前状态

最终同source、负例原字节恢复并刷新全Rust后：tools782 pass/3 ignored（54.36s），harness211 pass，team37 pass/2 ignored。workspace check --all-targets、Clippy --all-targets -Dwarnings、fmt --all --check、diff check全部通过。8/8负例为真实断言失败、无编译错误，finally原SHA256恢复。当前分支kzapp build通过；SHA25631bf051ba8ca722b323014eefe3db288f3b2c4ba23eb6b3a9dd1835897ebcf2e、83159552bytes。

默认双隔离profile/native正例通过：A41976/B41496；A首request held时B list/resume/wait不改DB、不加provider；A停止/首socket关闭/最后owner释放后B wait返回真实stopped，显式B takeover正确checkout/followup、attempt2。before-stop请求1，总请求2，全部ownPIDs已退出，无cleanup errors。第一次native因脚本JSON.stringify的Windows路径转义误判routing保留为execution_fault；读取parsed正文确认路径正确，修脚本后同binary重跑通过。两轮完整原证据及全部历史失败日志均保留，清单/hash见D2-verification.json。无公共coverage增量，root整合其它包后仍需统一验证。
