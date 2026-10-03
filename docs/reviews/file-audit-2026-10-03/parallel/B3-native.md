# B3 native observation — 默认跨 profile

## 判断

真实确认 P1，一条根因：team 的存活 owner 仅由进程内 TEAMS 判定，其他受支持 profile 的 runtime 将活跃 team 误恢复，并可启动同一持久任务的下一 attempt。没有按各表现重复计问题。

2026-10-03 15:02:31–15:02:35（北京时间），只执行一次既有 native-profile-owner.mjs 默认模式，exit 0。源 exe 和 fixture 复制 exe 的 SHA256 都是 `23cf09e9c37f977f20d4a425a3050f1ef7b0e1154dc25109b8a75ede9370baf0`，82754048 bytes。parent 给定基线 root HEAD `0e5355c6` 加未提交 M3 Rust，team 未变；本次没有运行 Git 确认 HEAD。

原始证据：`C:/Users/kanzei/.codex/worktrees/audit-b0/kanzei code/output/B3/run-1791010951151/evidence.json`。root另保存副本 `output/parallel-B3/native-evidence.json`，SHA256与观察JSON一致。
提炼证据和源码切片双 hash：[B3-native-observation.json](B3-native-observation.json)。不新增源码全文 coverage。root随后已将同一M3源码提交为 `b2983292`；执行用exe hash保持一致。

## 实际状态、路由、请求数

- 同一复制 exe，两个独立 KANZEI_HOME/profile；service A PID 57624 / 127.0.0.1:6487，B PID 61404 / 127.0.0.1:14363，build 都为 1791010672018715900。endpoint 必须匹配自己 spawn 的 PID、exe，真实 authenticated runtime IPC 已 ready。
- 唯一 fixture project 是 run-1791010951151/project；共享 project/.kanzei/state.db，owner `ses_project_ce4381f5cd2b7f34`，job `agent-1791010954162-0`。
- A spawn plan 后，真实 A get 返回 running、attempt 1、revision 0、updated_at 1791010954216。第一 POST 在 07:02:34.224Z 到达 loopback provider，只包含 A assignment。
- B list 返回 interrupted、revision 1、updated_at 1791010954298。紧接 A get 返回相同 id/owner 的相同 interrupted/revision 1；此时 first_socket_held=true。这不是 B 的 UI 缓存，也不是只看脚本 flag。
- B resume 接受同一 job，message_id agent-1791010954321-0。第二 POST 在 07:02:34.371Z 到达，包含 A retained history 和 B explicit follow-up；两请求都指定正确 fixture checkout、POST /v1/chat/completions、model primary、stream=true。没有误发到其他项目或真实 provider。
- 第一 socket 在第二请求到达后的最终检查仍未 destroyed/writableEnded；累计真实 POST=2。最终只读 sqlite snapshot 有同一 job 的 attempt 1/2 两条 meta trace，以及 B follow-up received。因此确认另一 native runtime 接管并发请求 admission。未记录第二 socket 独立状态或 worker OS PID；worker 是 service 内 async task。不要扩大为已观察到并发工具写入。
- 脚本没有返回任何模型响应。cleanup stop/shutdown 后，mode=ro 读取 DB 为 stopped、attempt 2、revision 3；这是清理后的状态，不能当作误恢复瞬间的 DB snapshot。误恢复瞬间来自真实 A/B get/list，并由 TeamStore::get/list 的 SQLite SELECT 实现核实。
- errors=[]；脚本退出后 Get-Process -Id 57624,61404 无存活结果。fixture 和证据保留。没有 Cargo、生产源码更改、仓库 Git 操作或真实用户配置读取。

## Failure path 和 DB/owner 边界

1. `kanzei-app/src/state.rs:637`：默认 process/未指定 process_id 都映射 project_session_id(root)，不含 KANZEI_HOME。`core/store/session.rs:1137` 项目 DB 固定 root/.kanzei/state.db。不同 profile 打开同一项目，合法共享同一 DB/session；本问题不是 hash 理论碰撞。
2. `tools/team/store.rs:78`：表主键 (owner,id)。list/get 从此 DB 按 owner 直接 SELECT；update 用 SQLite Immediate 事务，保证单次更新，却不提供跨整个活跃 worker 生命周期的 owner。
3. `tools/team/mod.rs:53–79`：TEAMS、registry mutex、key(root,owner) 都只在本进程。A 在自己的 registry 持有 team。
4. B `app/agent_team.rs:74–83` 的 list/get 没有本地 team → store_for_inspection。`team/mod.rs:97–103` 只检查 B 的 TEAMS，随后 recover_interrupted；`82–90` 将共享 DB 中 active job 改为 interrupted/revision+1，尽管 A 仍活着。
5. B resume 经 `app/agent_team.rs:107–175` 组装模型路由，AgentTeam::attach 的 `team/mod.rs:156–159` 同样只用本地 registry 并恢复。
6. `team/mod.rs:421–428, 695–760` resume/message 入队；`763–796` launch_locked 的 active.contains_key 只检查 B 本地 active，因而启动第二 worker。
7. `990–1061` worker 将同一 job attempt 加到 2；`1172–1185` transcript callbacks 指向同一 DB/job；`1225–1247` await 模型请求期间没有跨进程 owner。A 首请求不会因为 B 的恢复动作即时取消。
8. `879–915` 本地取消/active token/attempt 检查能阻止部分晚更新，但不防其他进程误恢复或第二模型请求；observe 的 `948–986` 和 history callback 也不能替代生命周期 owner。
9. `800–876` finish_worker 在 local lifecycle 下收尾、删除 active，并可能续派；`279–335` stop/stop_all 只取消本进程 worker，故 future owner 释放必须覆盖 cleanup 和 late callbacks。

