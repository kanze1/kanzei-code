# 文件级审查依赖地图

> 当前执行范围（2026-10-03）：工作台与交互控制台 → 对话执行与恢复 → 工具与代码操作 → 多任务与自动运行。其他功能已移出计划；本页全仓依赖和历史轮次继续用于定位，历史“下一步”不再派发。以 [当前计划](parallel-plan.md) 和 [执行队列](parallel-queue.json) 为准。

初始基线：19489ca2；2026-10-03 整合 abb596f0 与 fd36faa3 后更新底层 API。日期：2026-10-03。文件、模块声明、公开 API、import 候选见 inventory.json；生成器为 scripts/file-audit-inventory.py。词法索引不能替代宏、动态派发和调用方的人工核实。

## 当前四块进度：审计与修复完成

2026-10-04：269/269 文件已审，确认的39项（P0 2、P1 24、P2 13）均已修复并验证。按Git/持久状态→执行恢复→工具版本归属→浏览器/UI owner顺序完成 F1–F8；caller和失败路径同步修复。详见 [修复总报告](audit-first/fixes-summary.md)。原始审计和历史验证保持不变。另一用户任务的改动独立保留，本轮没有发版。

## WB1–WB6 历史进度

累计新增全文审查 27 文件，33 个真实问题已修复（P1 × 18、P2 × 15）；四块主审已完成/可复用 88/269，剩余 181 个文件（远端新增生命周期测试文件计入待审）。详见 [WB1](WB1-workbench-state.md)、[WB2](WB2-session-navigation.md)、[WB3](WB3-conversation-ownership.md)、[WB4](WB4-resource-parsing.md)、[WB5](WB5-rendering-ownership.md) 和 [WB6](WB6-delivery-activity.md)。本包按附件解析 → 交付缓存 → 卡片，以及侧栏策略/体验事件队列 → 活动表现整合。当时拟定的 WB7（workspace 概览快照与身份 → 项目会话列表/异步工作台）现已纳入本次全文审计，不再单独排队。

- 交付归属：后端回执/当前 metadata → 项目缓存 → 当前选中 row → 卡片；旧子代理 display 不覆盖最新事实。附件日期/时长保持不同语义。
- 交互归属：共享侧栏持有 pointer/focus，运行/失败/收起计时仍按会话；同会话前序 delta 在 fact/终态前投递，其他会话保留帧队列；starting 清理旧表现而不修改运行真源。详见 WB6。

## crate 依赖

箭头指向被依赖方；表中列出全部生产内部依赖，图展示主干。

```mermaid
flowchart TD
  app["kanzei-app / kanzei CLI"] --> tools["kanzei-tools"]
  tools --> memory["kanzei-memory"]
  memory --> core["kanzei-core"]
  core --> harness["kanzei-harness"]
  core --> llm["kanzei-llm"]
  harness --> base["kanzei-base"]
  llm --> base
```

| crate | 生产内部依赖 |
|---|---|
| base | 无 |
| harness | base |
| llm | base；harness 仅为测试依赖 |
| core | base、harness、llm |
| memory | base、harness、llm、core |
| tools | base、harness、llm、core、memory |
| CLI / app | harness、llm、core、tools |

生产 crate 图无环。审查顺序：base → harness/llm → core → memory → tools → CLI/app → UI。模块内部回调和运行期循环不等同于 crate 依赖环。

## 状态、入口与边界

