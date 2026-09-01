# Research 实验运行与路线图:字段与 Markdown 格式冻结

- 身份: live_design
- 状态: 设计基线(2026-09-01 用户逐点定调;数据模型经同日二次收敛后冻结)
- 上游文档: [research_mode.md](research_mode.md)(工件落点/证据等级/档位)、[interaction_modes.md](interaction_modes.md)(模式体系)、[phase2_system_upgrade.md](phase2_system_upgrade.md)
- 关联决策: A-014(terminal callback 协议)、A-015(路线真源)、A-016(执行与环境边界)、A-017(业务模型独立)、A-018(执行策略分档)、A-019(两层模型与只记录不定义)、A-012(运行事实进 SQLite)
- 实施条目: R-343 R-344 R-345 R-346 R-347 R-348

## 0. 本文补的是哪半条链路

`research_mode.md` 覆盖的是**文献与仓库研究 → 论文级工件**:检索、阅读、综述、LaTeX、绘图、引用校验。
它没有覆盖**真的把实验跑起来**这半条:实验路线图、探索与实验结果、服务器与显卡环境、
付费资源控制、Terminal Callback 与实时监控。

本文只做增量,不改 R-221/R-276/R-277 已验收的范围。既有条目的验收边界原样保留,新工作
全部走 R-343~R-348 新条目。

### 0.1 模块独立性边界(A-017)

用户开篇定的第一条:**Research Mode 是独立模块,和 Dev Mode 完全独立**;追问后收敛为
「底层可复用,主要是数据与业务模型的独立」。这条约束比任何字段都靠前,因为它决定了
后面所有对象该长在哪里。

**可以复用的底层**:SQLite 与事件存储、文件与 artifact 存储、进程启动与日志捕获、
Tauri command 机制、Markdown/PDF 渲染、LaTeX 编译引擎、搜索与权限框架、窗口能力。

**必须独立的业务面**:

- **领域对象**:研究方向、探索、实验结果、环境是 research 自己的对象;
  **不拿 dev 的 task/session/round 冒充实验模型**,dev 的任务关闭逻辑不影响实验状态。
- **事实链**:实验事实从 research 自己的 Markdown 与产物重建;不把研究结果当成 dev 对话历史的附件。
- **运行态**:实验运行状态自己显示、自己监控;dev 没有活动任务时实验照样可监控。
- **UI**:独立工作台与导航,不复用 dev 的任务面板/运行面板当主界面。
- **命令命名空间**:research 的后端命令与事件类型自成一套,不复用 dev 的业务命令。

共用底层但分开建表、分开投影;两边只保留显式的登记回流(research_mode.md 定调点 4),
不共享业务状态。

## 1. 已冻结的方向(用户定调,本轮不再重议)

1. **数据模型只有两层:探索 → 实验结果**。研究方向(=topic)之下是探索,探索之下挂实验结果;
   跑多少组参数就挂多少条结果。**不建第三层对象**。
2. **只记录,不定义**。kanzei 不理解也不管理参数空间、扫描策略、超参语义——参数在结果里
   是一段自由文本,由用户或脚本自己报。系统负责的是把结果、环境、终端、产物**记全并可回溯**。
3. **路线图节点 = 探索**。假设、结论是节点的上下文;一次次实验结果不占主图节点,进探索详情。
4. **探索 Markdown 是路线关系的唯一真源**,路线图只是自动生成的可视化投影。
   `route.md` 若保留,只写研究者的总体叙事、关键转折与人工注释,**不承载机器关系**。
5. **Terminal callback 协议**:带 `@@kanzei` 前缀的单行 JSON;普通 stdout/stderr 原样保留(A-014)。
6. **第一版执行边界 = 本机 + SSH 远程服务器**。不做作业调度器提交。
7. **环境 = 手工登记清单 + 每次实验的运行时快照**;远端第一次人工准备,准备步骤记进登记项,之后复用。
8. **执行策略按环境分档**,不是全局一个开关:自己的机器和卡可以宽松,共享与付费资源必须受控(见 §2.4)。
9. **编号 topic 内唯一**(`E-001` 起)。**实验不做并行**——实验结果之间往往相互依赖,
   并行是开发那边的需求,不是这边的。
10. **模板只做 LaTeX 研究文档模板**,内置几套基础的即可;第一版不做外部模板导入(见 §8)。

> 为什么关系写进探索文件而不是单独的 `route.md`:双真源必然漂移。项目里 D-276/D-294
> 一族已经付过这个学费——同一事实两个落点,最后两边都不可信。图能被完整重建,叙事不能,
> 所以机器关系归 Markdown 字段,人的叙事归 `route.md`。

