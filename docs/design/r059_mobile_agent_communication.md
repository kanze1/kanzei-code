# R-059 移动端子代理通信与通知设计

- 身份：validated_design
- 状态：部分交付；R-059 已 dropped，R-270/R-271 已完成，R-288 保留为 Android 真机 E3 验收
- 日期：2026-08-16
- 最近核验提交：f81c2ff（桥接）/49b65e2（PWA）
- 关联需求：R-059 (dropped) R-270 (done) R-271 (done) R-288 (todo)

> 实施前输入（2026-08-08）：原文“进行中”及“待确认”表述已被 §0 的用户定调覆盖；历史反馈、旧决策与协议草案保留供追溯。

## 0. 2026-08-16 定调（用户逐项拍板，覆盖下文与此冲突的"待定"表述）

- **交付形态:PWA + 现成通知桥,不做原生壳。** 手机(Android)用途定调为遥控器:给电脑发消息、看运行状态、批权限。原生壳唯一硬增量是息屏通知(前台服务保活),该增量由现成 LAN 推送桥(KDE Connect 类,纯局域网,不出公网)零开发补齐,不为舒适性引入 Android 工具链。iOS 不在范围。
- **实时通道:SSE**(§3.2 原"HTTP/SSE/WebSocket 待定"就此定案)。断线重连沿用 delivery_cursor 补发;发消息仍走既有 POST。
- **第一批含 approval 远程回答**:脱敏摘要 + 显式确认,最终门禁仍在 runner/harness 侧(§4 约束不变)。
- **公网禁止不变**;LAN 监听 + 每设备独立 token 配对/撤销;不做 TLS(自用威胁模型)。
- **实施载体**:R-269 浏览器工具(开发自检前置)→ R-270 桥接移动化(LAN 配对/SSE/approval/PWA serve/通知桥)→ R-271 移动端 PWA 界面。协议契约沿用本文档 §8。

## 1. 实施前输入快照（2026-08-08；不是当前事实）

- 当前产品只有 Tauri 桌面端，前端依赖 `window.__TAURI__` 的 `invoke`/`listen`。
- 当前没有 Android、iOS、Flutter、React Native、Capacitor 或 Expo 工程。
- 当前提供仅绑定 `127.0.0.1` 的 HTTP 通信入口，使用启动时生成的 bearer token；没有公网监听、SSE 或 WebSocket。
- 子代理只读运行在主 agent 的 task 调度内，现有事件包括 task start/end、子工具轨迹、运行完成和错误；桌面端已提供全局 agent-container manifest、版本升级和 previous 回滚记录，原生移动端仍未接入。
- 因此移动端通过受控通知/消息协议访问，不直接订阅桌面 Tauri 事件；桌面设置页负责显式启动、展示配对 token 和停止撤销。

## 1.1 当前交付边界（截至 R-270/R-271 关闭）

- R-270 已交付 LAN 可切换、默认回环、每设备配对/撤销、SSE cursor 补发、approval 通道、PWA serve 与通知桥；最终权限门禁仍在 runner/harness。
- R-271 已交付 Android Chrome 优先的原生 JS PWA：配对、通知流、发消息、approval、manifest、service worker 与通知列表窗口化。
- R-059 已 dropped；本设计的剩余真实设备边界由 R-288 单独承接：Android 真机在同一 LAN 上完成配对、通知、消息与证据留存。
- 公网监听、TLS、自研推送协议、iOS 专属适配和远程 shell/write 仍不在范围；R-270 的 LAN 与 approval 交付不改变这些安全边界。

## 2. 产品对象与关系

- `agent_profile`：主代理或子代理的身份、模型、能力策略和版本信息。
- `agent_instance`：一次可运行的代理实例，绑定一个 `process_id`/`thread_id`；其隔离边界依赖 R-030/R-050。
- `agent_message`：主代理与子代理之间的业务消息，必须带 `message_id`、发送方/接收方、会话归属、创建时间和幂等键。
- `agent_notification`：面向用户的状态通知，不等同于内部工具轨迹；必须区分 queued、running、succeeded、failed、cancelled、approval_required。
- `mobile_device`：经过认证的移动端设备实例，不以用户账号或项目路径单独作为身份。
- `delivery_cursor`：每个设备/订阅保存的最后确认序号，用于断线补发和重复投递去重。

关系约束：项目、进程、线程和消息的归属沿用 R-030/R-050 契约；桥接服务固定绑定一个项目状态库，通知按 thread_id 和 device_id cursor 隔离。

## 3. 通信模型

### 3.1 主代理与子代理