| 层 | 模块与公开入口 | 状态拥有者 / 边界 | 调用方 |
|---|---|---|---|
| primitive | base/atomic_file: write_atomic*, FileLock；path_form；content_hash | atomic_file/lock.rs 拥有进程内槽，标准库 OS 文件锁句柄拥有跨进程互斥；临时文件→rename 是单文件提交 | harness、llm/auth、core、memory、tools |
| 写入凭据 | base/write_log: record, entries_after, latest_by_path；LoggedContent | .kanzei/.write-log 是归因凭据，文档仍是业务真源 | memory/store、inbox、tools 写者、app/files_edit；managed/cross_tree 读取 |
| 工具契约 | harness/tool、tool_pipeline、permission、registry | ToolCtx、ToolOutput、不可变 HarnessSnapshot；权限与结果策略边界 | tools、memory、core runner、入口 |
| 调度 | harness/orchestration、managed_fence、async_mailbox | 租约、读槽位、队列；不能替代操作级文件锁 | core、tools、app |
| 模型协议 | llm/client、protocol、sse、auth/store | HTTP/SSE、凭据文件、统一 LlmEvent | core、memory/embed、入口 |
| 会话持久化 | core/store、history、replay | state.db 会话事实、事件及其 projection；SQLite 事务边界 | runner、tools、CLI、app |
| 执行 | core/runner/drive、phase | 每轮消息、取消、并发工具；持久化委托 store/history | tools/run、CLI/run、app/run |
| 记忆与文档 | memory/memory/store、docstore | markdown 真源，SQLite 搜索索引是派生数据；树锁与文档锁 | tools 再导出、tracker、app |
| 工具编排 | tools/managed、cross_tree、tracker、work、worktree、background | 围栏快照和日志归因；Git worktree 真源；后台进程句柄 | tool pipeline、CLI、app |
| 入口 | kanzei/cli；app/state、run、commands | AppState / SessionRuntime；Tauri command/event 跨 Rust/JS | UI invoke/event |
| 界面 | app/ui 自有 JS | 展示投影与草稿；数据库事实不能被展示状态覆盖 | 用户操作 |

## 底层修改的调用链

- `atomic_file`：经 memory/tools 再导出，覆盖 docstore、记忆、配置、凭据、会话工件、文件编辑。锁语义修改须保持共享/独占、同线程重入、限时等待和非 Send 契约。
- `write_log::record`：memory/store、memory/inbox、tools/lib 的 record_write_log、tools/write、app/files_edit。读者为 managed/cross_tree；恢复改用同一次窗口查询与 `latest_by_path`，已删除 `last_content`。
- CAS：tools/architecture、conventions、conventions/drafts、team/tools；app/files_edit、memory_chat。须逐一核实外层文件锁与 hash 口径。
- path_form：路径展示/打开与比较键分开；不能为了统一比较键而改变可打开路径。

## 历史依据

- docs/design/parallel_read_serial_write_orchestration.md：操作级 FileLock 与运行级调度的边界。
- base/lib.rs：R-208 / D-261 将文件原语下沉，保持零依赖。
- memory/lib.rs：R-203 拆分与再导出；scheduling 复制有历史依赖原因，不能仅凭重复删除。
- docs/architecture/01_runtime_loop.md、02_harness_registries.md：执行链和注册表。
- docs/design/architecture_diagrams.md：Cargo 清单是 crate 图真源。
- 原有未跟踪 `问题.MD` 保持未读取、未修改、未提交；本轮判断来自实际源码、调用链和回归证据。

## 覆盖口径

逐文件结论见 report.md。仅进入索引、搜索命中或编译通过的文件不算完成审查，不自动标 PASS。

## 按地图推进的文件级记录（2026-10-03 更新）

| 层 | 已完成内容 | 本轮 / 后续范围 |
| --- | --- | --- |
| base | 首批四文件审查；锁/CAS/路径/日志三态重构已整合验证 | 见整合报告；拆分文件不能仅因编译通过自动记为全文 PASS |
| harness | registry.rs、progress.rs 已完成首批全文审查 | 其他文件尚未全面审查 |
| llm 基础 | event.rs PASS；error.rs PASS；sse.rs P1 已修；protocol/mod.rs PASS；lib.rs PASS | 第二批五文件全文；[逐文件记录](llm-framing.md) |
| llm 请求与 Chat 状态机 | request.rs PASS；protocol/openai.rs P1 已修 | 第三批全文；[逐文件记录](protocol-completion.md) |
| llm caller | client.rs 的分帧和终态验证已检查；修复无终态 EOF 被误报成功 | 仅调用链切片，完整生命周期待审 |
| memory / tools | 原树底层 caller 已适配；managed、三类专用写者及后台守卫关键路径已修 | 不等同于整个模块全文审完 |
| core / API / UI | runner/drive.rs 已跟踪整步消费→工具分发边界，其余为合并与既有测试验证 | 全面逐文件审查待继续 |

第三批结束时的下一步为协议、proxy/auth/client；这些主链已在第四批推进。当前进度以下方最新批次为准。每轮以 coverage.json 和对应报告明确全文审查、调用链切片和未审范围，不用索引或测试数量代替审查覆盖。

