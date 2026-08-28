# 运行画像按执行任务关闭粒度

- 状态：草案
- 日期：2026-08-28
- 关联需求：R-337
- 关联缺陷：D-655
- 关联决策：无（R-337 的确认记录是当前语义边界）

## 背景与问题

当前“运行画像”把一次模型执行落成一个 `episode`，UI 将 episode 展示为“轮次”。鞭挞模式下一个长期 session 会连续承载很多输入和轮次，导致用户看到的是最近若干轮，而不是可比较的执行任务。用户已确认：完成后的主聚合维度应是执行任务关闭；session 保留为任务下钻上下文；未关闭任务独立显示为进行中，不进入已完成趋势。

本条只做审计和设计，不改任务生命周期、事件生产者、历史数据或运行控制逻辑。

## 目标与非目标

### 目标

1. 解释当前画像字段、统计来源、聚合边界和长 session 的信息损失。
2. 设计可评审的 task 身份、关闭事实、聚合指标、跨 session 关系、未关闭状态和 session 下钻。
3. 为后续实现、迁移和测试拆分出清晰边界，保证旧 episode 仍可回放。

### 非目标

- 本条不修改 `run_metrics`、`run_metrics_by_category` 或 UI 生产代码。
- 不从 prompt 文案或时间间隔推断历史 task close，不把推断结果写成事实。
- 不擅自拍板跨 session 续接、谁有权关闭任务、默认指标集合或历史回填策略。

## 讨论摘要

- 用户确认（R-337 确认记录）：运行画像以 task close/执行任务关闭记录为主粒度；长 session 仅作为任务下钻上下文；未关闭任务单独显示为进行中，不进入已完成趋势。
- 现有实现已经有运行身份 `run_id` 和输入身份 `input_id`，但二者都是单轮/单输入关联，不等于用户可理解的执行任务身份。
- `D-655` 已修复轮中压缩造成的轮次统计切片错位；这保证了本轮 episode 的消息真源稳定，但没有改变“episode = round、按 session 查询”的聚合语义。

## B1 现状审计

### 1. 数据与落库

`crates/kanzei-core/src/store/mod.rs:266-286` 的 `EpisodeRecord` 字段是 `session_id`、prompt、outcome、steps、token、工具/上下文/metrics JSON、provider/model、`run_id`、`input_id`、duration 和 overflow。没有 `task_id`、task 状态、task 开始时间或 task 关闭时间。

`crates/kanzei-core/src/store/episodes.rs:10-37` 将一条结果 INSERT 到 `episodes`；`crates/kanzei-app/src/run/persistence.rs:194-231` 在一轮成功收尾时生成 episode，并随后执行 `finish_input(promoted_input_id, true)`。因此当前“完成”是单轮/单输入完成，不是用户任务关闭。

`crates/kanzei-app/src/commands/run.rs:448-464` 的 `run_metrics`：

- 用 `project_session_id(project_dir)` 锁定一个 session；
- `recent_episodes(session_id, limit)` 按 `created_at DESC` 查询，默认只取 20 条；
- 返回名为 `rounds` 的数组，字段包括 prompt、outcome、steps、input/output token、tools、context、metrics、measured。

`crates/kanzei-core/src/store/episodes.rs:80-106` 的 SQL 明确是 `WHERE session_id = ?1 ORDER BY created_at DESC LIMIT ?2`，没有 task join、task close 过滤或跨 session 聚合。

`crates/kanzei-app/src/commands/run.rs:518-603` 的分类接口同样读取 session 下的 episode；它从 prompt_head 提取第一个 `R-/D-`，再读取需求/缺陷复杂度，按 `(kind, complexity)` 聚合。它回答的是“轮次按条目类型/复杂度的 token 汇总”，不是“任务完成画像”。

### 2. UI 展示

`crates/kanzei-app/ui/13-memory.js:150-166` 的 `refreshMetrics` 并行调用 `run_metrics(limit: 20)` 和 `run_metrics_by_category(limit: 200)`，再加载 incident metrics。`crates/kanzei-app/ui/03-shell.js:150-154` 在切换到 `metrics` 视图时触发它，是真实入口和消费者。

`crates/kanzei-app/ui/13-memory.js:218-275` 的 `renderMetrics`：