- 主代理通过受控 message broker 向子代理发送任务、取消、补充上下文和升级请求。
- 子代理只能回传消息、状态、结果摘要和允许的只读轨迹；写入、shell、联网和权限升级必须经过主代理的硬门禁。
- 每条消息使用幂等键，重复投递不得重复执行任务；状态迁移必须由服务端校验，不接受移动端直接改状态。
- 消息 payload 默认不携带完整项目密钥、API key 或未授权文件内容；需要上下文时使用服务端按权限生成的引用或摘要。

### 3.2 移动端订阅

- 移动端只订阅经过授权的 notification stream，不直接订阅桌面窗口事件。
- 事件最少包含：`event_id`、`sequence`、`thread_id`、`agent_id`、`kind`、`status`、`summary`、`created_at`、`requires_action`。
- 实施前输入（已由 §0/R-270 覆盖）：原方案在 2026-08-08 尚未决定 HTTP/SSE/WebSocket；当前实现固定为 SSE，沿用 delivery_cursor 断线补发。
- 断线重连携带 `delivery_cursor`；服务端补发未确认事件，客户端按 `event_id` 去重并按 sequence 排序。
- 非关键通知允许合并或过期；权限询问、失败、停止和升级结果不得静默丢弃。

## 4. 权限、安全与生命周期

- 移动端设备必须经过显式配对、撤销和过期控制；每台设备拥有独立 device id 和密钥材料。
- 项目、线程、代理实例和通知订阅分别校验授权，禁止仅凭项目路径访问事件。
- 移动端默认只能查看状态、发送允许的消息、取消自己有权操作的任务；不能绕过主代理获取写入、shell、联网或权限回答能力。
- `approval_required` 只显示脱敏摘要和可操作范围；最终权限判断仍在 runner/harness，移动端的同意不能替代服务端门禁。
- 代理升级必须是显式版本迁移：保留旧实例可回滚状态，记录迁移前后版本、操作者、时间和失败原因；升级不应隐式改变工具权限。
- 线程关闭、设备撤销、网络断开和服务崩溃都必须有明确状态；断线不能被当作成功或取消。

## 5. 实施前分阶段计划与验收（历史输入；当前交付边界见 §1.1）

### 阶段 A：协议与内存态验证

- 固化消息、通知、cursor、幂等键和错误分类的结构定义。
- 用内存 broker 测试主/子代理双向消息、重复投递去重、顺序和取消隔离。
- 用内存订阅测试断线补发、cursor 确认、重复事件去重和关键通知不丢失。
- 不连接移动端、不打开公网端口、不写项目文件。

### 阶段 B：受控本地服务 POC（已实现）

- 仅本机回环监听，启动生成 bearer token，未授权请求返回 401。
- 支持通知历史 cursor 补发、设备/线程 cursor 持久化和双向 message 事件写入；不开放远程权限回答或 shell/write 门禁。
- 停止服务即撤销当前 token；服务重启生成新 token，旧 token 不再有效。

### 阶段 C：双向交互与升级（桌面协议已实现，原生客户端待后续）

- 已增加消息事件写入和全局 `.kanzei/agent-containers/<agent_id>/manifest.json` 管理容器；升级自动保存 `.previous` 回滚点，权限清单固定为只读，不因升级扩大。
- 原生 Android/iOS 客户端、推送平台适配和受控 approval 流程仍属于后续外部客户端工作。
- 远程写入、worktree、合并和高风险权限必须单独设计并通过安全评审，不与只读通知 POC 混合上线。

## 6. 实施前验收矩阵（历史输入；当前交付见 §1.1）

| 场景 | 通过条件 | 当前阶段 |
|---|---|---|
| 主/子代理双向消息 | 消息带归属、幂等键和顺序，重复投递不重复执行 | 阶段 A 已验证：内存 broker 支持双向读取与 thread 隔离 |
| 通知实时投递 | 运行、完成、失败、停止、权限状态可区分 | 阶段 A 已验证：按 thread 的 sequence 与订阅拉取 |
| 断线恢复 | cursor 补发、event_id 去重、关键通知不丢 | 阶段 B 已验证：SQLite notification + delivery_cursor 跨重建回放 |
| 权限边界 | 移动端不能绕过 runner/harness 门禁 | 阶段 A 设计约束 |
| 设备安全 | 配对、撤销、过期和跨项目授权隔离 | 本机 bearer token；停止服务撤销；公网部署仍禁止 |
| 子代理升级 | 版本迁移可审计、失败可回滚、不隐式扩大权限 | 全局管理容器 manifest + previous 回滚点已实现 |
| R-030/R-050 对齐 | 所有消息/通知/权限/队列带 process/thread 归属 | 已按 session/thread 字段对齐 |

