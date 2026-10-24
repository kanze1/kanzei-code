# 记忆观测、生命周期与效果评价

- 身份：live_design
- 核对日期：2026-10-23，当前工作树源码。
- 关联：R-361 的历史实现与证据保留在后半部分；现行任务范围以 tracker 为准。

## 事实与来源

记忆内容以 Markdown 为真源，检索、使用观测与来源记录由现有领域服务提供。recall_events 是召回明细真源，旧 index.db.memory_recalls 不驱动当前页。项目知识与上下文选择见 [项目知识合同](project_knowledge.md)。

| 事实 | 当前语义与实现入口 |
| --- | --- |
| 检索机会 | 候选被考虑的机会，按记忆内容版本记录；不是采用或价值判断 |
| 召回、注入、正文读取 | 分别记录；读取关联同一 run 最近一次包含该记忆的注入，只证明工具返回过正文，可能是局部内容 |
| 采用、纠正与清除反馈 | 用户明确反馈，绑定内容修订；改写后的内容不能继承旧版本确认 |
| 操作恢复 | 同一 episode 中相同工具和完整目标的失败后成功；读取文件或无关命令成功不能验证先前修复 |
| 任务收益 | 需要固定任务对照或独立结果证据；上述计数与反馈不能单独证明因果收益 |

观测与反馈由 [memory_observations](../../crates/kanzei-core/src/store/memory_observations.rs)、[memory_retention](../../crates/kanzei-core/src/store/memory_retention.rs) 和 [usage](../../crates/kanzei-memory/src/memory/usage.rs) 实现。缺失历史观测显示未知，不补造旧身份或读取。排序以相关性为依据，不用注入次数充当采纳率。

## 生命周期与对话做梦

冷记忆处置已由 [retention](../../crates/kanzei-memory/src/memory/retention.rs) 实现：结合版本化机会、内容确认与保护条件判断，记录转移理由，保留隔离、归档和恢复路径。仅有年龄、零命中或零读取不构成自动退休的充分证据。阈值以实现中的 MemoryRetentionPolicy 为准，本文不另维护一份数值。

对话做梦已由 [memory_dream](../../crates/kanzei-tools/src/memory_dream.rs) 冻结 session 与消息范围，保存来源工件和 SHA-256；提案携带原文片段，speaker 与位置由引擎赋予。整理复用 [memory_consolidation](../../crates/kanzei-tools/src/memory_consolidation.rs) 的批次与作业，重复来源以稳定身份处理。提炼只形成待复核候选；review_required 的候选须确认对应内容版本才能晋升，不能把 assistant 建议当用户偏好。准入与状态转换仍走 [lifecycle](../../crates/kanzei-memory/src/memory/lifecycle.rs)。

当前 UI 与 IPC 位于 [memory.rs](../../crates/kanzei-app/src/memory.rs) 和 [13-memory.js](../../crates/kanzei-app/ui/13-memory.js)，显示使用观测、反馈、来源、保留原因和整理结果。领域服务是状态所有者，UI 是投影。

## 当前验证边界

本次核对确认上述源码路径与回归用例存在；本页不据此声明当前安装版全部验收通过。隔离/并发、旧修订反馈、做梦去重、晋升证据与恢复分别由相关回归覆盖；真实桌面、真实模型以及用户收益仍按各自回执判定。

效果评价复用 trace、replay 和任务画像。回放不实际执行外部工具，不能单独证明修改任务成功。10 月 6 日的 [冻结探索实验](../../dist/exploration-ab-20261006/report.md)是旧版本小范围结果；本轮建议的任务对照见 [优化调研](../reports/2026-10-23-optimization-research.md)。

## 历史记录说明

以下保留 2026-09-06 的迁移、实施、发布与验收观察。SQLite v23、旧 phase_pipeline 路径、测试数量、发布位置和当时的剩余批次只描述那个版本，不作为当前 schema、模块入口或待办。当前表结构以 schema 及迁移实现为准。

## 验证证据与复测用例

下表记录发布前的定向验证，使用临时库/测试运行或模拟前端数据；当时尚未安装新桌面程序。随后完整验证与安装结果见文末发布记录。