- 趋势只计算 `measured` 的轮次；
- 仅展示平均终端调用、平均 git 查询组、edit 未命中率、平均步数、平均输出 token 这 5 项；
- 明细每行仍是 timestamp、outcome、steps、token、prompt、工具和本轮 metrics；
- 没有 task 列表、task 状态、关闭时间、任务级总耗时或 session 下钻入口。

因此“统计量少”不只是缺几个字段，而是主对象选成 round 后，任务级开始/关闭和跨轮归属根本没有进入 UI 数据契约。

### 3. 真实本地数据库证据（V2）

读取目标是真实项目 `.kanzei/state.db`，不是测试夹具。执行的是 PowerShell here-string 传 Python 的只读 SQLite 查询，查看 `episodes`、`session_inputs`、`session_events` 的 schema、计数、状态和事件类型。

最小复现命令（项目根目录 PowerShell，只读）：

```powershell
python -c 'import sqlite3; c=sqlite3.connect(".kanzei/state.db"); print("episodes_sessions", c.execute("select count(*),count(distinct session_id) from episodes").fetchone()); print("main_session", c.execute("select count(*),count(distinct run_id),count(distinct input_id) from episodes where session_id=\"ses_project_c0b8d633186c2464\"").fetchone())'
```


结果（B1 首次采样快照）：

- 数据库大小约 171,495,424 bytes；`episodes` 共 1157 条，覆盖 11 个 distinct session。
- 首次采样时最大的真实 session `ses_project_c0b8d633186c2464` 有 1072 条 episode、1071 个 distinct `run_id`、1071 个 distinct `input_id`，时间范围为 `1786205959162..1787896197978`。
- 首次采样时同一 session 有 1310 条 `session_inputs`：completed 1084、cancelled 162、failed 63、running 1；其中 3 条输入的 `finished_at` 为空。episode 与 input join 后没有孤儿 episode，但 episode 只覆盖 1071 个不同 input_id，不能代表全部输入状态。
- 首次采样时 `session_events` 的高频完成事件是 `run.completed` 1014 条、`session.turn_completed` 21 条；事件类型中没有 `task*` 或 `close*`。
- 首次采样时最新 `run.completed` payload 的键是 `context`、`halted_by_user`、`input`、`output`、`steps`，没有 `task_id`、task 状态或 task close 时间。
- 后续复核 `T-1786922726829` 输出 `episodes=1158/sessions=11`、主 session `episodes=1073/distinct run_id=1072/distinct input_id=1072`，说明当前运行仍在追加数据；这不改变 task/close 事件缺失的结构性结论。
- 首次采样时主 session 最近 episode 的每条都有独立 `run_id`/`input_id`，但 UI 仍将它们命名和渲染为 rounds；当前 `limit: 20` 会把长期 session 压成最近 20 轮窗口。

这组数据证明了两个边界：真实系统确实存在“长 session 多输入/多轮”样本；同时历史库没有可用的 task close 事实，不能诚实地把旧数据直接改写为已完成 task。

## 候选方案

### 方案 A：继续以 episode/round 为主，只增加字段

- 优点：不改事件模型，迁移成本最低。
- 缺点：无法回答一个长 session 中哪些轮次属于同一执行任务，也没有关闭事实；只能继续展示执行碎片。
- 结论：不满足用户已经确认的 task close 主维度，否决为最终方案。

### 方案 B：事件驱动的 task projection（候选基线）

- 在不改写 `session_events`/`episodes` 证据的前提下，增加 task start/close 事实和可重建的 task projection。
- episode 通过 `task_id` 或明确的 task membership 关联到任务；任务聚合消费已完成 task，session 作为一个或多个任务的上下文下钻。
- 未收到 close 的 task 进入进行中列表，不进入已完成趋势。
- 历史 episode 没有 task close 时保持 legacy/未归属状态，不猜测完成。
- 优点：关闭语义明确、可回放、能把多轮聚合与单轮下钻分开；缺点：需要确定身份生产者、关闭权限、跨 session 续接和历史兼容。
- 结论：作为 B2 设计的起点，不在本批直接实现。

### 方案 C：从 prompt、时间间隔或连续鞭挞轮次推断 task

