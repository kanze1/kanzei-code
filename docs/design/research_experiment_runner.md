# Research 实验运行与路线图:字段与 Markdown 格式冻结

- 身份: live_design
- 状态: 设计基线(2026-09-01 用户逐点定调,字段与 Markdown 格式本轮冻结;协议方向见 A-014)
- 上游文档: [research_mode.md](research_mode.md)(工件落点/证据等级/档位)、[interaction_modes.md](interaction_modes.md)(模式体系)、[phase2_system_upgrade.md](phase2_system_upgrade.md)
- 关联决策: A-014(terminal callback 协议)、A-015(路线图真源)、A-016(执行与环境边界)、A-017(业务模型独立)、A-018(执行策略分档)、A-012(运行事实进 SQLite)
- 实施条目: R-343 R-344 R-345 R-346 R-347 R-348

## 0. 本文补的是哪半条链路

`research_mode.md` 覆盖的是**文献与仓库研究 → 论文级工件**:检索、阅读、综述、LaTeX、绘图、引用校验。
它没有覆盖**真的把实验跑起来**这半条:实验路线图、Experiment/Run/Result、服务器与显卡环境、
付费资源控制、Terminal Callback 与实时监控。

本文只做增量,不改 R-221/R-276/R-277 已验收的范围。既有条目的验收边界原样保留,新工作
全部走 R-343~R-347 新条目。

### 0.1 模块独立性边界(A-017)

用户开篇定的第一条:**Research Mode 是独立模块,和 Dev Mode 完全独立**;追问后收敛为
「底层可复用,主要是数据与业务模型的独立」。这条约束比任何字段都靠前,因为它决定了
后面所有对象该长在哪里。

**可以复用的底层**:SQLite 与事件存储、文件与 artifact 存储、进程启动与日志捕获、
Tauri command 机制、Markdown/PDF 渲染、LaTeX 编译引擎、搜索与权限框架、窗口能力。

**必须独立的业务面**:

- **领域对象**:研究问题、假设、Experiment、Run、Result、Environment 是 research 自己的对象;
  **不拿 dev 的 task/session/round 冒充实验模型**,dev 的任务关闭逻辑不影响实验状态。
- **事实链**:实验事实从 research 自己的 Markdown 与产物重建;不把研究结果当成 dev 对话历史的附件。
- **运行态**:实验运行状态自己显示、自己监控;dev 没有活动任务时实验照样可监控。
- **UI**:独立工作台与导航,不复用 dev 的任务面板/运行面板当主界面。
- **命令命名空间**:research 的后端命令与事件类型自成一套,不复用 dev 的业务命令。

共用底层但分开建表、分开投影;两边只保留显式的登记回流(research_mode.md 定调点 4),
不共享业务状态。

## 1. 已冻结的方向(用户定调,本轮不再重议)

1. **路线图节点 = Experiment**。假设、结果、结论是节点的上下文;多次 Run 不单独占主图节点,
   进实验详情;Terminal、实时指标、checkpoint 与产物挂在具体 Run 下。
2. **Experiment Markdown 是路线关系的唯一真源**,路线图只是自动生成的可视化投影。
   `route.md` 若保留,只写研究者的总体叙事、关键转折与人工注释,**不承载机器关系**。
3. **Terminal callback 协议**:带 `@@kanzei` 前缀的单行 JSON;普通 stdout/stderr 原样保留(A-014)。
4. **第一版执行边界 = 本机 + SSH 远程服务器**。不做作业调度器提交。
5. **环境 = 手工登记清单 + 每次 Run 的运行时快照**。不做全自动探测。
6. **执行策略按环境分档**,不是全局一个开关:自己的机器和卡可以宽松,共享与付费资源必须受控(见 §2.5)。
7. **模板只做 LaTeX 研究文档模板**,内置几套基础的即可;第一版不做通用模板市场与外部模板导入(见 §8)。

> 为什么关系写进实验文件而不是单独的 `route.md`:双真源必然漂移。项目里 D-276/D-294
> 一族已经付过这个学费——同一事实两个落点,最后两边都不可信。图能被完整重建,叙事不能,
> 所以机器关系归 Markdown 字段,人的叙事归 `route.md`。

## 2. 事实模型:四类对象