| 验证入口 | 测试与预期 | 结果 |
| --- | --- | --- |
| `cargo test -p kanzei-core --lib --offline -- --test-threads=1` | v22 缺列迁移、幂等、run 隔离、历史 NULL、miss、手动搜索、恢复配对、在线旧代理不算收益 | 284 项通过 |
| `cargo test -p kanzei-memory --lib --offline -- --test-threads=1` | 晋升先拒无恢复再接受匹配来源；排序不随注入计数变化；每轮提示；读取记录 | 168 项通过 |
| `cargo test -p kanzei-tools --lib read::tests --offline -- --test-threads=1` | 真 ReadTool 成功读取回填同运行，失败读取不回填，旧表不追认 | 10 项通过 |
| `cargo test -p kanzei-app --bin kzapp memory::tests --offline -- --test-threads=1` | 新表独立提供 IPC 数据、历史未知/实测 false 分开、损坏观测报错、来源缺口 | 2 项通过 |
| `cargo check -p kanzei-app -p kanzei --tests --offline` | 前后端 IPC 注册及 CLI/桌面调用链编译 | 通过 |
| 前端六项检查 | ui-runtime、ui-lint、parallel-lines-regression、ui-a11y、ui-i18n、ui-markdown；覆盖新字段、未知、miss、失败重试及迟到响应 | 全部通过 |
| `cargo test -p kanzei --test integration memory_hints_not_persisted --offline` | 提示只存在于当前运行上下文、不会累积回灌历史 | 2 项通过 |
| 其他前端检查 | IPC emit/listen 求差、工作空间浏览器回归、窄窗口布局 | 通过 |

前端操作验收：进入记忆页，最新检索默认展开，其余逐项展开；旧观测显示读取未知，未命中仍可查看检索词；发生加载错误时提示失败且能重试；快速切换项目时旧请求不得覆盖当前项目。

## R-361 第二批：重复失败重查（2026-09-06）

本批把重复失败从“仅增加 failure_count 标签”改为可审计的两步策略：`RecallWatch` 在同一运行内保存每个 `(tool, kind)` 最近一次原始候选 ID，并在下一次触发的 `RecallTrigger.previous_retrieved_ids` 中传给 policy；`FailureRecallPolicy` 在第二次及以后失败时加入“再次失败/重试/修复”意图词，过滤上一轮 Tier0/Tier1 候选，重新检索无新候选时返回真实 miss，不重复注入旧 Packet。实际 query 与排除候选写入 `recall_events`，因此遥测不会把重查次数倒推成策略变化（`crates/kanzei-core/src/runner/recall.rs:23-36,106-226`；`crates/kanzei-memory/src/memory/mod.rs:646-782,797-842`）。

回放评估底座是既有能力：`ReplayCase` 从真实 `run.trace` 解析固定任务，`run_arms` 对同一 case 执行 Current/LeaveOneOut 配对并写 `memory_eval`，`recompute_memory_effect` 只聚合成对样本（`crates/kanzei-core/src/replay.rs:34-43,267-360`；`crates/kanzei-core/src/store/eval.rs:94-154`）。本批复用而不重复申报该底座；真实 provider 的固定任务样本、重试/重复探索/用户纠正/成功率/耗时报告仍待第三批独立执行，不能由本地单测替代。

验证：`T-1786922726984` 记录 `cargo test -p kanzei-core -p kanzei-memory` 的 289+169 定向回归通过；新增 core 测试验证上一轮候选 ID 传播，新增 memory 测试验证重查 query 改变且旧唯一候选不再返回。

## R-361 第三批：有界任务上下文与结果呈现（2026-09-06）

B3 复用 Work Unit Projection 与既有事件真源，新增 `structured_control_output` 的 `context_capsule` 派生投影，按当前任务、实质进展、记忆来源声明和验证结果分组且继续受 `bound_text` 上界约束（`crates/kanzei-tools/src/work/output.rs:45-108`）。phase scout/review 对启用 Work Unit 的条目消费 checkpoint、决策、retrieval refs、验证命令和证据计数；legacy 条目保持原字段路径（`crates/kanzei-app/src/phase_pipeline.rs:606-670`）。docs snapshot 已提供完整 Work Projection，UI Work Unit 卡片展示实质进展、决策记录、记忆来源和验证结果（`crates/kanzei-app/src/docs.rs:404-418`；`crates/kanzei-app/ui/11-docs-list.js:729-800`）。记忆来源只标识 checkpoint 声明，注入/正文读取仍以 recall_events 为准，不把展示当采用或收益。

验证：`T-1786922726986` 六项前端清单与新增卡片 DOM 断言通过；`T-1786922726987` kanzei-tools 545 passed、kanzei-app 261 passed。当前窗口未挂载 `.work-unit-card`，因此没有把窗口缺少该区域当作视觉通过；自动化 DOM fixture 是本批可重放证据。真实 provider 固定任务的 Current/NoMemory 独立对照与重试、重复探索、用户纠正、成功率、耗时结果仍未执行，保留为 R-361 的唯一开放验收，不由本批单测替代。

## 2026-09-06 当时的待办与验证边界