## 第四批推进

- llm：剩余协议、proxy/auth/client 全文已审；修复结果见 [protocol-auth.md](protocol-auth.md)。
- core：history.rs 全文 PASS；drive.rs 仍按事件消费切片记录。
- 下一条链：core 存储/输入状态 → app 运行服务；同时从已稳定 CAS → 文件编辑服务 → 前端编辑器逐层检查。未审支线仍保留在 coverage.json。

## 第五批推进与当前进度

- 两条链：SessionStore 输入事务 → 桌面/CLI 准入与提升；CAS → files_edit/files_draft → 文件编辑器。
- 新增 6 个全文审查文件，累计 30 个；索引共 511 个文件，481 个尚未完成全文审查（包含已检查切片的文件）。
- 修复输入与事件非原子提交、保存/异步打开丢失新输入、重载失败提前删除草稿。详见 [state-ui.md](state-ui.md)。
- 全工作区 2231 passed / 0 failed / 5 ignored；Clippy、格式和完整 UI 运行时/浏览器回归通过。
- 七层均尚未全量结项；当前完成的是跨层主链，不能将主链完成等同于整层完成。
- 下一条链：输入存储 → 调度与停止 → 异步回调 → API/UI 状态投影。A 家专属范围按用户要求暂不处理。

## Sol 并行第一轮

- A0/B0/C0 和主代理 M0 已整合到 dev `fbb85333`；新增 20 个全文审查文件，累计 50/511，剩余 461 个尚未完成全文审查。
- base 的 9 个 Rust 文件均有全文记录，其他六层仍有未审支线。词法索引不覆盖全部配置、脚本和界面资产。
- 已收口：日志换行编码、托管窗口项目归因与基线并发、迁移事务与历史正文清理、后台事件桥及通知消费。回退段删除与交付工作树路由是调用链切片修复。
- 当前 A1 沿工件归档 → 压缩 → 会话提交 → GC 检查未提交正文；C1 沿进程退役和通知游标检查晚写；B1 沿旧问题回复 → 邮箱/子任务 → 停止检查复活窗口。
- 逐文件记录、分包验证和尚未完成的边界见 [第一轮整合记录](parallel-round-1.md)。全工作区整合检查与 main 同步在第二轮收包后执行。

## Sol 并行第二轮与当前进度

- A1/B1/C1/M1 已整合并通过当前 Windows 全工作区验证：2275 passed、0 failed、5 原有 ignored；check/Clippy/fmt、IPC 与完整 UI/浏览器回归通过。逐文件及全部验证边界见 [第二轮整合记录](parallel-round-2.md)。
- 新增 15 个全文记录，累计 **65/512**；尚未全文审查 **447**。新增 artifact_liveness 文件计入分母；调用方切片没有计为全文。
- 七层口径仅 base 全量完成，其余六层仍有未审支线。core store 13/26、runtime 1/39；harness 6/32、llm 15/16、memory 0/28、tools 0/126、CLI/app 19/145、UI 2/91。
- 状态 owner 收口：session_events 真源 → 原 source/sequence CAS → cache；工件发布共享锁 → GC 非阻塞独占；原 mailbox/generation → stop/callback 准入；退役表 → 进程别名；认证设备 → 通知/SSE 游标与撤销。
- 下一轮 C2 先稳 typed 事务/投影，B2 修 team worker 生命周期，A2 修 work-context source，主代理 M2 修手机凭据/消息持久化与服务 owner。已确认的多进程 team 恢复及工具副作用无提交 ack 问题保留在后续执行链，不能用本轮验证宣称它们已修。

## Sol 并行第三轮与最新进度