## 完整真实入口和 caller 地图（源码切片，不全文计数）

| 入口 | 实际 caller | 到达 team 的路径 |
| --- | --- | --- |
| 桌面 Tauri/native IPC | main.rs:310 注册；agent_team.rs:12 command → execute_impl:60 | absent list/get → store_for_inspection；其余 → attach → ui_command |
| 任务面板轮询/重建 | ui/27-agent-team.js:128 refreshAgents，每 3s/会话视图切换；:20 restoreAgent get | 只打开第二 profile 的面板就能触发误恢复，不要求先 resume |
| 面板显式操作 | ui/27-agent-team.js:15 agentCommand；:55 restart、:76 adopt、:84 message；ui/05-subagents.js:1132 stop | 通过 job.project_dir/process_id 路由真实 command；message 与 resume 同一 queue_message 路径 |
| parent run 的模型 task | app/run/coordinator.rs:303 attach，:323 放入 runtime.options.host | core/runner/subagent.rs:507 host.execute → team DelegationHost::execute/command；core/runner/drive.rs:376 wait |
| 持久子问题回复 | app/durable_questions.rs:115 reply_to_question → agent_team.rs:42/49 execute_impl | 可重新组装 attach，再 question_reply/launch；同样需要 owner 边界 |
| 子任务间消息 | tools/team/tools.rs:102–109 Messaging::execute → team.message | 本地 team queue_message/launch；不能绕过持有 owner |
| runtime 兼容探测 | app/runtime_service/compatibility.rs:113 旧协议迁移 probe 调真实 list | 与 absent list 同入口；当前默认复现不依赖旧协议 |
| stop/关闭 | app/commands/run.rs:211,278；processes/lifecycle.rs:844,943 | find → stop_all；必须等真实 worker/进程收尾才释放 owner |
| 状态辅助读取 | collaboration.rs:498 find/list；side_question.rs:259 find，:264 TeamStore::open/list；runtime_service/compatibility.rs:145 find/list | 后两者按直接只读路径读取，不调用 recover；不要另算恢复问题 |

全仓搜索未找到 CLI/mobile 直接 AgentTeam::attach 或 agent_team_command caller；CLI 独立 subagent 路径不能未经实证升级成本问题已复现范围。上表两个生产 attach 点是 app/agent_team.rs:159 和 app/run/coordinator.rs:303；单个生产 store_for_inspection caller 是 app/agent_team.rs:76。

## C4 primitive 复用的最小边界（待 API 合入）

C4 协调告知计划 API：`SessionExecutionGuard::try_acquire(state_path: &Path, session_id: &str) -> io::Result<SessionExecutionGuard>`。canonical DB path + session 独占，blocking worker 持 existing base 非 Send FileLock，Send guard RAII 释放。此 API 尚未验证/合入，本次没有读 dirty 源码作为证据。

可以复用这个 primitive，避免新 lease/heartbeat/PID 存活框架，但不能直接用 parent 原 session key：coordinator:303 attach 发生于 parent round 中，parent 可能已持同 key guard；必须为 team 使用稳定、所有进程一致、与 parent session 隔离的 owner namespace。确切 key/释放策略交 root 与 C4 确认，DB owner/id 和存储格式不需变。

- attach：在 recover/register 之前 acquire；busy fail closed，不能误恢复、入队或启动远程 owner 的第二 worker。guard 由实际 team/worker生命周期持有，不能只围住一次 DB update 或 attach 调用。
- inspection：本地 registry fast path 保留。无本地 team时，成功 acquire 才允许 orphan recovery；busy 时只读取当前持久记录，不能把运行中改 interrupted。检查与恢复必须在同一 guard 下防 TOCTOU。
- guard 必须覆盖整个 worker、模型 await、问题等待、finish/kill/晚 transcript callback，并允许多个同 team 子 worker 共用它；不能用一个 parent-session guard把合法兄弟并行任务串行化。
- TEAMS 目前全局强 Arc、没有 unregister/drop API。若将 guard 直接放 Inner，则 owner 会保留到 service 退出；这虽能防本问题，但跨 profile 合法 idle takeover 会改变。需要 root 明确该合同，再限定最小释放策略，不凭原则新增框架。
- 当前测试 `team/tests.rs:426 inspection_rechecks_registry_after_an_absent_snapshot_and_recovers_only_orphans` 通过 remove TEAMS 模拟进程重启，但仍保留 live AgentTeam Arc。新增 OS owner 后，这不能继续代表真实 owner 已死亡；需要分别检查“registry absent 但 guard仍活着”和“最后 guard释放后 orphan recovery”。
- 建议最小 D2 生产 scope：tools/team/mod.rs（owner admission/recovery/lifecycle）+必要测试 team/tests.rs；如 primitive 需要不变式 accessor，再与 C4协调，先不更改 core/app/source。

## 下一步验证需求

待 root 分派 D2：使用真实跨进程受控回归证明 B list 不改 A running/revision；B resume 在 A alive 时明确 busy 且不产生第二 POST；A 正常继续/stop不受影响；owner退出后允许 orphan recovery/显式续做，保留 transcript。再执行这同一个 supported native默认场景；仍保留失败/空结果。现在没有实施修复或新增测试通过声明。
