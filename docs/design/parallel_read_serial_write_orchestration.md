# 多进程代理编排：并行查、串行写

- 状态：设计基线
- 日期：2026-08-10
- 关联需求：R-171、R-050、R-117、R-138、R-141
- 关联缺陷：D-227
- 关联决策：无；本设计记录 2026-08-10 用户定调，稳定实施后再评估是否提升为长期 A-* 决策

## 背景与问题

kanzei 已经具备两块可复用能力：

- `task` 子代理可以在同一轮并行运行，子代理快照只包含 `read`、`glob`、`grep`；
- 普通工具在单个 runner 内按 `ToolConcurrency` 切 wave，冲突调用会被拆开。

现状没有形成多进程代理流的完整闭环。多个 `ProcessHandle` 各自拥有 runner，单个 runner 内的冲突判断无法阻止两个进程同时写同一项目；实现阶段的普通只读工具仍可能按 wave 并行；`quick_req`、tracker、`test_record`、worktree 命令等独立入口还可能绕过主对话的执行顺序。D-227 已出现并发 `test_record` 生成相同 ID 并覆盖记录的实例。

用户给出的核心调度原则是：

> 并行查，串行写。

本设计把这条原则落实为项目级执行契约：勘察与复核阶段允许多个只读子代理并行；进入实现后只允许一个写代理持有项目写租约，该代理的所有普通工具调用严格按原始顺序串行执行。

## 目标与非目标

### 目标

1. 把“基线 → 勘察 → 实现 → 集成 → 复核”变成可观察、可恢复的代理阶段流。
2. 勘察与复核阶段通过只读子代理并行降低等待时间。
3. 同一项目在任意时刻最多存在一个写代理，避免跨进程语义交错。
4. 实现阶段的工具调用保持 FIFO，不因模型一次返回多个 tool call 而并发执行。
5. 主对话、独立 Tauri 命令、tracker、memory、Git 与 worktree 操作共享同一写入仲裁入口。
6. 保留真实调用轨迹，能回答谁在排队、谁持有写权、何时释放以及失败原因。

### 非目标

1. P0 不实现图形化 DAG 编辑器。
2. P0 不允许子代理获得通用写工具。
3. P0 不建设跨机器分布式调度器；先覆盖当前应用内多个 `ProcessHandle`，接口为后续 OS 进程锁或远程 worker 留扩展位。
4. worktree 继续承担隔离、diff 审查、恢复和分支交付，不作为绕过项目级单写约束的通道。
5. 不重建已有 task、session、run.trace 和 ProcessHandle 体系。

## 讨论摘要与方案取舍

### 只按 tool call 加锁不足

如果每次 `tool.execute()` 前临时加锁，进程 A 的第一次写与第二次写之间可能插入进程 B 的修改。单个调用没有重叠，整个实现语义仍然交错。因此写租约必须归属一次 writer run，并持续覆盖实现与集成阶段，直到运行结束、取消或失败收尾。

### 只靠 worktree 隔离不足

不同 worktree 能隔离代码文件，但 `.kanzei/project`、memory、state.db、模型账户、发布与合并操作仍共享项目身份和外部资源。用户本轮选择的是严格项目级单写，因此 worktree key 与 project writer key 必须分开：前者标识实际代码树，后者统一指向主项目根。

### 只靠提示词不足

“不要并行写”必须由 runner 和协调器强制。提示词只负责让模型理解阶段目标，不能替代工具白名单、写租约、串行执行器和旁路接线。

## 最终流程

```text
基线(单协调器，只读)
  → 勘察(N 个只读子代理并行)
  → 汇总屏障(等待全部完成或明确失败)
  → 实现(单 writer run，所有普通工具 FIFO 串行)
  → 集成(同一 writer run 串行测试/构建/文档/提交)
  → 复核(N 个只读子代理并行)
  → 复核屏障
  → 修正(重新获取单 writer 租约，串行修复与最终验证)
```

### 阶段契约

| 阶段 | 活跃角色 | 工具规则 | 完成门槛 |
| --- | --- | --- | --- |
| `baseline` | coordinator | 只读、串行取快照 | 分支、dirty 状态、关键文档和验证入口已记录 |
| `scouting` | read agents | `read/glob/grep`，允许并行 | 所有任务完成、失败或超时均有终态 |
| `synthesis` | coordinator | 不写仓库 | 形成调用链、文件归属、实施顺序和验收矩阵 |
| `implementation` | writer | 只读 task 允许（见下注）；所有普通工具串行 | 每个改动批次有对应最小验证 |
| `integration` | 同一 writer | 测试、构建、迁移、文档和 Git 串行 | 集成验证结果落轨迹 |
| `review` | read agents | 只读并行 | 契约、测试与交付质量报告全部归位 |
| `fixup` | writer | 重新获取写租约，工具串行 | 复核问题关闭并完成最终验证 |