- 优点：不新增事件即可快速给旧数据分组。
- 缺点：推断不可证伪，可能把一次任务拆开或把多个任务合并；会把不存在的 close 事实伪装成历史数据。
- 结论：拒绝用于生产聚合；最多作为明确标注的分析辅助，不作为画像真源。

## 最终方案

### 1. 对象边界与权威关系

将运行画像拆成四个不同粒度，禁止互相冒充：

| 对象 | 身份 | 生命周期 | 画像作用 |
| --- | --- | --- | --- |
| session | `session_id` | 长期对话容器 | 任务下钻上下文，可承载多个 task |
| task | 新增不透明 `task_id` | `task.started` 到显式 `task.closed` | 已完成趋势和主指标的唯一主对象 |
| input | 现有 `input_id` | admitted/promoted/running/finished | 一次用户输入/投递事实，归属 task |
| round/episode | 现有自增 `episode_id`，并带 `run_id` | 一次模型执行收尾 | task 的细节下钻与指标加总来源 |

事实真源分层：append-only `session_events` 保存 task start/close/attach 等事实；现有 `episodes` 和 `session_inputs` 保持原始事实；后端从这些事实重建 task projection；前端只消费后端 projection，不在浏览器按 prompt、时间或 session 自行分组。`run.completed`/`session.turn_completed` 仍是执行/轮次完成事实，不替代 task close。

### 2. task 身份与开始/关闭事件（推荐基线，实施前需确认入口）

- `task_id` 使用不透明、全局唯一的字符串；不复用 `input_id`、`run_id` 或需求/缺陷编号。推荐由 task 事实生产者在显式开始一个新任务时生成，禁止从 prompt 文案或时间间隔推断。
- `task.started` 至少携带 `task_id`、产生它的 `session_id`、`started_at` 和可选的用户标题/首个 `input_id`。同一 task 的后续 input/episode 通过明确 membership 事实关联。
- `task.closed` 至少携带 `task_id`、`closed_at`、`outcome` 和关闭来源/操作者。只有该事实使 task 进入 completed/failed/cancelled 等已关闭集合；普通 `run.completed` 只结束一轮。
- 同一 task 的 close 必须幂等：重复 close 不增加趋势样本；close 后再来的输入必须拒绝或创建新 task，具体错误/新建策略列入实现条目。
- 用户已确认“关闭 task 是主粒度、未关闭单列进行中”；但 task start 的 UI 入口、关闭权限、outcome 枚举和 close 后继续输入仍是待评审语义，本条不擅自拍板实现。

### 3. 跨 session 归属

推荐基线是“默认 task 隶属于开始它的 session；只有显式 resume/attach 事件才允许跨 session”。projection 的 task 行保存 `session_ids[]`，每个 input/episode 仍保留自己的原始 `session_id`，从而可以在任务下钻时按 session 分段。

不允许通过相同 prompt、相邻时间或同一项目路径自动把两个 session 合并。是否允许用户在 UI 中把进行中 task 续接到新 session、续接后旧 session 的展示方式和权限审计，属于待用户拍板的高影响语义；在确认前实现条目应默认禁止跨 session attach，而不是隐式支持。

### 4. 主指标、未关闭任务与去重

已关闭 task 的主行建议包含：`task_id`、标题、状态/outcome、started_at、closed_at、wall_duration_ms、session_count、input_count、round_count、steps_sum、input_tokens_sum、output_tokens_sum、tool_calls_sum、failed_calls、terminal_calls、edit_misses/edit_calls、context_peak 和 last_activity_at。所有 sum/count 从 task membership 关联的 episode/input 去重后计算，`episode_id` 是一次 round 的去重键，不能因重放事件重复计数。

趋势只对有合法 `task.closed` 的 task 计算，默认指标为：已关闭 task 数、完成/失败/取消分布、平均/中位 wall duration、平均 round/input/step 数、token 合计/均值、失败率和工具/编辑质量指标。未关闭 task 单独进入“进行中”列表，展示已累积 round/token/last activity 等非终态指标，但不进入已完成趋势分母。

任务下钻保留每个 task 的 input、episode/round、session 分段、provider/model、工具计数、context bill 和错误；因此任务级汇总不会吞掉 D-655 关心的轮次真源。session 视图只作为“这个 task 在哪些会话中发生、同一 session 还承载了哪些 task”的上下文，不再作为主趋势分组。