> **2026-09-01 二次收敛记录**:本文初稿把对象定为 Experiment / Run / Result 三层,并要
> 「参数变了就是另一个实验」。用户否掉了这个框架:「参数扫描这个是不同实验呀,我们只负责
> 结果记录,不负责定义这些」。三层塌成两层,参数从建模对象降为自由文本。下面 §2 是收敛后的版本。

## 2. 事实模型:三类对象 + 一张环境登记表

| 对象 | 真源载体 | 标识 | 理由 |
| --- | --- | --- | --- |
| 研究方向 | `.kanzei/research/<topic>/` 目录本身 | `<topic>` | 沿用 research_mode.md §3,不新造 |
| 探索 | `<topic>/explorations/E-<n>.md` | `E-<n>`(topic 内唯一) | 图节点;要被人读、被论文引用 |
| 实验结果 | 探索 Markdown 的「实验结果」段 + `<topic>/explorations/E-<n>/<result-id>/` 产物 | `E-<n>-<nn>` | 一次跑一条;高频事实进 state.db 与产物 |
| 环境 | `.kanzei/research/environments.md` + 每次实验的 `environment.json` | `ENV-<name>` | 声明与实况分开记 |

事实边界(A-014 重申):高频 terminal 行与指标点进运行事件流/日志/指标产物;
**稳定研究事实由实验结束后的 Markdown 摘要维护**,不把每条回调写进事实文档。

### 2.1 探索字段(冻结)

frontmatter 用与 tracker 同一口径的**单行值**,不引入嵌套 YAML:

```text
kind: exploration
id: E-001
topic: <kebab-case 研究方向名>
title: <一句话>
status: draft | running | done | abandoned
hypothesis: <一句话可证伪陈述>
depends_on: E-003 E-005          # 前置探索 = 路线图的边(空表示根节点)
supersedes: E-004                # 可选:取代关系,图上画虚线
entry_refs: R-xxx D-xxx          # 可选:与 dev backlog 的挂钩,只收 R-/D-/T-
environment: ENV-gpu01           # environments.md 中的默认环境
budget: 40 gpu-hour              # 可选,见 §7
created_at / updated_at          # unix_ms
```

正文段落用**固定标题**,投影器只认这几个:

- `## 假设` —— 一段话 + 证伪判据(什么结果算否定它)。
- `## 实验结果` —— 一张表,一行一次实验(见 §2.2);由运行器在实验结束时追加,人可以补结论列。
- `## 结论` —— 对假设的裁决:支持 / 否定 / 不确定,加一行下一步。
- `## 后续` —— 派生探索和**为什么**派生(反向边由 `depends_on` 提供,这里写给人看的理由)。

没有 `## 参数` 段——参数是每条结果自己的事,不在探索级冻结。

### 2.2 实验结果字段(冻结)

一次跑 = 一条结果。表格行(Markdown,人读)与运行事实(state.db,机器读)是同一条记录的两面:

表格列固定为 `实验 | 参数 | 状态 | 关键指标 | 产物 | 结论`。其中**参数是自由文本**,
系统不解析、不校验、不据此判等价——这是 §1 第 2 条的直接后果。

state.db 侧的运行事实(A-012):

```text
result_id, exploration_id, topic
status: queued | running | succeeded | failed | cancelled
execution: { kind: local | ssh, host, workdir, command, env_id }
policy, lease_id, max_duration, cleanup        # §2.4 的运行合同
started_at, finished_at, exit_code, cancel_reason
params_text                                    # 原样保存的自由文本,不解析
code_ref: { repo, commit, dirty }              # dirty=true 时结论只能标 V1
environment_snapshot_ref                       # <result-id>/environment.json
artifacts[]: { kind, path, bytes }             # checkpoint | figure | log | metric-series
metrics_last: { name -> value }                # 末值;完整序列在指标产物里
cost: { gpu_seconds, currency, amount, source } # §7
callback_stats: { parsed, malformed, truncated }
```

`code_ref` 回答「这条结果是哪版代码跑出来的」。参数不做结构化,所以跨结果比较由人来判断——
系统的职责是把**当时的参数原文、代码版本和环境快照**一并留下,而不是替用户定义可比性。

### 2.3 环境登记(手工 + 快照 + 准备步骤)

`.kanzei/research/environments.md`,一个环境一节(完整样例见 §11.2)。

每次实验启动时抓一份 `environment.json` 快照:`nvidia-smi` 输出、python/框架版本、
可用显存、`git rev-parse HEAD` 与工作树是否 dirty。