> **2026-08-10 口径修订（`implementation` 阶段的 task）**：本表初版写的是「task 禁用」，与**不变量 9**（「只读代理可以在 writer 活跃时继续读取已经存在的状态」）以及 R-173 验收④（「writer 活跃时允许只读勘察继续」）直接冲突。两处冲突时**以不变量为准**：表里那格表达的是*阶段纪律的期望*（勘察应当在勘察阶段做完），被误当成了*安全约束*。
>
> 它不是安全约束的理由：只读子代理的工具集在**构造时**就只有 `read/glob/grep`（`crates/kanzei-tools/src/subagent.rs` 的 `SubagentBase`），且子代理内 `ask` 恒 Deny——writer 阶段跑 task 在代码层面不可能产生写入，破坏不了单写语义。
>
> 这条修订有实际代价背景：R-171 把 task 注册挂在 `!execution_policy.is_serial_writer()` 上（`crates/kanzei-core/src/runner/drive.rs`），而桌面端主对话无条件设 `ReadParallelWriteSerial`（`crates/kanzei-app/src/run.rs`），结果是**桌面端主对话根本不注册 task 工具**，「并行查」被整个关掉、读槽登记代码不可达。修订后 task 注册不再受 policy 门控，串行强制仍只作用于普通工具。

### 推荐勘察角色

1. `architecture_scout`：crate、模块、入口与依赖方向。
2. `runtime_scout`：主代理、task、ProcessHandle、SessionRuntime 调用链。
3. `write_surface_scout`：文件、Git、tracker、memory、SQLite、后台进程的写入口。
4. `test_scout`：现有测试、缺口与 E2/E3/E4 验证边界。
5. `docs_scout`：requirements、defects、goals、design 与代码状态一致性。

子代理输出统一包含：事实结论、证据路径与行号、风险、建议改动面；不得直接实施建议。

> **2026-08-11 口径明确(R-173 批6 交付回写)**：本节是**给编排器的角色表**，不是"建议模型这样分工"。
> 勘察由阶段编排对象按本表直接派发（`crates/kanzei-app/src/phase_pipeline.rs` 的 `SCOUT_ROLES`），
> 不经模型的 `task` 工具调用。
>
> 理由是**屏障**：让模型自己派 `task` 也能并行，但那样汇总屏障无从谈起——模型什么时候派、
> 派几个、派完没有，编排对象都不知道，`join_scouts` 拿不到任何可等待的终态，不变量 2
> 就退化成一句提示词恳求。按角色表派发拿到的是一组确定的 future，屏障才有东西可等。
>
> 两条路(编排派发 / 模型自派 `task`)走的是**同一个** `run_subagent`
> (`kanzei_core::run_read_agent` 是薄封装)，只读白名单、`ask` 恒 Deny、读槽登记与
> RAII 回收完全一致——不存在"编排派的子代理走了另一条没人管的路"。模型自派那条路
> 仍然保留，writer 阶段也可用(见阶段契约表 `implementation` 行的修订说明)。
>
> **复核阶段同构**：`review` 的角色表由该阶段完成门槛("契约、测试与交付质量报告全部归位")
> 直接导出，实现见同文件的 `REVIEW_ROLES`。
>
> 角色的模型路由由 `[models] scout` 配置(取值与 `primary`/`fast` 同一套解析)；
> 未配置时沿用 `fast`。并行角色数上限复用 `[limits] max_tasks_per_turn`，不另立新键。

## 核心不变量