## 7. 实施前 POC 的暂不实现项（历史输入）

- 不新增移动端框架；R-271 采用原生 JS PWA，原生客户端仍属于后续工作。
- 不把 Tauri `kz:*` 事件直接暴露为远程 API。
- 不开放公网端口；R-270 已支持 LAN 可切换，但默认仍为回环并受 token/设备授权保护。
- 远程 approval 已由 R-270 交付且仍受 runner/harness 门禁；远程 shell/write 和公网部署不属于当前范围。

## 8. 阶段 A 字段契约与状态语义

以下示例是逻辑协议，不代表最终传输格式；字段统一使用 `snake_case`。

### 8.1 主代理发送任务消息

```json
{
  "message_id": "msg_01",
  "idempotency_key": "task_01_attempt_01",
  "project_id": "project_01",
  "process_id": "process_01",
  "thread_id": "thread_01",
  "sender_agent_id": "primary_01",
  "receiver_agent_id": "subagent_01",
  "message_kind": "task_requested",
  "payload": {"prompt": "只读检查依赖关系"},
  "created_at": "2026-08-09T00:00:00Z"
}
```

服务端按 `idempotency_key` 保存执行结果：同一 key 的重试返回原结果，不重新创建任务；不同 key 即使 payload 相同也必须显式视为新任务。

### 8.2 通知事件

```json
{
  "event_id": "evt_01",
  "sequence": 42,
  "project_id": "project_01",
  "process_id": "process_01",
  "thread_id": "thread_01",
  "agent_id": "subagent_01",
  "kind": "agent_status_changed",
  "status": "failed",
  "summary": "只读检查超时",
  "requires_action": false,
  "created_at": "2026-08-09T00:01:00Z"
}
```

`sequence` 只在同一订阅归属范围内递增；客户端遇到跳号必须请求 cursor 补发，不能自行填充缺失事件。`event_id` 是去重主键，重复事件只更新接收时间，不重复展示或触发动作。

### 8.3 状态迁移

```text
queued -> running -> succeeded
                  ├-> failed
                  ├-> cancelled
                  └-> approval_required -> running | cancelled
```

- `queued` 只能由服务端接受任务时产生。
- `running` 只能由实际执行器确认，移动端不能伪造。
- `approval_required` 只允许进入脱敏、待授权状态；超时或设备撤销默认迁移为 `cancelled`，不得自动放行。
- `succeeded`、`failed`、`cancelled` 是终态；重试必须生成新的 `message_id`/`idempotency_key`，并通过 `retry_of` 关联原消息。
- 终态通知必须持久化到可补发流；普通进度通知可按策略合并，但不能覆盖终态。

### 8.4 错误分类

| 错误 | 客户端行为 | 是否自动重试 |
|---|---|---|
| `duplicate_message` | 使用原消息结果 | 否 |
| `cursor_expired` | 重新获取允许范围内的历史快照，再建立新 cursor | 否 |
| `device_revoked` | 清理本地订阅并要求重新配对 | 否 |
| `not_authorized` | 不展示敏感 payload，提示权限不足 | 否 |
| `temporary_unavailable` | 保留本地未确认 cursor，退避重连 | 是，有限次数 |
| `invalid_state_transition` | 展示服务端权威状态，禁止本地强行修改 | 否 |

### 8.6 cursor 重建与过期边界

阶段 A 的 `InMemoryBroker` 不裁剪历史，因此只要 cursor 仍在内存 broker 的序列范围内，重新创建订阅并恢复已持久化的 cursor 即可继续读取，不会重放 cursor 之前的事件。当前订阅的 `seen_event_ids` 是内存态，重建订阅后依靠 cursor 避免已确认事件重复投递；跨进程或进程重启的 event_id 去重仍需持久化 delivery_cursor/去重记录。

生产服务必须在历史窗口被裁剪或 cursor 不属于当前 thread 时返回 `cursor_expired`，由客户端获取授权范围内的历史快照并建立新 cursor；不能静默从最早事件开始，避免重复展示或遗漏终态通知。


- 相同 `idempotency_key` 重复投递只产生一个任务和一个终态。
- 不同 `thread_id` 的同名消息互不覆盖，事件 sequence 和 cursor 各自隔离。
- 客户端断线后从 cursor 补齐缺失事件，重复事件不会重复通知。
- 终态事件在普通进度事件合并后仍可补发且顺序正确。
- `approval_required` 在设备撤销、超时和取消后不能自动迁移为 running。
- 未授权项目、线程或设备不能读取消息 payload，只能收到明确的错误分类。