| 对象 | 真源载体 | 标识 | 理由 |
| --- | --- | --- | --- |
| Experiment | `<topic>/experiments/E-<n>.md` | `E-<n>`(topic 内唯一) | 要被人读、被论文引用、被路线图投影 |
| Run | state.db 运行事实 + `<topic>/runs/<run-id>/` 产物 | `<run-id>` | 高频、可重跑、可物理删除(A-012) |
| Result | Experiment Markdown 的 `## 结果` 段 | 挂 run-id 锚 | 稳定研究事实,不是过程事实 |
| Environment | `.kanzei/research/environments.md` + 每 Run 的 `environment.json` | `ENV-<name>` | 声明与实况分开记 |

事实边界(A-014 重申):高频 terminal 行与指标点进运行事件流/日志/指标产物;
**稳定研究事实由实验结束后的 Markdown 摘要维护**,不把每条回调写进事实文档。

### 2.1 Experiment 字段(冻结)

frontmatter 用与 tracker 同一口径的**单行值**,不引入嵌套 YAML:

```text
kind: experiment
id: E-007
topic: <kebab-case 课题名>
title: <一句话>
status: draft | ready | running | done | failed | abandoned
hypothesis: <一句话可证伪陈述>
depends_on: E-003 E-005          # 前置实验 = 路线图的边(空表示根节点)
supersedes: E-004                # 可选:取代关系,图上画虚线
entry_refs: R-xxx D-xxx          # 可选:与 dev backlog 的挂钩,只收 R-/D-/T-
environment: ENV-gpu01           # environments.md 中的登记项
budget: 40 gpu-hour              # 可选,见 §7
created_at / updated_at          # unix_ms
```

正文段落用**固定标题**,投影器只认这几个:

- `## 假设` —— 一段话 + 证伪判据(什么结果算否定它)。
- `## 参数` —— 表格 `名称 | 值 | 说明`。这一段是实验的身份:参数变了就是**另一个实验**,不是另一次运行。
- `## 运行` —— 表格 `run-id | 起止 | 状态 | 环境 | 关键指标 | 产物路径`;由运行器在 run 结束时追加,人不手写。
- `## 结果` —— 每条一句话 + V 等级 + 证据锚(run-id 或 `file:line`);沿用 research_mode.md §4 的 V 表,不混用 E0-E4。
- `## 结论` —— 对假设的裁决:支持 / 否定 / 不确定,加一行下一步。
- `## 后续` —— 派生实验和**为什么**派生(反向边由 `depends_on` 提供,这里写给人看的理由)。

### 2.2 Run 字段(冻结)

运行事实进 state.db(A-012),产物落 `<topic>/runs/<run-id>/`:

```text
run_id, experiment_id, topic
status: queued | running | succeeded | failed | cancelled
execution: { kind: local | ssh, host, workdir, command, env_id }
started_at, finished_at, exit_code, cancel_reason
code_ref: { repo, commit, dirty }        # 可复现的代码锚;dirty=true 时结果只能标 V1
params_digest                            # §2.1 参数段的摘要,和 code_ref 一起决定可比性
environment_snapshot_ref                 # runs/<run-id>/environment.json
artifacts[]: { kind, path, bytes }       # checkpoint | figure | log | metric-series
metrics_last: { name -> value }          # 末值;完整序列在 metric-series 产物里
cost: { gpu_seconds, currency, amount, source }   # §7
callback_stats: { parsed, malformed, truncated }  # 协议健康度,不静默吞
```

`params_digest` 与 `code_ref` 一起回答「这两次 run 能不能放一张图上比」——没有这两个锚,
跨 run 的曲线对比就是在骗自己。

### 2.3 Result 不是第五张表

Result 就是 `## 结果` 段里的条目加一个 run 锚。理由:结果要被人读、被论文引用、
被路线图节点显示;run 是可重跑、可清理的过程事实。把结果单独做一张表,等于又造一个
和 Markdown 并列的真源——见 §1 的双真源教训。

### 2.4 Environment 登记(手工 + 快照)

`.kanzei/research/environments.md`,一个环境一节:

```markdown
## ENV-gpu01 [active]
- kind: ssh
- host: user@10.0.0.11
- gpu: 4 × RTX 4090 24G
- workdir: /data/exp
- 计费: 按卡时 | 单价 <x>/h | 结算方 <who>
- 备注: 
```

