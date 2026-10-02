# 决策控制台：现有系统接入说明

- 需求：R-379；日期：2026-09-27。
- 状态：决策闭环与冻结快照后台验证已接入源码；用手机阅读器完成真实 CLI / Flutter 实验。未替换安装版。
- 产品方向：[product_operating_model.md](product_operating_model.md)。

## 本批链路

自动模式 `question` → 当前模型填写决定、简短理由与影响 → 原项目事件库持久化 → 继续运行 → 现有项目总览显示决策 → 用户通过或纠正 → 原会话输入队列 → 按明确范围写入现有记忆库 → 记忆图显示决策来源。

没有调用第二个裁决模型，也不机械采用 `default`。首次只给问题时，返回 `DECISION_REQUIRED` 让当前模型补充决定；交互模式继续真实提问。权限规则、自动放行开关和停止令牌沿用原实现。

## 所有者与调用边界

| 能力 | 使用者 / 调用方 | 服务提供者 | 持久事实 / 输出 |
| --- | --- | --- | --- |
| 自主决定 | 当前 agent；CLI / App 共用 runner | `core/runner/drive/question.rs` | `decision.updated`，原问题与模型选择分别保存 |
| 项目、会话、运行身份 | CLI / App 装配入口 | `ToolCtx.session_id` 与现有 project/run/process 字段 | 显式归属；工作树不代替项目根 |
| 决策账本与并发保护 | runner / App review service | `core/store/decisions.rs` | 既有 `session_events`，SQLite schema 仍为 25 |
| 多项目进展 | 用户；现有 workspace 视图 | `projects::workspace_snapshot` | 线路、Work Unit、决定、复核与原线队列；单项目失败单独显示 |
| 可启动交付产物 | 用户；控制台交付与验证视图 | `deliver` file display → `run.trace` → `conversation_trace_get` → `open_delivered_path` | 只持久化交付文件元数据；打开时校验项目根或归属已核实的来源 worktree，旧轨迹不伪造路径 |
| 纠正与返工 | 用户；`decision_review` IPC | `app/decisions.rs` + core store | 复核与原会话输入在同一事务提交，附 `prompt.admitted` |
| 长期偏好 | 用户选择 project / global | 现有 `MemoryStore` | 用户来源的 active preference，保留决策及复核标记 |
| 人工试用通过 | 用户；`work_delivery_accept` IPC | core store | `work.user_reviewed`，绑定 Work Unit 的 source_sequence |
| 冻结版本与后台验证 | agent 的 Bash verification / CLI verify-async | tools verification + 专用 worker | 文件内容指纹、环境、日志、Work Unit 验证事件 |
| 验证取消与状态 | 用户 / agent | workspace_snapshot、verification_cancel、work verification_jobs | 原项目任务记录；取消只终止指定验证任务 |
| 验证完成唤醒 | App 运行时 | `verification_monitor` → Work Unit owner branch → process/session → 稳定 `verification-wake-{job_id}` 输入 → `run_prompt(wake_queued)` | 只对 App 存活期间观测到的有效终态结果唤醒；取消 / superseded 跳过，输入显式标记为系统通知 |
| 决策来源图 | 用户；现有记忆页 | 文件图缓存 + 决策投影 + force-graph | 已入库偏好到已复核决策的 `derived_from` 边 |

跨项目复核始终携带目标项目与决策 ID，不临时切换当前项目来执行写入。旧工具记录仍按旧格式回放。

## 复核语义

- 本次通过：只记录当前决定已复核，不写偏好。
- 纠正 / 仅本次：原会话排入修改要求，不写记忆。
- 纠正 / 项目或全局偏好：先确保纠正和队列落盘，再写对应记忆库。记忆保存失败仍保留返工输入，并显示可重试状态。
- 相同问题、同一运行的重放复用决策；已作出的决定不被工具重放覆盖。复核检查版本号，重复 request_id 不重复入队。
- 用户纠正 / 仅本次：原会话排入修改要求，复核保存后显式触发 `run_prompt(wake_queued)`。原线空闲时在运行锁内提升已有队首输入并启动，不创建空输入；原线繁忙时不重复排队，交给现有消费循环续接。界面不把排队或唤醒请求说成完成。
- 关联 Work Unit 的决定被纠正后，旧交付显示需要重新验证，不能接受旧证据。返工应创建新的工作单元，原记录作为历史保留。
- 机器完成不等待 `work.user_reviewed`。人工复核不改写机器状态，不代替自动验证。