**登记表是你声明的,快照是当时真实的。** 两者不一致时在界面显式标注环境漂移,
不静默覆盖登记表——环境漂移本身就是实验结果不可比的头号原因,它必须可见。

**远端准备(用户定调)**:每个人的机器和目录习惯都不一样,所以第一次跑一台新服务器,
代码与数据由**人工准备**,运行器只负责问清楚并把准备步骤记进登记项的 `准备步骤` 字段;
之后同一环境的实验直接复用,不再问。运行器**不自动同步代码与数据集**。

### 2.4 执行策略、租约与凭据(A-018)

用户原话:「给用户受控的,或者我是自己的服务器和卡就可以随意一点」。所以策略是**每个环境
一档**,不是全局一个开关。登记项里的 `执行策略` 字段:

| 档位 | 典型归属 | 启动前 | 运行中 |
| --- | --- | --- | --- |
| `relaxed` | 自己的机器/自己的卡 | 直接跑,不问 | 照常记录命令、快照、终端、产物;超时与异常仍清理 |
| `managed` | 共享服务器 | 查租约/资源/占用,冲突则拒绝 | 同上,外加占用登记与结束释放 |
| `approval` | 付费资源 | 显示预估时长与费用,等用户确认 | 同上,外加预算刹车(§7) |
| `strict` | 高风险或他人资源 | 每次运行都要显式确认 | 同上 |

**`relaxed` 不等于不管**:它免掉的只是「每次弹确认」,命令记录、环境快照、terminal 捕获、
产物保存、超时与异常清理一条都不能省——这些是结果可复现的前提,不是权限门。

**租约(managed/approval)**:共享环境按 `ENV-*` 维护占用状态,启动前认领、结束或超时后释放。
要挡住的是两个具体事故:和别人抢同一块卡;实验失败后远端进程继续挂着烧钱。
**不为「同一用户并行跑多个实验」设计**——见 §1 第 9 条。

**凭据不进 Markdown**:登记项只写服务器标识与 secret 引用(如 `secret://my-server/ssh`),
真实密钥走系统凭据通道。Markdown 是要进 git、要给人读的,任何形式的密钥都不该落在那里。

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
- 不依赖 W&B/TensorBoard/云端账户;后续可提供语言级便利封装,但协议本身是纯文本。

## 4. 路线图投影(只读,不落第二真源)

- **节点** = 探索,颜色随 `status`;标题取 `title`,悬停显示 `hypothesis`;
  节点上带一个「已跑 N 次」的计数与最近一次状态。
- **边** = `depends_on`(前置→后续,实线)、`supersedes`(替代,虚线)。
- **输入只有** `<topic>/explorations/*.md`。缺失 id、成环、悬挂引用在投影时**报诊断**,
  不静默丢边——图上少一条边比报错更难发现。
- **下钻**:节点 → 探索详情(假设/实验结果表/结论)→ 单条结果 → 终端回放、指标曲线、产物。
- 图可整块重建;**不做图的手工编辑**——编辑图就是编辑 Markdown。

## 5. 执行边界(第一版:本机 + SSH)

- `local`:直接起进程,workdir 取自环境登记项。
- `ssh`:复用系统 ssh 客户端,不自造实现;远端只要求「能跑命令 + 能把带前缀的行打到 stdout」。
- **代码与数据不由运行器同步**:见 §2.3 的远端准备约定。
- 取消:关闭通道 + 发终止信号;远端被强杀时靠 `heartbeat` 超时判定,不假装还在跑。
- 断线重连:从产物与最后一次 heartbeat 恢复视图,状态标「连接中断,最后心跳 <t>」。
- **不做**:Slurm/K8s 作业提交、容器编排、跨机分布式训练编排、自动挂载与数据同步。

## 6. 实时监控

- 前端订阅实验事件流:指标画曲线、进度画条、message 与原始输出进终端视图。
- 高频合并:同名 metric 在时间窗内合并,长跑不产生事件风暴(与 R-284 高频 delta 边界同口径)。
- 断点续看:重连从持久事实恢复;表现事件允许丢帧,持久事实不允许。
- 实验监控与 dev 的任务级运行画像(R-338 起)是**两条事实链路**,共用事件包络但不合并展示。

## 7. 付费资源控制

- 每个环境登记计费口径(按卡时/包月/自有);每条结果记 `gpu_seconds` 与折算金额。
- 预算旋钮两级:探索级 `budget`(frontmatter)与环境级上限;`approval` 档启动前先给预估。
- 超限行为:**停止发起新实验 + 显式告警**,不杀正在跑的、不静默继续烧钱。
- **不做**:真实账单对接、自动扣费、跨环境成本优化调度。