每次 Run 启动时抓一份 `environment.json` 快照:`nvidia-smi` 输出、python/框架版本、
可用显存、`git rev-parse HEAD` 与工作树是否 dirty。

**登记表是你声明的,快照是当时真实的。** 两者不一致时在运行画像里显式标注环境漂移,
不静默覆盖登记表——环境漂移本身就是实验结果不可比的头号原因,它必须可见。

### 2.5 执行策略、租约与凭据(A-018)

用户原话:「给用户受控的,或者我是自己的服务器和卡就可以随意一点」。所以策略是**每个环境
一档**,不是全局一个开关。登记项里多一个 `policy` 字段:

| 档位 | 典型归属 | 启动前 | 运行中 |
| --- | --- | --- | --- |
| `relaxed` | 自己的机器/自己的卡 | 直接跑,不问 | 照常记录命令、快照、终端、产物;超时与异常仍清理 |
| `managed` | 共享服务器 | 查租约/资源/并发,冲突则拒绝 | 同上,外加占用登记与结束释放 |
| `approval` | 付费资源 | 显示预估时长与费用,等用户确认 | 同上,外加预算刹车(§7) |
| `strict` | 高风险或他人资源 | 每次运行都要显式确认 | 同上 |

**`relaxed` 不等于不管**:它免掉的只是「每次弹确认」,命令记录、环境快照、terminal 捕获、
产物保存、超时与异常清理一条都不能省——这些是结果可复现的前提,不是权限门。

**租约(managed/approval)**:共享环境按 `ENV-*` 维护占用状态与并发上限,启动前认领、
结束或超时后释放。要挡住的是两个具体事故:两个实验抢同一块卡;实验失败后远端进程
继续挂着烧钱。

**凭据不进 Markdown**:登记项只写服务器标识与 secret 引用(如 `secret://my-server/ssh`),
真实密钥走系统凭据通道。Markdown 是要进 git、要给人读的,任何形式的密钥都不该落在那里。

**运行合同**:每次 run 除 §2.2 字段外,再记 `policy`、`lease_id`(有租约时)、
`max_duration`、`cleanup`(失败后如何收尾)。这几项是「启动前答应了什么」的留痕,
和「实际发生了什么」的快照分开存。

## 3. Terminal Callback 协议(A-014 落到字段级)

每行 `@@kanzei <single-line-json>`;其余输出原样保留为终端日志。
公共信封 `{"t": <event>, "ts": <unix_ms>, ...}`。

| 事件 | 必填字段 | 语义 |
| --- | --- | --- |
| `stage` | `name` | 阶段切换(训练/评测/导出) |
| `metric` | `name`, `value`(可选 `step`、`split`) | 指标点;进序列产物,不进 Markdown |
| `progress` | `done`, `total`(可选 `unit`) | 进度 |
| `artifact` | `kind`, `path` | 产出登记;`path` 相对 workdir |
| `checkpoint` | `path`(可选 `step`、`metric`) | 断点 |
| `message` | `level`, `text` | 人读消息 |
| `heartbeat` | —— | 存活信号,决定「卡死」判据 |
| `result` | `status`(可选 `summary`、`metrics`) | 实验自报结果 |

运行器另行记录:`run_started`、`run_finished`、`run_failed`、`run_cancelled`、`environment_captured`。

健壮性约束:

- 单行上限 8KB,超长截断并补一条 `message`,计入 `callback_stats.truncated`。
- JSON 解析失败**不终止运行**:原行进终端日志,计入 `callback_stats.malformed`。
- 未知 `t` 值保留原文进日志,不崩运行器、不猜语义。
- 不依赖 W&B/TensorBoard/云端账户;后续可提供语言级便利封装(一个打印函数),但协议本身是纯文本。

## 4. 路线图投影(只读,不落第二真源)

- **节点** = Experiment,颜色随 `status`;标题取 `title`,悬停显示 `hypothesis`。
- **边** = `depends_on`(前置→后续,实线)、`supersedes`(替代,虚线)。
- **输入只有** `<topic>/experiments/*.md`。缺失 id、成环、悬挂引用在投影时**报诊断**,
  不静默丢边——图上少一条边比报错更难发现。