### 5. 前后端契约与真实消费者

后端新增版本化的 task projection 查询（推荐 `run_metrics_by_task`，或对 `run_metrics` 提供明确 response version），返回 `completed_tasks`、`in_progress_tasks`、`trend` 和 task 下钻所需的 rounds/sessions；旧 `rounds` 查询在迁移期保留，标明 legacy round 视图，不与新趋势混合。后端负责事件归约、membership、去重、关闭筛选和指标公式。

前端 `13-memory.js` 的 `refreshMetrics` 只请求并渲染后端 task projection：主区显示已关闭 task 趋势与进行中 task，点击 task 读取 session/round 下钻；不复制 task 状态机。`03-shell.js` 的 metrics 入口仍是真实消费者，incident metrics 保持独立区域。后续实现必须让真实入口从旧 rounds 切换到 task projection，并补空态、未关闭态和 legacy 提示；只加一个静态表格不算接通。

### 6. 历史兼容、迁移与回滚

- 不改写旧 `episodes`、`session_inputs` 或原始 `session_events`；新增 task 事件/投影表或等价的派生存储，保持 additive migration。
- 历史数据没有 task start/close 时标成 `legacy_unassigned`/legacy round 集合：可在旧轮次视图和 session 下钻中查到，但不伪装成已关闭 task，不进入已完成趋势。
- 新数据先写 task 事实，再由 projection 归约；projection 可从全量事件重建，并以 task 数、membership 数、episode 覆盖数和 legacy 数做迁移对账。
- 回滚只停止 task projection 消费并恢复旧 rounds 查询，不删除新增事件；若 schema migration 失败，按 SQLite 迁移规范回滚新增表/索引，原有 episodes 与 session events 保持可读。具体 schema、版本号、备份和回滚脚本另立迁移条目。

### 7. 长 session 多任务可复核演算

真实长 session 样本是 `ses_project_c0b8d633186c2464`（B1 实测 1072 episodes、1071 run_id/input_id）。从该库最近四条真实 episode 行取值，构造“事件归约演算向量”：这不是回填生产库，也不声称历史已有 close，而是用真实 episode 作为输入，叠加两组待实现的显式 membership/close 事实验证聚合公式。

| task close 组 | 关联真实 episode | round_count | steps_sum | input_tokens_sum | output_tokens_sum | episode 时间跨度 |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| `task-demo-a`（演算 close） | 1156, 1157 | 2 | 64 | 942136 | 21219 | 445321 ms |
| `task-demo-b`（演算 close） | 1154, 1155 | 2 | 64 | 771947 | 35934 | 528678 ms |

复核方式：对每组仅允许 `task.closed` 的 task 行进入 completed_tasks；两组各两条 episode 仍能在 task 下钻看到，四条 round 不会被计为四个完成任务；若去掉 close 事件，同样的 episode 只能落到 in_progress/legacy，completed trend 样本数应为 0。该演算同时明确了当前缺口：真实库没有这些 task 事件，所以当前 UI 不能合法展示上述两行。

B2 设计结论：采用“显式 task 事实 + 可重建 projection + task 主趋势 + session/round 下钻 + legacy 不回填”的方案作为实现输入；task start 入口、关闭权限/outcome、跨 session attach 和新增 schema 需在实现前由用户评审确认。

## 技术选型与取舍

- 证据真源继续是 append-only 的 session events 与 episodes；画像是可重建 projection，不反向改写原始轮次。
- task 聚合必须以显式 close 事实为完成判据；不能以 `run.completed`、`session.turn_completed` 或 input finished 单独替代用户任务关闭。
- round/episode 保留为 task 下钻的最小事实单元，避免任务级汇总吞掉 provider、模型、工具失败和上下文账单。
- 旧数据采用“未归属/legacy”可见策略比自动回填更保守；回填如需实施，必须另有迁移、回滚和计数对账条目。

## 实施边界与调用方

本条的真实现有调用方是 `03-shell.js` 的 metrics 视图切换与 `13-memory.js` 的 `refreshMetrics`；后端真实入口是 Tauri `run_metrics` 和 `run_metrics_by_category`。本批不修改它们，只记录其当前契约。