## 8. 与既有系统的关系

- `research_mode.md` §3 工件落点:`explorations/` 是 topic 目录的新增子目录,其余结构不动。
- A-012:运行事实进 state.db,Markdown 承担可读治理与稳定研究事实。
- R-284 事件契约:实验事件走同一 snake_case 事件包络,不另起一套词表。
- research 档工具面:实验运行需要**发起命令**的能力,而 research 档 bash 是硬 deny
  (research_mode.md 定调点 6)。因此运行器是**专用工具通道**(与 latex/plot 同一手法):
  命令由用户指定,但必须经运行器与环境管理器,不能绕过链路直接起 bash。
- **LaTeX 与模板(R-348)**:第一版只做**内置基础 LaTeX 模板**(基础报告/基础论文/实验记录/带图表论文),
  新建时把模板复制进 topic 的 `latex/`;编译产出 PDF 与编译日志,PDF 可直接预览并保留历史版本;
  实验图表以路径引用插入论文。**不做**外部模板导入、模板市场、上游模板同步。
  一次编译本身也是可追溯产物:`.tex` → 编译运行 → 编译日志 → PDF → 当时的环境快照。

## 9. 分批实施

| 条目 | 范围 |
| --- | --- |
| R-343 | 探索与实验结果的 Markdown 真源:frontmatter/段落解析、校验、E- 编号分配、目录骨架 |
| R-344 | Experiment Runner:本机与 SSH 执行、`@@kanzei` 回调解析、结果事实与产物落盘(专用工具通道,不开 bash) |
| R-345 | 环境登记表、准备步骤、运行时快照与执行策略分档 |
| R-346 | 路线图投影与下钻前端:探索图、探索详情、结果列表与产物入口 |
| R-347 | 实时监控与付费资源预算:指标曲线、进度、终端回放、成本记账与超限刹车 |
| R-348 | 内置 LaTeX 模板与 PDF 预览:模板落项目、编译日志、PDF 历史版本、实验图表引用 |

## 10. 边界(本设计不做)

- **不定义参数语义**:不做超参搜索、不做参数空间建模、不判断两条结果是否可比。
- 不做实验队列与并行调度(实验之间常互相依赖,串行是常态)。
- 不做 W&B/TensorBoard/云端账户对接。
- 不做跨项目实验库与常驻实验服务。
- 不把每条 callback 写进 Markdown。
- 不改 dev 侧取活纪律与工具面;不动 E0-E4 在验证体系里的语义。

## 10.1 默认取值(2026-09-01 用户认可,实现时按此,不必再问)

这些没有单独立决策,但已经过用户过目认可。实现时直接采用;要改就在改动处写一行理由。

| 项 | 默认 |
| --- | --- |
| 指标序列格式 | `metrics.jsonl`,一行一个 metric 事件,追加写 |
| 结果编号 | `E-<n>-<nn>` 顺序号,topic 内唯一 |
| 终端日志上限 | 不截断;超大日志由 UI 分页,不在写入端丢数据 |
| heartbeat 超时 | 10 分钟无心跳判卡死;做成可配置项 |
| 失败后 cleanup | **保留远端产物**,只清临时目录——清错了不可逆,宁可留 |
| 探索被否定 | 只标 `abandoned`,不删文件、不从图上摘;失败路径本身是研究事实 |
| 跨课题引用 | 写 `<topic>/E-001` |
| PDF 历史版本 | 全留,不自动清理 |
| 模板变量填充 | 最小占位替换,不做表单 |
| research 运行态 | 独立运行态但**共用应用进程**,不单开 kz 进程(与 R-030 进程模型不冲突) |

## 11. 附录:可直接照抄的骨架

### 11.1 目录结构

```text
.kanzei/research/<topic>/
├── route.md                       # 可选:人工叙事、关键转折;不承载机器关系
├── explorations/
│   ├── E-001.md                   # 探索(图节点)
│   ├── E-001/                     # 该探索每次实验的产物
│   │   ├── E-001-01/
│   │   │   ├── environment.json   # 运行时快照(不可覆盖)
│   │   │   ├── stdout.log         # 原始终端
│   │   │   ├── metrics.jsonl      # 指标序列(每行一个 metric 事件)
│   │   │   └── artifacts/
│   │   └── E-001-02/
│   └── E-002.md
├── environments.md                # 环境登记表(手工维护)
├── latex/                         # R-348 模板落点
├── figures/
├── sources.md                     # 既有
├── findings.md                    # 既有
└── report.md                      # 既有
```