1. 发布准备同步修复两个既有 CI 测试告警：`tracker/fields.rs` 的生产函数移至测试模块前，`tracker.rs` 移除多余可变引用；不改变业务行为。`cargo clippy --workspace --all-targets -- -D warnings` 已通过；发布前继续执行完整验证。
2. 用户于 2026-09-06 明确授权「直接发版就行」。完整 `scripts/verify.ps1 -Full`、打包、发布和安装校验进入发布流程；实际结果以对应提交的 `dist/verification.json`、发布日志及 GitHub Release 为准。原生桌面交互与实际收益单独验收。
3. 第二批：重复失败时真正改变重查策略，围绕决策点供给记忆；固定真实任务做开启/关闭记忆对照，测重试、重复探索、用户纠正、成功率与耗时。
4. 第三批：结伴/自主职责分离，Work Unit 有界上下文与实质进展判定，前端整合当前任务、依据和验证结果。
5. 本轮查询旧 CLI 的 `kz test --help` 意外落入通用运行入口，已中止；命令解析将未知子命令作为提示的行为应另行评估，未在本批修改。

## 变更记录

- 2026-09-06：按用户确认建立三批实施顺序；首批接通真实观测、收紧恢复证据、修正页面与统计语义，补迁移和前后端回归。分支 `kanzei/memory-feedback`；用户授权首批发版，合并与发布结果由发布记录留证。

## 发布后验收记录

2026-09-06 第一批已发布为 `build-4a85596c`，提交 `4a85596cbcb5f8a4fe056f11b741317a14216f75`；相对上一版共 1 个提交，`dev`、`main` 和远端标签一致。干净发布工作区执行完整 verify：14 项全部通过、无跳步；Rust 测试 1581 项通过、0 失败，2 项既有忽略用例。全目标 Clippy 另行通过。

安装包 18,069,947 字节，独立下载 SHA256 为 `e54edfe4ccfe19895e4ccf11936c33f8674d086bb3a892ea19a1e5da59d9d948`，与本地产物和 GitHub 摘要一致；Range 请求返回 HTTP 206。本机安装及随附 CLI 校验通过。桌面二进制与未打包产物仅存在 Tauri 标准的 3 字节 NSIS 类型标记差异，按当前 bundler 源码重建安装载荷后逐字节一致。

原生界面实操、实际收益对照和第二、三批仍未完成。上述发布后记录留在开发工作区维护；绑定发布提交的验收证据位于 `C:/Users/kanzei/Documents/kanzei-release/dist/acceptance-4a85596c/`，发布页为 https://github.com/kanze1/kanzei-code/releases/tag/build-4a85596c。


## D-745 自主运行实录修复（2026-09-06）

用户在复核 `ses_project_c0b8d633186c2464#p20` 后确认实施。本轮范围是调度、当前状态、验证证据、记忆相关性与等待信息；继续沿用 R-361 三批计划，尚不声明实际记忆收益对照完成。

### 实现与边界

- 调度器在本线未阻塞的在途任务中按工作优先模式稳定选择一项，其余列为 `queued_wip`。当前选择终态或阻塞后重新选择，不写入临时停车或依赖。人工停车、具名外部阻塞和其他线认领仍沿原判据处理。
- CLI 和桌面复用 harness 的逐步刷新上下文源，每次模型请求接收当前裁决与近期终态；不把旧裁决固定在 agent.system。`work next` 默认裁剪历史字段、队列和对账，`detail=true` 可取全量；`req/defect get` 不重复携带字段注册表。已归档条目的重复关闭返回 `ALREADY_TERMINAL`，不重新验收、不重复记成功。
- 前端冒烟命令归一化 Windows 路径并识别运行型 Node 调用；语法检查、echo、eval 不视作冒烟。提交级验证可读取 Git 注册工作树的同提交证据；当前源码/配置有变更或未跟踪文件、证据提交不符、所需检查未通过时拒绝复用。返回实际证据路径，保持条目关联要求。
- 记忆候选仍由 FTS/向量生成，在检索门面按显式符号与文件锚点筛选，再考虑标题、适用描述与正文覆盖。弱共词允许返回空；指纹命中和无词面交集的语义召回保留。自主轮预检索采用引擎当前选择，不再固定先取需求队列前两条。
- 自主轮 `question` 返回 `QUESTION_PENDING` 和结构化问题，随现有 ToolResult 持久化；实时工具卡及历史回放均可填入回复草稿。默认选项不构成答案，权限策略保持原判据。文档页显示阻塞条件并提供回复草稿入口；用户发送答复后由正常执行链复核解除条件。
- 本轮结束与任务完成分开。全部阻塞显示等待；只有 BacklogEmpty 使用完成样式，判定不依赖翻译后的文案。UIA 报告新增 `launch_mode`、`cold_start_verified`、`restart_verified`，复用用户窗口不能报冷启动或重启通过。