后续实现必须同时接通：task 事实生产者、task projection/查询、前端 task 画像入口与 session 下钻、旧 episode 兼容和自动化测试；只增加一个 display-only 表格不算完成。

## B3 后续条目拆分

B3 不实现生产代码，只把实现边界拆成可独立验收的条目：

1. **R-338 任务级运行画像事实链路与可重建投影**：评审 task start/close 入口、权限、outcome、membership 和 projection 真源；接通后端真实查询消费者。
2. **R-339 运行画像历史任务兼容迁移与回滚**：在 R-338 契约后处理 additive schema、legacy 不回填、迁移对账、备份和回滚。
3. **R-340 运行画像任务主视图与 session/round 下钻**：在 R-338 projection 可消费后改真实 metrics UI；完成 task 主趋势、进行中单列、legacy/空态和下钻。
4. **R-341 运行画像任务级真实链路收口与回归矩阵**：跨 R-338～R-340 验证真实 task start/close→SQLite→API→UI 链路，覆盖长 session 多任务、未关闭、legacy 和失败路径；单测、viewport 模拟和替身服务不能替代链路证据。

依赖关系：R-338 是事实与 API 前置；R-339 依赖其数据契约；R-340 依赖其 projection 查询；R-341 在三者完成后做真实链路收口。task start 入口、关闭权限/outcome、跨 session attach 和新增 schema 仍以用户评审为准，任何后续条目都不得在评审前擅自实现。

## 变更记录

- 2026-08-28：R-337 B1，读取现有运行画像实现、episode 落库链路、UI 消费者，并对真实 `.kanzei/state.db` 做只读统计；确认 task close 事实尚不存在。
- 2026-08-28：R-337 B2，在不修改生产代码的前提下形成任务级画像候选基线：显式 task start/close 事实、可重建 projection、关闭 task 主趋势、进行中单列、session/round 下钻、legacy 不回填；task start 入口、关闭权限/outcome、跨 session attach 和 schema 仍待评审。

## 验证证据

- 代码域 V1：`crates/kanzei-app/src/commands/run.rs:448-464,518-603`、`crates/kanzei-core/src/store/episodes.rs:10-37,80-106`、`crates/kanzei-core/src/store/mod.rs:266-286`、`crates/kanzei-app/src/run/persistence.rs:194-231`、`crates/kanzei-app/ui/13-memory.js:150-275`。
- 本地实测 V2：真实 `.kanzei/state.db`；1157 episodes/11 sessions，最大 session 1072 episodes/1071 run_id/1071 input_id，1310 inputs 状态分布，`session_events` 事件类型和最新 `run.completed` payload 均无 task/close 字段。
- 数据库审计复核：`T-1786922726829`，同一只读命令在继续运行的数据库上得到 1158 episodes/11 sessions、主 session 1073 episodes/1072 run_id/1072 input_id；数字按取样时刻解释，不改变无 task/close 事件的结论。

- 长 session 演算证据：`T-1786922726828`，命令原样读取真实 `.kanzei/state.db` 的 episode 1154–1157 并断言两组结果；输出为 task-demo-a=(2 rounds,64 steps,942136 input,21219 output,445321ms span)、task-demo-b=(2 rounds,64 steps,771947 input,35934 output,528678ms span)。该记录是可复核设计向量，不是历史 task 回填或生产运行事实；只有显式 `task.closed` 才进入 completed trend。
- UI 既有运行时检查：`T-1786922726826`，命令 `node --experimental-vm-modules scripts/ui-runtime-smoke.mjs`；覆盖 27 个 UI 脚本、2326 次 invoke、主视图切换及 0 运行时错误，用于证明当前入口可加载；它不证明 task 画像已实现。

## TODO 与后续风险

- B3：拆出后端 task 事实/投影、历史迁移/回滚、UI task 画像与 session 下钻、自动化测试和真实端到端链路条目。
- B3：把 task start 入口、关闭权限/outcome、跨 session attach 和新增 schema 的待评审点原样写入后续条目；未经确认不进入生产代码。
- 风险：若用 prompt 或时间猜 task，历史趋势会产生不可审计的假精度；若只按 session 聚合，长会话仍会吞没任务边界。