### 11.2 `environments.md`

```markdown
# Environments

## ENV-gpu01 [active]
- kind: ssh
- host: user@10.0.0.11
- 归属: personal
- 执行策略: relaxed
- gpu: 4 × RTX 4090 24G
- workdir: /data/exp/nas-search
- 运行时限: 24h
- 计费: 自有 | 单价 0 | 结算方 —
- 凭据引用: secret://gpu01/ssh
- 准备步骤: 首次已人工完成:git clone <repo> 到 workdir;数据集软链到 /data/datasets;conda activate exp。后续实验直接复用本目录。
- 备注: 卡 0-1 常被室友占,跑前看一眼 nvidia-smi。

## ENV-rent-a100 [active]
- kind: ssh
- host: root@203.0.113.9
- 归属: shared
- 执行策略: approval
- gpu: 1 × A100 80G
- workdir: /workspace/exp
- 运行时限: 6h
- 计费: 按卡时 | 单价 8.5/h | 结算方 <平台名>
- 凭据引用: secret://rent-a100/ssh
- 准备步骤: 每次开机后需重新 pip install -r requirements.txt(镜像不持久)。
- 备注: 超过预算会停新实验,在跑的不杀。
```

### 11.3 `explorations/E-001.md`

```markdown
---
kind: exploration
id: E-001
topic: nas-search
title: 小模型上先验证搜索空间是否有效
status: running
hypothesis: 在 CIFAR-10 上受限搜索空间能在 4 GPU-hour 内找到优于手工基线的架构
depends_on:
supersedes:
entry_refs:
environment: ENV-gpu01
budget: 20 gpu-hour
created_at: 1788230400000
updated_at: 1788256800000
---

## 假设

受限搜索空间(仅 3 类算子、深度 <= 8)能在 4 GPU-hour 内找到 test acc 高于手工基线
(93.2%)的架构。**否定判据**:三组不同随机种子跑满预算后,最好结果仍不超过基线。

## 实验结果

| 实验 | 参数 | 状态 | 关键指标 | 产物 | 结论 |
| --- | --- | --- | --- | --- | --- |
| E-001-01 | seed=0 lr=1e-3 ops=3 | succeeded | test_acc 0.921 @ 4.0h | [产物](E-001/E-001-01/) | 略低于基线 |
| E-001-02 | seed=1 lr=3e-4 ops=3 | succeeded | test_acc 0.938 @ 3.6h | [产物](E-001/E-001-02/) | 超过基线 |
| E-001-03 | seed=2 lr=3e-4 ops=5 | failed | — | [产物](E-001/E-001-03/) | OOM,ops=5 超显存 |
| E-001-04 | seed=2 lr=3e-4 ops=3 | running | test_acc 0.930 @ 2.1h | [产物](E-001/E-001-04/) | — |

### E-001-02

- 环境: ENV-gpu01 / [快照](E-001/E-001-02/environment.json)
- 命令: python search.py --seed 1 --lr 3e-4 --ops 3
- 代码: a1b2c3d (clean)
- 起止: 2026-09-01 14:02 → 17:38 (3h36m)
- 终端: [stdout.log](E-001/E-001-02/stdout.log) · 指标: [metrics.jsonl](E-001/E-001-02/metrics.jsonl)
- **事实**: test acc 0.938,超过手工基线 0.932;搜索在第 1800 步后不再提升。

## 结论

**倾向支持**,但证据还不够:三组种子里两组完成,其中一组超过基线。等 E-001-04 跑完再定。
ops=5 在 24G 卡上不可行,这是搜索空间的硬约束,不是超参问题。

## 后续

- [E-002](E-002.md):把胜出架构迁到 ImageNet 子集,验证是否只是 CIFAR 过拟合。
- 考虑另开一条探索处理显存约束下的算子选择(ops=5 需要梯度检查点)。
```

### 11.4 脚本侧回调最小示例

```python
import json, time

def kz(**kw):
    kw.setdefault("ts", int(time.time() * 1000))
    print("@@kanzei " + json.dumps(kw, ensure_ascii=False), flush=True)

kz(t="stage", name="search")
for step in range(2000):
    ...
    if step % 50 == 0:
        kz(t="metric", name="val_acc", value=acc, step=step)
        kz(t="progress", done=step, total=2000, unit="step")
kz(t="artifact", kind="figure", path="figures/acc.svg")
kz(t="result", status="succeeded", summary="test acc 0.938", metrics={"test_acc": 0.938})
```

`flush=True` 是必须的——缓冲住的回调在长跑里等于没有回调。
