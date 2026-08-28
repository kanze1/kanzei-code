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

结果：

- 数据库大小约 171,495,424 bytes；`episodes` 共 1157 条，覆盖 11 个 distinct session。
- 最大的真实 session `ses_project_c0b8d633186c2464` 有 1072 条 episode、1071 个 distinct `run_id`、1071 个 distinct `input_id`，时间范围为 `1786205959162..1787896197978`。
- 同一 session 有 1310 条 `session_inputs`：completed 1084、cancelled 162、failed 63、running 1；其中 3 条输入的 `finished_at` 为空。episode 与 input join 后没有孤儿 episode，但 episode 只覆盖 1071 个不同 input_id，不能代表全部输入状态。
- `session_events` 的高频完成事件是 `run.completed` 1014 条、`session.turn_completed` 21 条；事件类型中没有 `task*` 或 `close*`。
- 最新 `run.completed` payload 的键是 `context`、`halted_by_user`、`input`、`output`、`steps`，没有 `task_id`、task 状态或 task close 时间。
- 主 session 最近 episode 的每条都有独立 `run_id`/`input_id`，但 UI 仍将它们命名和渲染为 rounds；当前 `limit: 20` 会把长期 session 压成最近 20 轮窗口。

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

B1 只形成现状基线。B2 需要在方案 B 的边界内补齐 task 对象、事件契约、投影字段、跨 session 关系、历史兼容和迁移/回滚方案；在这些语义被评审前不写生产代码。

## 技术选型与取舍

- 证据真源继续是 append-only 的 session events 与 episodes；画像是可重建 projection，不反向改写原始轮次。
- task 聚合必须以显式 close 事实为完成判据；不能以 `run.completed`、`session.turn_completed` 或 input finished 单独替代用户任务关闭。
- round/episode 保留为 task 下钻的最小事实单元，避免任务级汇总吞掉 provider、模型、工具失败和上下文账单。
- 旧数据采用“未归属/legacy”可见策略比自动回填更保守；回填如需实施，必须另有迁移、回滚和计数对账条目。

## 实施边界与调用方

本条的真实现有调用方是 `03-shell.js` 的 metrics 视图切换与 `13-memory.js` 的 `refreshMetrics`；后端真实入口是 Tauri `run_metrics` 和 `run_metrics_by_category`。本批不修改它们，只记录其当前契约。

后续实现必须同时接通：task 事实生产者、task projection/查询、前端 task 画像入口与 session 下钻、旧 episode 兼容和自动化测试；只增加一个 display-only 表格不算完成。

## 变更记录

- 2026-08-28：R-337 B1，读取现有运行画像实现、episode 落库链路、UI 消费者，并对真实 `.kanzei/state.db` 做只读统计；确认 task close 事实尚不存在。

## 验证证据

- 代码域 V1：`crates/kanzei-app/src/commands/run.rs:448-464,518-603`、`crates/kanzei-core/src/store/episodes.rs:10-37,80-106`、`crates/kanzei-core/src/store/mod.rs:266-286`、`crates/kanzei-app/src/run/persistence.rs:194-231`、`crates/kanzei-app/ui/13-memory.js:150-275`。
- 本地实测 V2：真实 `.kanzei/state.db`；1157 episodes/11 sessions，最大 session 1072 episodes/1071 run_id/1071 input_id，1310 inputs 状态分布，`session_events` 事件类型和最新 `run.completed` payload 均无 task/close 字段。
- 自动化既有证据：`T-1786922726825` 的 `cargo test -p kanzei-app` 为 249 passed；该记录验证既有 crate 行为，不被误写成 task 画像已实现。

## TODO 与后续风险

- B2：确认 task_id 的生产者、task start 与 close 的权限/入口、close outcome、跨 session 续接和未关闭任务的展示口径。
- B2：给出 task projection 的字段、指标公式、session 下钻关系和去重规则；明确一个 task 多轮、多 input、失败/重试/鞭挞的归属。
- B3：拆分后续实现、历史迁移/回滚、UI 与端到端测试条目；保持本条不改生产代码。
- 风险：若用 prompt 或时间猜 task，历史趋势会产生不可审计的假精度；若只按 session 聚合，长会话仍会吞没任务边界。
