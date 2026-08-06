# M2 SQLite 会话存储

## 范围

R-003 的第一步在 `kanzei-core` 引入项目级 SQLite 存储层，默认数据库路径为：

```text
<project_root>/.kanzei/state.db
```

数据库文件属于运行时状态，不提交到 Git。

## Schema v1

- `schema_meta`：记录 schema 版本；
- `sessions`：会话聚合元数据；
- `session_events`：按 session 独立递增序列的事件日志；
- `session_inputs`：等待提升的 `steer` / `queue` 输入 inbox。

事件 payload 使用 JSON，业务字段使用 `snake_case`。事件日志保存事件边界，不保存每个流式字符增量。

## 迁移

`SessionStore::open` 会在打开数据库时执行幂等迁移。当前版本为 `schema_version = 1`，创建上述表和必要索引。

迁移在单个 SQLite 事务中执行。已存在更高版本时拒绝打开，避免旧程序误写新数据库。

## 回滚

当前 v1 迁移只创建表和索引，没有破坏性删除操作。若迁移失败，SQLite 事务整体回滚；不自动覆盖或删除原数据库。需要回滚到旧程序时，应使用数据库备份恢复，或让旧程序使用新的空项目状态目录，不对现有数据库执行降级写入。

## 调度语义

- `steer` 在安全 provider-turn 边界一次性按 admission 顺序提升；
- `queue` 在 drain 即将空闲时一次提升一条，严格 FIFO；
- 输入 admission 以 `input_id` 幂等；
- 事件序列按 session 递增，事件 ID 在不同 session 间也必须唯一。

当前实现已完成存储层、CLI/桌面端输入 admission、运行生命周期状态事件和单元测试；steer 输入的前端入口、运行中的 queue drain，以及完整消息历史从事件恢复仍是 R-003/R-009 后续工作。