1. `read_agent` 的运行时工具集合严格等于审计通过的只读白名单；构造后和执行前各复核一次。
2. `scouting` 的全部任务进入终态前，不得进入 `implementation`。
3. 同一规范化 `project_root` 同时最多一个 `writer_run_id`。
4. writer 租约覆盖实现与集成阶段，不允许在两个工具调用之间切换写代理。
5. writer 阶段 `max_parallel_tools` 强制为 `1`；模型返回多个 tool call 时按原始下标顺序逐个执行并归位结果。
6. 权限询问在获取写租约前完成；用户拒绝后不得占用写租约。
7. 停止、关闭、panic 收尾和窗口退出都必须释放写租约并给排队者确定终态。
8. `quick_req`、tracker、goal、memory 写工具、`test_record`、Git 写操作、worktree 创建/合并/清理不得绕过协调器。
   **2026-08-10 补注（租约辖区 vs 文件锁辖区）**：本条约束的是**代理发起的写动作**。另有一类写入不归租约管——UI 只读命令顺手做的**幂等归档**（如 `docs_snapshot` 开头的 `archive_terminal`，只在「有条目刚进终态」时写盘）。它必须走 R-138 的**毫秒级文件锁**（`crates/kanzei-tools/src/atomic_file.rs` 的 `FileLock`，限时 `try_lock` 拿不到就跳过并落 `warnings`），**不得挂写租约**：`MemoryCoordinator::acquire_writer_lease` 无超时，挂上去会让文档面板在 agent 跑一轮期间整段卡死——拿一个更严重的问题换一个更轻的。
   判据：**谁发起的**。代理的写动作 → 租约；界面读路径顺手做的幂等维护 → 文件锁。两者的目标都是「不并发写坏」，但排队语义不同，不可互相替代。
9. 只读代理可以在 writer 活跃时继续读取已经存在的状态；复核阶段必须等 writer 释放租约后再启动，保证审查的是稳定快照。
10. dirty 工作树属于用户；基线必须记录，writer 不得清理或覆盖无关修改。

## 接口设计

### 执行策略

建议在 runner 配置中新增显式策略，避免用 `max_parallel_tools = 1` 暗示完整语义：

```rust
pub enum ExecutionPolicy {
    Default,
    ReadParallelWriteSerial,
}
```

`ReadParallelWriteSerial` 同时约束 task 使用阶段、writer 租约和普通工具执行模式。

### 项目级协调器

```rust
pub trait ProjectExecutionCoordinator: Send + Sync {
    async fn acquire_read_slot(&self, request: ReadSlotRequest) -> Result<ReadPermit>;
    async fn acquire_writer_lease(&self, request: WriterLeaseRequest) -> Result<WriterLease>;
    fn cancel_waiter(&self, run_id: &str);
    fn snapshot(&self, project_root: &Path) -> CoordinatorSnapshot;
}
```

首个实现由桌面端 `AppState` 按规范化主根共享。CLI 使用同一接口的单运行实现；未来需要多个 OS 进程同时操作同一项目时，再增加文件锁或持久 lease 实现，不改变 runner 调用契约。

### 上下文双键

`ToolCtx` 需要明确携带：

- `cwd`：实际代码工作树；
- `project_root`：项目身份与托管文档真源；
- `worktree_key`：工作树内工具冲突键；
- `project_write_key`：跨进程 writer 仲裁键；
- `run_id`、`process_id`：租约归属和审计身份。

### 事件

新增或补齐以下持久事件：

- `orchestration.phase_changed`
- `orchestration.agent_started`
- `orchestration.agent_completed`
- `orchestration.agent_failed`
- `orchestration.barrier_reached`
- `writer.queued`
- `writer.acquired`
- `writer.released`
- `writer.cancelled`
- `writer.recovered`

活动面板按这些事件聚合阶段、代理数、token、工具数、耗时和写队列状态；历史回放继续以 state.db 与 `run.trace` 为真源。

## 实施边界与调用方

### P0：强制执行地基，对应 R-171

1. 在 harness/core 增加执行策略、项目写键、writer 租约接口和确定性状态机。
2. `drive.rs` 在实现模式禁用 task，并强制普通工具走串行路径。
3. writer run 获取一次租约并跨工具调用持有；所有结束路径统一释放。
4. `kanzei-app` 的所有 ProcessHandle 共享项目级协调器。
5. `quick_req`、tracker、memory、test_record、Git/worktree 等旁路接入同一仲裁入口。
6. 事件进入现有 session/run 轨迹，先做可观察性，不先扩复杂 UI。

主要文件：

- `crates/kanzei-harness/src/tool.rs`
- `crates/kanzei-core/src/runner/drive.rs`
- `crates/kanzei-core/src/runner/tool_exec.rs`
- `crates/kanzei-core/src/runner/event.rs`
- `crates/kanzei-app/src/state.rs`
- `crates/kanzei-app/src/run.rs`
- `crates/kanzei-app/src/processes.rs`
- `crates/kanzei-app/src/subagents.rs`

### 后续批次

