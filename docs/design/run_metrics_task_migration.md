# 运行画像任务历史兼容、对账与回滚

- 状态：实施基线
- 日期：2026-09-01
- 关联需求：R-339
- 前置实现：R-338（`288bb57a`）

## 1. 范围与不变量

R-338 已把新的 `task.started`、`task.membership_added`、`task.closed` 事实追加到现有 `session_events`，并从事实即时重建 task projection。本条负责历史兼容边界、可核对的覆盖统计和失败恢复说明，不重新定义 task 生命周期。

必须保持以下不变量：

1. `episodes`、`session_inputs` 和原始 `session_events` 只读保留，历史行不因 task 画像上线而改写。
2. 没有显式 task membership/close 的历史 episode 和 input 分类为 `legacy_unassigned`；它们可以继续由旧 rounds API 读取，但绝不进入 `completed_tasks` 或 completed trend。
3. task projection 是从 session events 与 episodes 重建的派生结果，不写入缓存表，不把 prompt、时间间隔、session 边界当作 task 事实。
4. 同一 episode 通过 episode_id 去重，同一 input 通过 input_id 去重；重放 task 事实不会扩大趋势分母。

## 2. Schema 与迁移策略

本次采用 **additive、零 SQLite schema 变更**：

- R-338 复用当前 `schema_meta` v18、既有 `session_events` 表和 `session_events_session_type_sequence` 索引；task 事实是带版本号的 JSON payload，不需要新表、列或索引。
- 因为没有新增 schema 对象，本条不提升 `SCHEMA_VERSION`，也不做历史 task 回填。若后续为了性能增加 task projection 表或索引，必须另开迁移批次，同时提升版本、更新 `SCHEMA_OBJECTS/SCHEMA_COLUMNS`、提供前滚和回滚测试。
- 新库与旧库使用同一套读取路径：旧数据库打开时沿用 `SessionStore::migrate` 的既有幂等迁移；task 查询只在现有表上执行只读归约。

因此这里的“迁移”是**读取契约迁移**，不是把历史轮次转换成虚构的 task 事实。历史记录的分类由 `task_compatibility_audit` 计算，不向数据库写入 `legacy_unassigned` 标记。

## 3. 对账口径

`SessionStore::task_compatibility_audit` 返回以下两组可复核数据：

- task：`task_count`、`membership_count`、`total/assigned/legacy_episode_count`、`total/assigned/legacy_input_count`。
- 事件：`total_session_event_count`、`task_event_count`、`legacy_session_event_count`。

归属规则固定为：

- assigned episode = 真实存在于 `episodes` 的 membership episode_id 去重集合；
- assigned input = `session_inputs` 中被 task start/membership/close 事实引用的 input_id；
- task event = 带 task_id 的三类已知 task 事实；其余历史事件属于 legacy event 计数。

自动化验收要求这些守恒式成立：

```text
总 episodes = assigned episodes + legacy episodes
总 session_inputs = assigned inputs + legacy inputs
总 session_events = task events + legacy events
```

对账是只读查询，不会因发现数量不一致而删除、回填或重写任何事实；缺失的 episode/input 只保留在计数诊断中。

## 4. API 过渡边界与 legacy 展示

过渡期同时保留两个真实调用方：

- 旧 `run_metrics(project_dir, limit)` 继续从当前 session 的 `episodes` 返回 `rounds`，因此旧用户仍能读取 legacy 历史轮次；其 response 形状和排序不变。
- 新 `run_metrics_by_task(project_dir)` 从 `task_metrics` 返回 `completed_tasks`、`in_progress_tasks`、`trend`、`legacy` 和 `audit`。`legacy.classification` 固定为 `legacy_unassigned`，legacy 计数仅作为未归属可见性和对账，不进入 completed trend。

两条 API 不互相覆盖：旧 rounds 是兼容读取视图，新 task projection 是任务级趋势真源。前端只消费新 projection 的 task 字段，不在浏览器复制归约规则；旧 rounds 的保留和新 projection 的并存由 app command 测试验证。

## 5. 备份、失败恢复与回滚

虽然本条不新增 schema，项目已有的 SQLite schema migration 仍必须遵守既有备份边界：

1. `SessionStore::migrate` 在发现旧版本时，先由 `backup_before_upgrade` 使用 `VACUUM INTO` 生成 `state.db.v<N>.bak`，避免 WAL 中未 checkpoint 的内容丢失。
2. 迁移事务失败时，事务回滚，旧 `episodes`、`session_inputs` 和 `session_events` 保持可读；备份保留为恢复点。更高版本数据库由 `UnsupportedSchema` 拒绝打开并给出升级指引，不能让旧程序降级写入。
3. 运行画像查询失败时只报告新 projection 错误，不删除 task 事实；回滚消费方即可恢复旧 `run_metrics` rounds 视图。已经追加的 task 事实不删除，也不借回滚改写历史。
4. 如果未来把本条的只读归约改成物化 projection，必须先追加迁移版本与可回滚备份测试；失败恢复顺序是停止新 projection 消费、恢复旧 rounds API、保留原始 task events，最后才处理派生对象。

现有 `schema.rs` 的“旧版本备份可打开且保留迁移前数据”测试，连同本条 task compatibility 测试，构成备份与历史可读性的回归证据。

## 6. 验收映射

| 验收项 | 实现/证据 |
| --- | --- |
| ① schema、旧数据、备份、失败恢复 | 本文第 2、5 节；`crates/kanzei-core/src/store/session.rs:229-261` 的 `VACUUM INTO`；`crates/kanzei-core/src/store/schema.rs:988-1067` 的备份/高版本拒绝测试 |
| ② 旧事实可读且计数对账 | `crates/kanzei-core/src/store/task.rs` 的 `task_compatibility_audit` 与 `task_compatibility_audit_marks_legacy_without_entering_trend`；守恒断言覆盖 episodes、inputs、session_events |
| ③ legacy 明示且排除 completed trend | `TaskLegacyProjection.classification = legacy_unassigned`、`task_metrics` 分流；同一测试断言 legacy episode 不在 completed task round，`run_metrics_by_task` 输出保留 legacy 计数 |
| ④ 旧 API/新 projection 过渡 | `crates/kanzei-app/src/commands/run.rs` 的 `run_metrics_command_reads_real_episode_projection` 与扩展后的 `run_metrics_by_task_command_reads_real_task_projection`：同一真实 SQLite fixture 同时读取旧 rounds 与新 task projection |

## 7. 后续边界

- 本条不允许从现有 prompt、时间或 session 连续性推断历史 task close。
- R-340 负责 task 主视图与下钻 UI；R-341 负责真实 task start/close 到 SQLite、API、UI 的端到端收口。
- 若未来需要删除旧 rounds API，必须另立兼容窗口和真实调用方迁移验收；R-339 不提前下线它。