- **下钻**:节点 → 实验详情(假设/参数/运行列表/结果/结论)→ run → 终端回放、指标曲线、产物。
- 图可整块重建;**不做图的手工编辑**——编辑图就是编辑 Markdown。

## 5. 执行边界(第一版:本机 + SSH)

- `local`:直接起进程,workdir 取自环境登记项。
- `ssh`:复用系统 ssh 客户端,不自造实现;远端只要求「能跑命令 + 能把带前缀的行打到 stdout」。
- 取消:关闭通道 + 发终止信号;远端被强杀时靠 `heartbeat` 超时判定,不假装还在跑。
- 断线重连:从产物与最后一次 heartbeat 恢复视图,状态标「连接中断,最后心跳 <t>」。
- **不做**:Slurm/K8s 作业提交、容器编排、跨机分布式训练编排、自动挂载与数据同步。

## 6. 实时监控

- 前端订阅 run 事件流:指标画曲线、进度画条、message 与原始输出进终端视图。
- 高频合并:同名 metric 在时间窗内合并,长跑不产生事件风暴(与 R-284 高频 delta 边界同口径)。
- 断点续看:重连从持久事实恢复;表现事件允许丢帧,持久事实不允许。
- 实验 run 与 dev 的任务级运行画像(R-338 起)是**两条事实链路**,共用事件包络但不合并展示。

## 7. 付费资源控制

- 每个环境登记计费口径(按卡时/包月/自有);每次 run 记 `gpu_seconds` 与折算金额。
- 预算旋钮两级:实验级 `budget`(frontmatter)与环境级上限;`approval` 档启动前先给预估。
- 超限行为:**停止发起新 run + 显式告警**,不杀正在跑的、不静默继续烧钱。
- **不做**:真实账单对接、自动扣费、跨环境成本优化调度。

## 8. 与既有系统的关系

- `research_mode.md` §3 工件落点:`experiments/` 与 `runs/` 是 topic 目录的新增子目录,其余结构不动。
- A-012:运行事实进 state.db,Markdown 承担可读治理与稳定研究事实。
- R-284 事件契约:实验事件走同一 snake_case 事件包络,不另起一套词表。
- R-273/R-274/R-275:论文、图表与调色板复用既有专用通道,本设计不新造绘图能力。
- **LaTeX 与模板(R-348)**:第一版只做**内置基础 LaTeX 模板**(基础报告/基础论文/实验记录/带图表论文),
  新建时把模板复制进 topic 的 `latex/`;编译产出 PDF 与编译日志,PDF 可直接预览并保留历史版本;
  实验图表以路径引用插入论文。**不做**外部模板导入、模板市场、上游模板同步。
  一次编译本身也是可追溯产物:`.tex` → 编译运行 → 编译日志 → PDF → 当时的环境快照。
- research 档工具面:实验运行需要**发起命令**的能力,而 research 档 bash 是硬 deny
  (research_mode.md 定调点 6)。因此运行器是**专用工具通道**(与 latex/plot 同一手法),
  不是给 research 档开 bash——这条是 R-344 的硬边界。

## 9. 分批实施

| 条目 | 范围 |
| --- | --- |
| R-343 | 实验事实模型与 Markdown 真源:frontmatter/段落解析、校验、E- 编号分配、experiments/ 骨架 |
| R-344 | Experiment Runner:本机与 SSH 执行、`@@kanzei` 回调解析、run 事实与产物落盘(专用工具通道,不开 bash) |
| R-345 | 环境登记表与运行时快照:environments.md 契约、`environment.json` 采集、环境漂移标注 |
| R-346 | 路线图投影与下钻前端:实验图、实验详情、run 列表与产物入口 |
| R-347 | 实时监控与付费资源预算:指标曲线、进度、终端回放、成本记账与超限刹车 |
| R-348 | 内置 LaTeX 模板与 PDF 预览:模板落项目、编译日志、PDF 历史版本、实验图表引用 |

## 10. 边界(本设计不做)

- 不做实验调度器与队列管理(第一版一次跑一个,排队由人决定)。
- 不做 W&B/TensorBoard/云端账户对接。
- 不做自动超参搜索与自动选题。
- 不做跨项目实验库与常驻实验服务。
- 不把每条 callback 写进 Markdown。
- 不改 dev 侧取活纪律与工具面;不动 E0-E4 在验证体系里的语义。
