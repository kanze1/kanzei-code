# 子代理管理体系扩展方案（R-058）

日期：2026-08-08
状态：方案已验证，按低风险顺序实施

> **文档状态(2026-08-08 整理):历史方案。** 对应 R-058 已 done;方案 P1(可观测性)部分已落地,策略层与目录 UI 未实施,相关演进见 R-117(子代理运行状态可观察性)与 `memory_system.md` 的 memory-manager 设计。

## 目标与边界

当前 kanzei 已有两类“管理”能力：

- 需求、缺陷、目标是持久化的项目工单；
- `task` 子代理是运行中的只读探索单元，活动面板可查看实时和历史轨迹。

子代理扩展不应把所有东西都塞进需求列表，也不应让子代理获得主 agent 的写入权限。本方案把管理对象拆成四层：

1. **角色层**：agent 定义、profile、模型、steps、人格提示词；
2. **任务层**：一次 task 的 prompt、模型档位、状态、耗时、子工具轨迹和结果；
3. **策略层**：并发上限、超时、只读工具白名单、失败重试和预算；
4. **审计层**：权限询问、模型用量、失败原因、持久化运行轨迹。

R-030 多进程和 R-050 并行线程的隔离设计优先级高于本方案的 UI 扩展；本方案不通过前端 Tab 模拟进程或线程。

## 当前能力盘点

| 对象 | 当前入口 | 已有字段/能力 | 缺口 |
|---|---|---|---|
| agent 角色 | `~/.kanzei/agents/*.md`、项目 `.kanzei/agents/*.md` | profile/model/mode/steps/system，项目覆盖全局 | 没有目录化查看、校验和启停入口 |
| task 任务 | 右侧活动面板 | start/end、耗时、ok、preview、display、子工具 trace | 没有模型/预算/超时状态的统一摘要 |
| 子代理权限 | `SubagentBase` | read/glob/grep 独立快照，权限 allow，ask 固定 deny | 构造层之外还缺运行时白名单复核 |
| 运行轨迹 | `run.trace` | task progress 可回放 | 普通工具 start/end、token、失败原因未全部归档 |
| 模型策略 | `task` 的 fast/primary | fast 缺失回退 primary | 没有项目级默认策略和预算展示 |

## 推荐产品结构

### 1. Agent 目录（设置/工作区入口）

展示全局与项目 agent，字段：

- 名称、来源（内置/全局/项目）；
- profile、mode、模型角色、steps；
- 状态：可用/配置错误/被当前 profile 隐藏；
- 系统提示词只读预览；
- “打开原文”入口，不在 UI 内直接编辑提示词。

硬边界：显式选择 agent 时必须校验 profile；主循环不能选择 `Subagent` mode。配置错误应可见，不得静默扩大权限。

### 2. Task 任务面板

活动面板现有条目继续作为唯一实时入口，补充统一摘要：

- `task_id`、父运行 ID；
- agent/model（fast 或 primary）；
- queued/running/succeeded/failed/timeout/cancelled；
- 开始时间、耗时、token 预算；
- 子工具数量与只读工具轨迹；
- 失败原因和可复制结果。

历史回放复用现有 `run.trace`，不另建前端私有数据库。

### 3. 策略面板

先提供只读诊断，再提供可配置项：

- 每轮 task 数量上限；
- 全局并发数；
- 单 task 超时；
- fast/primary 选择；
- 失败重试次数；
- token/时间预算。

策略的强制执行必须在 runner/harness，UI 只负责编辑并显示当前生效值。默认值应保持现有行为，配置读取使用 serde default。

### 4. 审计摘要

在运行结束卡片中提供：

- 主 agent 与子代理数量；
- 各模型调用次数与 token；
- 权限询问/拒绝数量；
- 子代理失败和超时列表；
- 运行轨迹入口。

审计信息属于运行记录，不写入需求/缺陷列表，避免污染 backlog。

## 硬门禁与实现顺序

### P0：先修契约再扩 UI

1. 注册工具时校验 registry key 与 `Tool::name()`；
2. 空 `resources()` 默认进入 `*` 权限评估；
3. 子代理 runtime 构造时再次过滤 read/glob/grep；
4. 最后一步 runner 硬拦截 ToolCall；
5. 限制每轮 task 数量和全局并发预算。

每项都要先补单元测试，并登记对应 defect 后再修复。

### P1：增强可观测性

1. `TaskTrace` 增加 model、started_at、finished_at、outcome、timeout；
2. `run.trace` 追加完整工具生命周期和 token 摘要；
3. 活动面板增加任务过滤、失败筛选、复制结果；
4. 历史回放保持向后兼容，缺字段显示“未知”。

### P2：策略与目录 UI

1. 新增只读 Agent 目录；
2. 新增策略设置，保存到配置并由 runner 强制；
3. 新增运行审计摘要；
4. 与 R-030/R-050 的进程/线程 ID 合并，所有 task、权限和队列字段必须带归属 ID。

## 可用性验证

使用三条日常路径验证方案不引入额外打断：

1. **快速探索**：主 agent 发起多个 task，活动面板能区分每个子代理、显示完成/失败和只读轨迹；主对话不被工具块淹没。
2. **失败诊断**：一个 task 超时或失败，用户能看到失败原因、耗时和结果，不需要翻终端；主 agent 仍可继续处理其他 task。
3. **权限边界**：子代理尝试写入、bash 或联网时，构造层和 runner 层都拒绝；主 agent 的权限询问队列不被子代理串入。

当前项目已经具备活动面板、task 子轨迹回放和独立只读 snapshot，因此 P1 的一部分已可用；本方案将后续改动限制为“补字段/补硬门禁/补诊断”，不重复建设任务系统。

## 验收记录

- 已完成代码盘点：agent 注册、Markdown 覆盖、SubagentBase、runner task 并发、Tauri 事件、前端活动面板和历史 trace 回放。
- 已完成风险交叉检查：与 R-049 静态 harness 报告中的 H-001/H-002/H-003、M-003/M-005/M-006 对齐。
- `cargo test -p kanzei-harness -p kanzei-core -p kanzei-tools` 通过，现有只读子代理测试通过。
- 后续实现顺序已明确：先硬门禁与回归测试，再扩展任务/agent/策略 UI。