## 已退役的旧行为

1. 普通自动提问统一变成 `QUESTION_PENDING`：由自主决定协议替代。真正无法取得的外部事实单独登记；没有独立可执行工作时才进入等待用户。
2. 自动模式因末尾问号停机：取消此兜底的自动模式权力；交互模式保留。
3. 另造项目管理页面：升级现有 workspace 入口，保留项目添加、切换、重命名和移除。
4. “前端继续 classic scripts”的旧评估：调整为历史身份，当前真实入口为 ESM。

没有删除旧会话、旧决策文档、需求、记忆或其他已有未提交改动。

## 第二批：冻结快照后台验证

入口是既有 Bash 的 `verification: {unit_id, criteria, environment, resource}`；CLI 提供 `work verify-async`。先走原 Bash 权限和命令 guard，再复制 Git 工作树内的实际源文件，包括未提交改动及未忽略的新文件。排除 `.git`、`.kanzei` 与忽略产物；快照不写回开发目录。

快照记录文件路径、内容 SHA256 与可执行位，复制后再次核对原树，测试前后核对快照。当前要求普通 Git 文件，不支持子模块、符号链接和树外依赖；最多 30000 个文件 / 512 MiB。已验证版本与继续变化的开发树分别保存，测试结果不冒充最新源码结果，也不做跨工具链的缓存复用。

- 独立 worker 执行命令，提交即返回；同资源键的构建排队，共享资源的串行不占开发 WIP。
- 待验证的单元不允许人工补证据或提前完成；无依赖单元可领取，下游依赖仍等待机器完成。
- 命令实际覆盖的验收原文进入证据；只有覆盖全部标准才自动完成该单元。人工试用仍是独立事件。
- 状态覆盖排队、运行、通过、失败、超时、取消、中断、源码变化及被新工作替代。一次结果的结论、证据、完成在 SQLite 中原子提交；迟到结果不覆盖新工作。
- Windows worker 用独立 Job 管理子进程，取消或 worker 崩溃时收口子进程。超过 30 秒无心跳且进程租约已释放时，读状态会登记中断，不自行重跑。
- 项目任务元数据保存在原项目 `.kanzei/verification`；快照和日志保存在本机应用数据目录 `kanzei/verification`。控制台交付详情显示版本、环境、日志与取消入口。

`kz run --autonomous` 复用 App 的自主决策协议，仍沿用工具权限；默认 CLI 交互行为不变。只读档位允许 runner 记录选择，不因此开放源文件编辑。

阅读器实验的真实操作记录见 [第二批验证记录](../reports/2026-09-27-reader-verification-lab.md)。实验台账与原阅读器 R-001 分开，原需求保持 2/5。

## 验证与后续

本批覆盖 Rust 持久化与原会话归属、幂等、复核范围、IPC 形状、真实浏览器交互、图谱渲染及减少动效。浏览器回归的 IPC 使用夹具；Rust 测试使用临时 SQLite / 记忆库。它们不等于安装版或真实模型长跑验收。

具体命令、结果和未验证层见 [本批验证记录](../reports/2026-09-27-decision-console-integration.md)。

B5 已加入 App 侧验证完成监视器：对运行期间观测到的有效终态结果，以 job_id 派生稳定通知输入并唤醒 Work Unit 所属线路；取消与 superseded 不触发，监视器启动前已终态且无通知输入的旧任务不回放。控制台交付文件入口已可从新 `run.trace` 直接打开或定位；旧轨迹没有保存路径的不补造链接。调用方审计已确认自主 `question`、项目总览、决策复核、交付文件打开、人工验收和验证唤醒均有生产入口；默认线路也按项目根隔离，`process_list` 只返回该项目线路，默认 ID 为 `d|<normalized root>`。当前实际运行版仍为 build-d6796bdd，早于 B3-B5；本轮不声称新桌面版或真实模型长跑已验收。真实桌面长跑与真实模型偏好采用属于额外运行时证据，不在 R-379 的登记验收条款中，不阻止源码验收；后续若执行，必须用包含 B3-B5 的运行版。

回退：停用新入口前先处理在途 worker，保留事件和快照。SQLite schema 不变，但旧程序不认识新的 WorkFact 事件，事件重放必须保留新解析器或明确迁移，不能直接认定降级二进制可完整恢复新单元。