### 验收用例

| 入口 | 触发及预期 |
| --- | --- |
| 工具层 work 测试 | 多 WIP 按优先模式选一项，另一项排队；阻塞当前项后自动选择其余项；人工停车与其他线认领不被清除 |
| 刷新上下文测试 | 同一 harness 快照下关闭 D-001 后下一次模型请求选 D-002，并显示近期终态；长历史不再整段注入 |
| test_record coverage 测试 | 反斜杠与正斜杠、Node VM 运行参数等价；echo/check/eval 不放行；同提交工作树证据可复用，脏源码/错误 SHA/未通过检查拒绝 |
| memory 全套 | 实录中的标识符查询拒绝权限故事，真实匹配保留；无关查询返回空；原有精确指纹及语义改写用例通过 |
| UI 运行时与六项检查 | 全部阻塞显示等待，队列清空显示完成；当前与历史问题可回复，保留草稿和选项，点击不自动提交答案或解除阻塞；缺失/损坏历史保留原始可读结果 |
| 桌面与 CLI 编译 | 两入口均注册动态裁决，自主记忆检索与所选工作模式一致；IPC 和会话事件边界不新增数据库迁移 |

本地代码验证完成：工具层 545 通过、0 失败、1 项既有忽略；核心 287 通过；记忆 169 通过。轻量裁决路径调整后另跑 work 34 项全过；向量通道锚点约束另跑检索回归通过。六项前端检查、IPC 事件契约、PowerShell BOM、格式检查和相关包全目标 Clippy 均通过；桌面与 CLI 测试目标编译通过。

发布前项目 CLI 实测：默认输出 7432 字符/244 行，完整详情 60648 字符/934 行（字符数减少约 88%）；轻量默认裁决单次约 692ms，不作统计性能保证。当时两者同选 D-742，D-745/R-360/R-361 排队。全量交付对账只在显式 detail/reconcile 请求计算，不进入逐步刷新热路径。

已逐条撤销 R-360/R-361/D-742 在该实录中写入的“WIP 收敛让位 D-504”停车条件；保留 D-504、R-287 等真实用户阻塞及其他人工停车。原始原因已由 tracker 更新日志留痕，状态恢复不代表这些条目完成。

用户于 2026-09-06 明确授权「发版」。本批从 `kanzei/control-reliability` 合并至 `dev`，在干净发布工作区完成完整验证并发布为 `build-361de2e9`。D-745 已通过正常关闭门禁归档为 fixed；R-361 保持 1/3，固定任务收益对照及原生桌面实操仍待完成。当前用户仍在运行 `build-4a85596c`，本轮安装因进程运行而跳过。

首次发布验证在 `c4537325` 检出 `work.rs` 相对规模基线增长 120 行，超过 100 行上限。将近期终态摘要组装移入已有 `work/context.rs`，保持选取顺序与数量不变；规模门禁恢复通过，发布须重新生成最终提交的完整验证凭证。

本轮本地验收记录与裁决快照：`.kanzei/research/d745-control-reliability/validation.json`、`control.json`、`control-full.json`。这些记录对应提交前源码；最终提交的发布凭证见下节。

### D-745 发布后验收

2026-09-06 已发布 `build-361de2e9`，最终提交 `361de2e974b3713f83bbecfa87e2d321d9c5a283`；相对 `build-4a85596c` 共 2 个提交，`dev`、`main` 和远端标签一致。最终提交完整 verify 的 14 个步骤全部通过、无跳步；Rust 1590 项通过、0 失败、2 项既有忽略用例。测试记录 `T-1786922726980` 通过正式 test_record 工具登记并关联 D-745/R-361，D-745 通过同提交工作树证据复用及条目关联门禁后关闭。

独立下载安装包为 18,122,501 字节；SHA256 `624dd451b6169e71feb1e3669c7a1bbc7c86d1b5499a3120e5a1d2b0e93dbc4c` 与本地产物、GitHub 资产摘要一致。Range 请求返回 HTTP 206、`bytes 0-0/18122501`。Release 为正式发布，发布页：https://github.com/kanze1/kanzei-code/releases/tag/build-361de2e9。

安装阶段检测到现有 `kzapp` 进程，按安装脚本规则跳过。本轮没有安装新版本、关闭或重启用户窗口，原生桌面操作与实际任务收益对照继续待验。发布凭证、打包日志及下载校验留在 `C:/Users/kanzei/Documents/kanzei-release/dist/acceptance-361de2e9/`；本节及 tracker 收尾记录在开发工作区维护，不另造发布提交。