- **P1：阶段编排对象、汇总屏障、全局只读并发预算与失败策略 —— 已交付(R-173，2026-08-11)。**
  - 契约在 `crates/kanzei-harness/src/orchestration.rs`：`Phase` 七阶段 + 合法迁移表、
    `BarrierKind`/`ScoutOutcome`/`BarrierOutcome`、`PhaseError`、`PhaseObserver`，
    以及 `OrchestrationEvent` 的落库单一出口(`event_type()` / `payload()`)。
  - 实现在 `crates/kanzei-core/src/phase.rs`：`PhaseOrchestrator` 持有写租约不外泄，
    `join_scouts` 是进入 `synthesis` 的唯一通路，`enter_review` 交出租约后才能进 `review`。
  - 接线在 `crates/kanzei-app/src/phase_pipeline.rs` + `run.rs`：**只在自主推进轮装配**
    (手动一问一答不构造编排对象，运行路径与引入前逐字节相同)。
  - **并发预算**沿用 `[limits] max_tasks_per_turn`；**失败策略**为「失败/超时不中止，
    但必须让模型知道」(`BarrierOutcome::model_notice`)；**屏障上界**为
    `[limits] barrier_timeout_secs`，缺省由 `subagent_timeout_secs` 推导且强制宽于内层。
- P2：活动面板按阶段展示、writer 排队/持有者/取消与历史回放。
  - 事件侧已就绪(全部 `orchestration.*` 事件落 `session_events`，`sequence` 单调可回放)；
    面板渲染与单条停止归 R-174。
- P3：worktree 真实绑定、稳定快照复核、崩溃恢复和未来 OS 多进程协调器实现。
  - 补注:R-173 的复核屏障保证的是「**本 run 已交出写权**」，不是「项目全局无 writer」——
    释放瞬间另一个 ProcessHandle 的排队 writer 会立刻接手。跨进程的全局稳定快照留在 P3；
    等全局静默会被后来的写者饿死，需要另设策略。

## 验收矩阵

| 场景 | 通过条件 |
| --- | --- |
| 并行勘察 | 至少两个只读子代理执行区间真实重叠，且运行时不存在写工具 |
| 屏障 | 最慢任务完成前 writer 不启动；失败/超时不会让屏障永久挂起 |
| runner 串行 | writer 阶段普通工具最大 in-flight 恒为 1，结果按模型调用顺序归位 |
| 跨进程单写 | 两个 ProcessHandle 同时申请 writer，实际持有区间不重叠且顺序可审计 |
| 运行级租约 | 同一 writer 的连续写调用之间不能插入第二个 writer |
| 读写共存 | writer 活跃时允许只读勘察继续；稳定复核等待 writer 释放后开始 |
| 旁路收口 | quick_req/tracker/test_record/Git/worktree 写入均出现 writer 事件，无法绕过 |
| 取消恢复 | 排队取消、运行停止、panic 收尾和重启后均无永久占用租约 |
| dirty 保护 | 用户预存修改保持不变，writer 只改计划内文件 |
| 真实闭环 | 一次需求完成并行勘察→屏障→串行实现/验证→并行复核→串行修正全轨迹 |

> **2026-08-11 证据落点(R-173 交付)**：上表中由 R-173 承接的四行，可复核证据分别在——
> 并行勘察 = `crates/kanzei/tests/parallel_scouting_under_serial_writer.rs`(真实 HTTP 请求体里
> 断言 `task` 已注册 + 读槽区间重叠)；屏障 = `crates/kanzei-app/src/phase_pipeline_tests.rs`
> 的七阶段闭环测试(汇总屏障事件早于第一次 `writer.acquired`)与 `crates/kanzei-core/src/phase.rs`
> 的三终态测试(含永不返回的任务由外层上界收敛)；读写共存 = `phase.rs` 的
> `writer活跃时读槽仍可获取` 与前述集成测试(整轮真实持租约);真实闭环 = 同一份七阶段
> 闭环测试(事件流从 `session_events` 读回、`sequence` 单调、七阶段按序)。

## 与既有设计的关系

- `subagent_management.md`：复用其只读子代理与可观察性基础，本设计补项目级调度纪律。
- `deep_parallel_dev.md`：继续复用 worktree、显式主根、diff、恢复与模型隔离设计；其中“多条线同时写”的旧方向受本设计约束，当前口径改为多进程并行查、项目级单 writer。
- `memory_control_plane.md`：R-161～R-167 按当前顺序优先推进；R-171 排在该开发序列之后，避免本次调度改造与正在进行的 memory runner/store 改动并发写同一批核心文件。
- R-138：docstore 原子写与文件锁仍有价值，用于保护非 runner 或未来 OS 进程入口；不能替代运行级 writer 租约。
- R-141：显式主根与 worktree 身份拆分是本设计双键上下文的前置基础。

