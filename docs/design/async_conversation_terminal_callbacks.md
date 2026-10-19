# 异步对话与终端回调

2026-10-07。面向 Kanzei 桌面会话；研究依据是 Claude Code 的官方行为文档和公开 Agent SDK 类型。Claude Code 内部执行循环没有在这些来源中公开，因此下文把公开契约和 Kanzei 的实现分开描述。

## Claude Code 的公开契约

- [交互模式](https://code.claude.com/docs/en/interactive-mode#background-bash-commands)：后台 shell 立即返回任务 ID，输出可回读，当前对话可以接收新提示。Ctrl+B 可以将运行中的 shell 移到后台。
- [消息队列](https://code.claude.com/docs/en/interactive-mode#queue-messages-while-claude-works)：普通消息在完整工具组结束后进入模型；Ctrl+Enter 的即时发送可以把可移交的工作放到后台。不能移交的工作有不同的中断行为。
- [后台子代理](https://code.claude.com/docs/en/sub-agents#run-subagents-in-foreground-or-background)：主会话可以继续运行，完成结果通过后续通知交付。子代理自己的后台终端结束时，通知应送给该子代理。
- [公开 SDK 类型](https://github.com/anthropics/claude-agent-sdk-python/blob/main/src/claude_agent_sdk/types.py)：`TaskNotificationMessage` 带有任务、会话、状态、输出文件、摘要及可选工具调用身份。部分终态仅通过 `TaskUpdatedMessage` 发布；终态显示不能只依赖一类完成通知。
- [TypeScript SDK](https://code.claude.com/docs/en/agent-sdk/typescript#sdktasknotificationmessage)：`origin.kind=task-notification` 区分自动通知和人类输入；通知正文会说明没有新的人类授权。
- [异步 hooks](https://code.claude.com/docs/en/hooks#run-hooks-in-the-background)：普通 async hook 在下次对话交付结果；空闲时主动唤醒另有 `asyncRewake` 语义。它和后台 shell 的结果交付不能混为同一个开关。

可以据此推导出需要会话级任务注册、输入排队和结果交付，但不能据此断言 Claude 内部采用哪种 Rust/JavaScript 事件循环、队列或数据库。

## Kanzei 的所有权与流转

```mermaid
sequenceDiagram
    participant U as 用户
    participant S as 会话调度器
    participant R as Core 循环
    participant P as 终端进程
    participant M as 异步邮箱
    U->>S: Ctrl+Enter 插话
    S->>S: SQLite 准入，绑定 session_id
    S->>M: 请求移交前台终端
    M->>P: 原进程及输出流交给进程注册表
    P-->>R: 返回后台进程句柄
    R->>R: 提交完整工具结果，读取插话
    R-->>U: 处理新输入
    P-->>M: 退出码及最终输出
    M->>S: 幂等保存回调，调度原会话
    S->>R: 在运行边界读取，或空闲时开下一轮
```

1. `session_id` 是对话所有权；项目根只定位资料与状态库，`process_id` 标识工作区会话或终端。当前 UI 显示哪条对话不参与回调收件人的计算。
2. 复用 `AsyncMailbox`、`session_inputs` 和原有 `schedule_run`。运行中的输入在完整工具/结果批次之间消费；尚未完成的模型请求可取消并回到该边界，丢弃未提交正文、思考与工具调用。空闲会话由相同调度器继续。
3. `AsyncMailbox` 分开发布人类输入与自动结果通知。`InputRequests` 通过输入序号和成功读取水位让迟订阅的工具仍能看到待处理输入，失败事务不确认，新工具不重放已消费输入。自动通知可请求工具移交，但不打断正在为用户生成的回答。
4. 前台 shell 移交包含原 Child、stdout/stderr、已捕获输出、原执行截止时间和 owner。没有重跑命令或重新执行副作用。
5. 完成通知先等待输出收集，再发布稳定的 `terminal:<id>` 回调。桌面入口以 owner 和回调 ID 保存输入，避免重复启动。回调是工具结果，不构成新的人类指令或授权。
6. 明确停止/关闭会话会关闭邮箱并推进生命周期代数；旧回调不能重新启动该会话。普通轮次结束不关闭邮箱，因此后台命令可以跨轮完成。
7. 可写子代理沿用项目的独立工作树。是否需要初始化应用自有的无项目资料仓库，由实际存储根身份决定；结对模式的 `project_workflow=false` 只关闭项目交付流程，不把普通项目变成无项目存储。
8. `task wait` 在本地子任务均连接完成回调时返回 `state=waiting, delivery=automatic` 回执，`pending` 列出仍在运行的任务，`jobs` 只包含已完成且已释放执行所有权的结果。主模型没有独立工作时应结束本轮，等待完成或失败回调唤醒；回执和空 `collect` 均不代表任务完成。显式 `background=false` 默认等待实际结果；新的人类插话到达时，在生命周期锁内为原本地执行者启用完成回调并让出前台。没有回调的环境或外部执行者继续等待实际结果。
9. 任务结果摘要的 `result_available` 明确区分运行状态与终态；`get` 读取运行中的任务时说明尚未完成。通知发布与模型结果确认通过同一把通知锁串行化，发布前重新核对已通知和已读取的版本，已读取结果的旧快照不能再次唤醒父会话。UI 从任务状态解析等待、运行、完成、失败与空收集结果，不再仅显示 JSON 字节数。

## 行为边界

| 操作 | 行为 |
|---|---|
| 新用户未设置交付偏好 | 默认插入；保留已保存的排队偏好 |
| Enter，交付方式为排队 | 保留原行为，本轮结束后执行 |
| Enter，交付方式为插入 | 保存插话；未完成模型响应让出前台；可移交的终端或本地子代理继续在后台执行 |
| Ctrl+Enter / Cmd+Enter | 此次发送使用插话，不改用户的排队选择 |
| 运行中发送普通消息，已有工作选择 | 消息只进入当前对话；选择保留给后续执行，不绑定执行批次 |
| 空闲时发送且已有工作选择，或显式开始工作 | 复核工作归属，沿用执行批次与排队规则 |
| `bash background=true` | 立即返回句柄；支持邮箱的会话会自动接收完成结果 |
| 前台命令移交后到达原超时 | 终止原进程树，完成回调带 `timed_out: true` |
| 自动记录测试的命令 | 保持等待真实退出的契约，不因插话提前完成测试记录 |
| 托管项目中工作目录不符合后台边界 | 保持前台执行，不放宽后台围栏 |
| 正在生成模型响应 | 取消未提交响应，按所属 pane 清理全部临时段落；新消息进入同一会话，不触发任务停止 |
| 正在执行其它工具 | 等完整工具/结果批次提交后交付，不抛弃已执行的工具副作用 |
| 切换显示的会话 | 不改变后台任务及完成通知的 owner |

Kanzei 的一次性 `kz run` 没有在本次改成常驻交互终端；现有 CLI 无邮箱场景继续通过 `process` 读取结果。也没有引入 Claude 的云端会话管理、完整 PTY 或 hook 配置系统。后台移交使用已有管道和进程注册表。

## 验证入口

- `cargo test -p kanzei-harness async_mailbox`：迟订阅、单次观察、失败事务不确认与已消费输入不重放；自动通知不打断模型。
- `cargo test -p kanzei-core runner::drive::execution_tests::input`：请求建立、流式正文/思考/未执行工具、限流等待阶段及时响应新输入。
- `cargo test -p kanzei-tools foreground_handoff`：真实 PowerShell 进程；不重跑、保留前后输出、单次完成回调、原截止时间有效。
- `cargo test -p kanzei-tools background::`：进程树停止、围栏归因、输出收集及后台生命周期回归。
- `cargo test -p kanzei-tools team::tests::`：异步 wait 立即交还控制、单次真实结果通知、外部执行所有权、显式前台等待、已读结果拒绝旧通知，以及原有子任务生命周期回归。
- `node scripts/ui-tool-summary-contract-smoke.mjs`：任务等待回执、运行/完成/失败状态、空结果与等待操作标签的浏览器显示。
- `node scripts/ui-async-input-smoke.mjs`：真实 Edge 键盘发送、运行中的工作选择隔离、默认与显式交付方式、草稿清理和跨会话渲染归属。
- `cargo test -p kanzei-app async_mailbox`：停止与正在发布的回调竞态、幂等准入。
- `cargo test -p kanzei-tools writer_background_isolated_result_then_explicit_adoption`：项目流程与结对模式都能完成真实子代理写入、独立工作树和显式采纳。
- `node scripts/ui-async-agents-native-smoke.mjs <fresh kzapp.exe>`：真实 WebView2、Rust 调度器、SQLite、Git、终端和本地确定性模型。覆盖后台子代理、插话、异步问答、Ctrl+Enter 前台移交、OS PID 连续性、切换会话后的结果交付和停止。

原生验收使用隔离项目与应用目录，保存执行文件 SHA-256、模型请求和检查结果。它验证执行机制，不衡量外部模型自主选择后台任务的成功率。

## 2026-10-07 验证记录

2026-10-07，新构建通过 25 项原生检查，产生 29 次本地模型请求，无浏览器或运行时错误。不可变验收记录为 `output/playwright/async-agents/1791379068068/acceptance.json`；执行文件 SHA-256 为 `9bc536295d4f9d050e1924a308d640b80650a011122c8f435f000215b213e0f1`。

- 旧发布文件在相同前台终端场景中，Ctrl+Enter 仍排队，直到原命令 30 秒超时并被终止；原始记录为 `output/playwright/async-agents/1791377692871/failure.json`。新构建在释放测试终端之前已处理插话；移交前后的 OS PID 相同，完成回调恰好一次，含前后两段输出。
- 主会话和子代理的后台终端结果都回到原接收者；切换显示的会话不改变收件人。异步问题的回答使用原历史恢复同一个子代理，明确停止及关闭主会话的检查也通过。
- 结对项目的可写子代理回归测试先复现 `Not a general conversation store`，修复存储身份判断后，结对与项目流程两种模式的真实写入、独立工作树和显式采纳均通过。
- 定向 Rust 测试、三个相关 crate 的 all-targets Clippy、UI lint、完整 `ui-runtime-smoke.mjs` 和 release 构建通过。后台测试中的既有忽略项仍为忽略，没有作为通过证据。

以上保留 2026-10-07 的本地实现与验收记录。异步功能随后已随 `build-46e2e63a` 发布；后续可靠性与 Harness 修复已随 `build-b97f8a2e` 发布，并于 2026-10-17 合入 `dev`。本次仓库整理未重跑上述原生验收，历史记录不作为当前提交的全量验证证据。

## 2026-10-20 修复与验证（未发行）

运行中普通提问与待选工作解耦；新偏好默认插入，显式排队选择保留。人类插话能在请求建立、流式响应和限流等待时取消未提交响应，继续原会话；可移交终端与本地前台子代理保留原执行实例并回传真实结果。自动完成通知不会反复打断用户回答。前端按 pane 清理未提交正文与全部思考段落，连续模型步骤在复用渲染槽前先完成上一轮渲染。

验证基于当前未提交工作区与本地 debug 构建，未打包发行、安装或替换运行中的用户版本。

| 验证 | 结果 |
|---|---|
| Harness 邮箱定向测试 | 3 通过：迟订阅、成功读取水位、事务失败不确认、自动通知隔离 |
| Core 执行边界测试 | 12 通过，其中 3 项新增测试覆盖响应头、部分流及限流等待 |
| Tools 前台移交测试 | 5 通过，覆盖原进程输出、截止时间、迟订阅和子代理原实例完成 |
| 子代理生命周期测试 | 44 通过，2 项子进程辅助入口保持忽略 |
| App 插话持久化与邮箱测试 | 6 + 1 通过，含 FIFO、事务回滚、停止竞态 |
| 四个相关 crate 的 all-targets Clippy | 通过，`-D warnings` |
| Edge 异步输入验收 | 41 项通过，`output/playwright/async-input/acceptance.json` |
| 原生桌面标准验收入口 | 34 项通过、35 次本地模型请求、0 浏览器/运行时错误 |
| 原有对话呈现回归 | 代码块及剪贴板 115 项、Markdown 16 项、任务摘要 31 项通过 |
| 前端其它检查 | 135 个模块的完整运行时冒烟、lint、并行线路、i18n、a11y 与 verification registry 检查通过 |

原生不可变记录：`output/evidence/ui-async-agents-native-smoke/1792440253384-6323f266-cfd3-40bf-9dcc-bc0f2fe82f14/acceptance.json`。标准入口的 `run.json` 为 `state=success, cleanup_state=removed`。被测 debug 文件 SHA-256 为 `f2e6bf604d8c382b9b0a76ead2e250291a9eacfc6f027afc4b713824d74ddab7`；记录中的 commit `137faa31e129255b1049ebd14cd238874b843869` 是构建基线，修复仍在工作区，不能把它当成已提交版本的全量证据。

测试使用隔离项目、真实 WebView2/Rust/SQLite/Git/PowerShell 和本地确定性模型；不代表已验证外部模型的自主调度选择或生产端网络表现。运行中的附件、自动记录测试的同步退出契约、托管目录后台围栏维持原有边界。