- C2/B2/A2/M2/D0 已整合，Windows 全工作区 **2315 passed / 0 failed / 5 原有 ignored**；check/Clippy/fmt、IPC 与完整 UI/浏览器回归通过。见 [第三轮整合记录](parallel-round-3.md)。
- 新增 15 个全文审查，累计 **80/512**，剩余 **432**；30 个调用链切片不计全文。七层仅 base 全量完成：harness 11/32、llm 15/16、core store 15/26 + runtime 4/39、memory 0/28、tools 3/126、CLI/app 21/145、UI 2/91。
- owner 收口：Immediate 事务内权威状态 → seed/projection；真实 source/version → work-context CAS；设备持久提交 → 鉴权 cache；手机原子输入 → canonical cache；team 唯一注册/token/attempt → 生命周期；服务槽 → listener 释放。
- 纠正第二轮“已确认多进程 team 恢复”表述：普通同 executable/home 窗口已有 service_lock，跨 portable/profile 共享项目尚需原生实测，未计为已确认问题。工具无持久提交 ack 的真实拒绝路径由 C3 修复中。
- 下一条底层到上层链：D1 共同配置文档事务 → M3 settings/models/规则身份 → A3 权限界面；C3 并行推进 durable ack → drive → 工具。当前未合入的新包不计入 80 文件。A 家专属继续暂缓。

## Sol 并行第四轮与最新进度

- D1/C3/A3/M3 已整合，Windows 全工作区 **2336 passed / 0 failed / 5 原有 ignored**；check/Clippy/fmt、IPC 与完整 UI/浏览器回归通过。见 [第四轮整合记录](parallel-round-4.md)。
- 新增 15 个全文审查，累计 **95/512**，剩余 **417**；其中 28 个仅检查了合同切片，389 个尚未审查。仍只有 base 全量完成：harness 19/32、llm 15/16、core store 15/26 + runtime 7/39、memory 0/28、tools 3/126、CLI/app 23/145、UI 4/91。
- owner 收口：配置文件路径锁 → 文档读改写/最终模型校验 → 原子提交；规则原项目与完整 tuple → 删除 IPC/权限事件 → UI 操作；typed 持久接受结果 → 同步 receipt → drive 后续工具/模型请求。
- 下一条链：C4 跨进程 session 执行 owner → CLI/desktop 恢复和输入收尾；D2 复用该 primitive → team worker 的恢复/准入/释放；A4 UI 偏好与 pane 状态 owner → M4 会话调用方和入口服务。
- B3 已用同 executable、不同受支持 profile 的真实双进程证实 team 错误恢复及第二模型请求准入，见 [原生观察](parallel/B3-native.md)。这条 P1 正由 D2 修复，未计入本轮已修复结果。新包尚未合入，不计入 95 文件。

## Sol 并行第五轮与最新进度

- 本轮16个审查包与补充验证已整合，Windows全工作区 **2427 passed / 0 failed / 7 ignored**，17项统一检查通过；新增app/CLI实际构建、双profile owner、11项偏好/导出、7项启动和完整UI/浏览器均有本次源证据。见[第五轮整合记录](parallel-round-5.md)。
- 全文累计 **132/514**，本轮新增 **37**，剩余 **382**。base与harness两层全文完成；五层仍有未审支线，A家专属仍暂缓。必要切片与未验证准备不计全文。
- 底层owner/ACK/原子结果 → DocStore/prefs原文真源与导出目录owner → 后台真实收尾 → 会话/workspace UI原身份，完整合同见本轮记录和逐包map。
- 后续A7/B5/C7/M7已准备，实际验证队列root统一 → B5 → A7 → C7；四工作树持续复用，一个Cargo runner，无push/release。

## 第六轮发布检查点

B5/A7 已合入并通过当前源的 18 项组合验证。全文累计 135/514，剩余 379；base/harness 两层完成，其余五层继续。C7 的 3 个 P1 已复现但尚未修复，M7/B6/A9 仍为草稿或只读候选，均排除本次发布。详情见 [第六轮记录](parallel-round-6.md) 和 [发布解读](../../reports/2026-10-03-file-audit-release.md)。最新用户已明确授权先发布本版，覆盖此前仅本地不 push/不发布的阶段限制；保留其他工作树和原有 问题.MD。

## 发布门禁补修后的当前覆盖

独立tests.rs误计生产的CLI P1已完成全文审查和9项回归/实际CLI前后对照，线路静态检查的验证P2已修，度量基线按既有规约更新且阈值不变。新增metrics.rs全文1个，当前 **136/514、剩余378**；CLI/app为31/146，其余分组不变，仍完成base/harness两层。前文18项组合证书保留原5d01fb3f身份，补修通过相关检查后还必须由当前最终提交重新执行规范Full。详见 [补修记录](parallel/Release-checks.md)。