## 变更记录

- 2026-08-10：依据用户“并行查、串行写”定调建立设计基线；确定项目级单 writer、实现阶段全工具串行、并行复核后串行修正；关联 P0 需求 R-171。
- 2026-08-10（交付后修订）：R-171 已交付并关闭；R-173 承接阶段编排对象与两道屏障。本次并行交付一批同族条目后，对本文做三处口径修订：
  1. **阶段契约表** `implementation` 行「task 禁用」改为「只读 task 允许」——它与不变量 9 冲突，且实测把桌面端的「并行查」整个关掉了（详见该表下方的修订说明）。
  2. **不变量 8** 补注租约辖区与文件锁辖区的判据（详见该条）。R-138 的跨进程文件锁已交付，`docstore` 的四个整文件写点全部改 tmp+rename 原子替换，`TrackerTool` 的写动作分支在 `load → next_id → save` 整段持锁。
  3. 「与既有设计的关系」中对 R-138 的定位（「保护非 runner 或未来 OS 进程入口」）已由实测确认为真需求：`kz` CLI 的 tracker 子命令**没有协调器**，与桌面端并发时项目级单 writer 在跨进程层面本就不成立——文件锁补的正是这一层，不是第二套租约。
- 2026-08-11（R-173 交付回写）：**P1 阶段编排对象已交付**，本文四处更新——
  1. 「后续批次」P1 标注已交付并列出契约/实现/接线的落点；P2 补「事件侧已就绪」；P3 补跨进程稳定快照的语义边界。
  2. 「推荐勘察角色」明确为**给编排器的角色表**而非对模型的分工建议，并写明为什么必须由编排器派发（屏障需要可等待的确定终态，模型自派拿不到）。
  3. 交付过程中在 R-171 既有实现里发现并修掉三个**同族缺陷**，成因相同——代码路径不可达时缺陷不会暴露：`release_writer` 交接路径不发 `WriterReleased`（审计断档）、`WriterReleased` 的 `process_id` 恒为空串、`ReadPermit` 按 `agent_name` 回收（并行角色同名，回收身份错乱）。前两个在写租约事件真正落库时才可见，第三个在「并行查」恢复后才可见。
  4. 交付中另发现并修掉一个**跨进程写仲裁漏洞**：`normalize_project_root` 未剥 Windows `\\?\` 扩展长度前缀，导致 worktree 命令（走 `canonicalize`）与主对话（走裸路径）落进两个仲裁桶，不变量 8 在 Windows 上实际被绕过。修复点收在该函数一处，两种形态现已竞争同一租约。
  另记两条本次实测澄清：①D-227 的 `test_record` 同 ID 与并发无关（wave 排他与写租约都生效、四条记录全部存活），根因是分配器只读系统时钟不读文件，**串行不等于唯一**——租约在原理上修不掉它；②`core/orchestration.rs` 的 `normalize_project_root` 不剥 Windows 扩展长度前缀 `\\?\`，而 worktree 写命令走 `canonicalize`（带前缀）、主对话 writer 走发现式取根（不带），两者落在**不同的项目桶**，worktree 写入实际绕过了协调器。该缺口在 R-173 批次内修复。

## 验证证据

- 本轮完成只读代码与文档核对，确认 task 并行、SubagentBase 只读白名单、普通工具 wave、ProcessHandle 与 worktree 脱节、独立写入口以及 D-227 并发覆盖记录。
- 尚未修改运行时代码，尚未运行测试；设计状态不代表 P0 已实现。

## TODO 与后续风险

1. TODO：R-171 实施前先补协调器与 runner 并发测试，再接生产调用方。
2. TODO：决定协调器事件仅复用 session_events，还是增加面向查询的 orchestration 投影表；P0 优先复用事件，除非查询性能证明需要新表。
3. 风险：writer run 长时间等待用户输入会阻塞后续写者；实现时权限询问必须发生在租约获取前，运行中 question 是否释放租约需用状态机测试定案。
4. 风险：后台 bash 在工具返回后继续写文件；实现时后台进程必须继承 writer owner，writer 结束前收尾或显式转为受管任务，不能提前释放租约。
5. 风险：未来多个 OS 进程同时打开同一项目时，AppState 内存协调器不可见；P3 必须用文件锁或持久 lease 扩展同一接口。
