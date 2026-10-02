# 会话运行态、并行线路与任务设置统一设计

- 状态：已确认，作为 R-197 的实现基线
- 日期：2026-08-12
- 关联：R-030 R-086 R-169 R-178 R-197 D-209 D-271 D-281 D-283

## 1. 目标

桌面端的运行状态、线路状态、停止控制、鞭挞设置、活动轨迹和历史恢复必须围绕同一个 `session_id` 工作。事件是运行中的实时输入，后端会话运行时是真实运行态，数据库事件是重启和切线后的恢复源，轮询只负责补偿事件丢失和最终校准。

本设计解决以下一组互相耦合的问题：

1. 实际运行时线路按钮显示空闲，停止按钮消失。
2. `kz:done` 轮末事件把整个会话误显示成空闲。
3. 主线和并行线的 profile、鞭挞开关、定时器互相串线。
4. 切换线路后活动记录暂时消失，运行中的轨迹只能在轮末恢复。
5. 历史对话、活动轨迹和任务设置的归属口径不一致。

## 2. 状态真源与显示状态

### 2.1 后端真源

`SessionRuntime` 是当前进程内的运行真源：

- `running`：会话运行循环是否仍存活；
- `stage`：最近一次真实阶段；
- `current_run`：当前运行任务句柄；
- `live`：当前运行的增量轨迹和统计；
- `auto_run`：该会话的自主推进控制器。

`ProcessInfo` 和 `CollaborationLine` 只读上述状态并返回快照，不自行推导运行状态。

### 2.2 会话状态

前端统一维护以下状态：

```text
idle -> starting -> running -> round_finished
                         \-> failed
running/round_finished -> stopping -> idle
round_finished -> auto_pending -> starting
```

- `starting`：已提交 `run_prompt`，尚未收到首个进度事件；
- `running`：收到本轮进度，或后端 `runtime.running=true`；
- `round_finished`：收到 `kz:done`，但尚未收到会话级终态；
- `auto_pending`：本轮结束后等待鞭挞定时器；
- `stopping`：已发停止请求，等待 `kz:stopped` 或 `kz:idle`；
- `idle`：会话级运行循环结束；
- `failed`：本轮失败且运行循环结束。

### 2.3 事件边界

- 进度事件：`kz:turn`、`kz:meta`、`kz:status`、`kz:text`、`kz:reasoning`、`kz:tool-start`、`kz:tool-progress`、`kz:task-progress`、`kz:step`。带有 `session_id` 时立即把该会话投影为运行中；
- 轮末事件：`kz:done`。只结束当前轮，不收回会话运行态；
- 会话终态：`kz:idle`、`kz:stopped`。只有这两类事件允许把会话收敛为空闲；
- `kz:error`：记录本轮失败；后端必须在 payload 中明确 `terminal: true/false`。持久化告警、停止清理告警等 `terminal: false` 不能收回仍在运行的会话；只有终态错误(`terminal: true`)或随后到达的 `kz:idle` 才收敛为空闲。

任何单独的 `set_status(..., false)` 都不能覆盖统一会话状态。`auto_pending` 必须显示为“等待下一轮”，并提供取消鞭挞语义；它不伪装成后端仍在执行。

## 3. 设置作用域

| 设置 | 真正作用域 | 切线规则 |
|---|---|---|
| `model` | 线路 | 读取目标线路快照 |
| `profile` / `dev_auto` | 线路 | 没有目标线路设置时使用 `dev_pair` |
| `reasoning` | 线路 | 不继承上一条线路 |
| `phase_pipeline` | 线路 | 读取目标线路后端值 |
| `tracker_writes` | 线路 | 读取目标线路后端值 |
| 鞭挞 enabled/paused/stop_after_round/max_rounds | 会话/线路绑定的 session | 切线前保存旧线，切线后恢复目标线 |
| `auto_allow` | 当前桌面会话 | 自主推进必须明确显示其实际策略 |
| `delivery` | 项目级输入偏好 | 不影响运行态和 profile |
| work priority | 项目记忆 | 只作为后端轮末判定输入 |

切线事务固定为：

```text
保存旧线路 UI 设置
→ 取消旧线路自动定时器
→ 切换 active_process_id / active_session_id
→ 应用目标线路设置（无设置使用安全默认）
→ 同步目标 session 的 auto_run
→ 恢复目标对话、轨迹和待处理询问
```

## 4. 活动轨迹与历史

- 实时事件继续直接渲染到当前线路的活动面板；
- 后端按工具调用、阶段和轮次增量写入 `run.trace`，不再只在完整轮次收尾时写一整包；
- `conversation.updated` 仍保存对话快照，但恢复接口必须同时返回对应线路的 trace；
- 切线/重载时先清理当前线路 UI，再按目标 `session_id` 回放已持久化轨迹；
- 轮内未完成工具必须显示为“运行中/无结果”，不能被错误当作已完成或静默丢弃；
- 活动面板、历史对话和右侧并行线路页面不能跨 `session_id` 读取数据。

## 5. 轮询原则

轮询只做三件事：

1. 启动或事件通道恢复后的快照校准；
2. 事件丢失时从 `process_list` / `collaboration_snapshot` 修复；
3. 并行线路页面的文件和 token 等非事件字段刷新。

轮询不得把已经收到的实时进度覆盖为空闲，也不得替代运行态事件。发送后的 `starting` 窗口由本地启动意图保护，直到首个实时事件或后端快照确认；终态收敛后的旧快照也不能把状态翻回运行。运行中的 UI 延迟目标是事件到达后的一个渲染帧；轮询延迟不作为运行态正确性的依据。

## 6. 停止语义

- 当前会话处于 `starting`、`running`、`round_finished` 且后端仍可停止时显示“停止”；
- `auto_pending` 显示“停止鞭挞”，只取消该线路的自动定时器并关闭会话级鞭挞，不误发全局停止；
- `stopping` 禁止重复提交停止请求，但保留明确的处理中状态；
- `idle`、`failed` 隐藏停止按钮；
- 后端 `stop_run` 必须按目标 `process_id` 计算唯一 session，不能回退为项目内任意运行时。

## 7. 不变量

1. 一个 `session_id` 只有一个后端运行态和一个前端状态投影。
2. `kz:done` 不等于 `idle`。
3. 任意线路的设置不会改变另一条线路的控件或 auto timer。
4. 事件、快照、数据库回放都必须携带并使用同一 `session_id`。
5. 轮询不能覆盖已确认的实时状态。
6. 活动轨迹至少在运行中、停止后、重载后各有可恢复路径。
7. 停止必须只影响用户指定的线路；
8. “顶部状态栏、左侧线路按钮、停止按钮”只能由同一投影函数驱动。
