# 深度并行开发模式与模型选择隔离 — 深度分析

- 日期: 2026-08-08
- 状态: **评审中** —— §6 决策点未经用户定案前不动工;定案后按 §4 拆 R 条目
- 关联: R-050(重启方案) R-030 R-086 R-115 R-136 D-096 D-168 D-170

## 0. 一句话定位

- **深度并行开发**:把并行从"多个进程共用同一个工作目录"(R-030 现状)升级为"每条开发线绑定独立 git worktree、独立会话、独立模型,互不写冲突,应用内完成 diff→合并→清理"。
- **模型选择隔离**:模型选择从"前端 localStorage 记一份、后端内存记一份、全局 toml 是真源"的三层脱节,收敛为"全局 → 项目 → 线"的分层真源,全部落后端、可持久、可恢复。

两件事在一个用例上交汇:**同一条需求开两条线,各用不同模型(如 claude vs codex)独立实现,diff 对比选优**。没有 worktree 隔离,两条线互相污染;没有模型隔离,两条线跑的是同一个模型。这是把它们放进同一个计划的原因。

## 1. 现状盘点

### 1.1 并行能力:四层,只有最上层缺口

| 层 | 现状 | 位置 |
|---|---|---|
| 工具批内并发 | ✅ 已落地(R-097 批一):互不冲突的工具按 wave 并发,`ToolConcurrency` 三态冲突判定 | `kanzei-core/src/runner.rs`、`kanzei-harness/src/tool.rs` |
| task 子代理 | ✅ 同轮 ≤8 个并行,FuturesUnordered,完成即回报 | `runner.rs`(MAX_TASKS_PER_TURN) |
| 多进程(R-030) | ✅ 每进程独立 session/队列/模型覆盖/连跑;⚠️ 但共用同一工作目录,写冲突零防护;进程列表不持久化(P3 未做);后台进程事件被前端丢弃(R-086) | `kanzei-app/src/main.rs` ProcessHandle |
| worktree 并行线(R-050) | ❌ **命令存在但完全脱节**:`worktree_create/diff/merge/discard` 四命令齐全(含 `merge-tree --write-tree` 冲突预检),但 `ProcessHandle.worktree_path` 恒 None,`run_prompt` 强校验进程必须属于主项目目录——没有任何线能在 worktree 里跑。R-050 已被验收核查退回 | `main.rs` worktree_* 命令 |

R-050 退回意见已经指明路线:"先让进程可绑定 worktree 并在其中运行,再补同工作树并行的写冲突防护,最后接 diff 查看器。"本设计就是这个路线的展开。

### 1.2 模型选择:三层脱节

解析链(`config.rs::resolve_model`):`primary`/`fast` 角色 → `provider:model` → `[providers]`。配置合并:全局 `~/.kanzei/kanzei.toml` ← 项目 `.kanzei/kanzei.toml` 覆盖(机制已存在且正确)。

但**选择的存放**分裂成三处、互相不知道对方:

| 层 | 存哪 | 问题 |
|---|---|---|
| 每次运行直选 | 前端下拉,随请求发送 | 无(这层是对的) |
| 进程覆盖 | `ProcessHandle.model` 纯内存 | **重启即丢** |
| 项目偏好 | `localStorage["kz-model:<项目路径>"]`(R-115) | 真源在前端;CLI/后端/移动端全都看不到;换台机器/清缓存即丢 |
| 角色默认 | 全局 toml `[models]` | 设置页**只写全局**;项目层 `[models]` 覆盖机制存在但无 UI,只能手改 toml;D-168 只做了"本页改动不会生效"的提示,没解决写入 |

R-115 把模型偏好按项目隔离——方向对,落点错(落在了前端)。本计划把它收回后端。

### 1.3 可直接复用的资产

- `ResolveCtx` 已经分离 `cwd` 与 `project_root` 两个字段——"代码在 worktree、身份在主根"的关键抽象**已经存在**。
- worktree 四命令 + 冲突预检真实可用,只差接线。
- 每进程 session 隔离(消息/权限/队列/停止)有 POC 测试背书,R-050 核查确认这部分真实。
- D-170 的教训与修复(`ensure_project_isolated`、显式根绑定)直接适用于 worktree 的根解析。

## 2. 目标定义

### 2.1 深度并行开发模式

**并行单元 = 开发线(line)= 绑定一个 worktree 的进程。** 完整生命周期:

```
创建线(自动建 worktree + 分支 kanzei/thread-<name>)
  → 并行开发(agent 在 worktree 内读写代码、跑测试、提交到线分支)
  → 审查(应用内看结构化 diff)
  → 合并(冲突预检 → merge --no-ff)或放弃(worktree remove,分支保留)
  → 清理(worktree 回收,会话记录留在主项目)
```

硬性质:
1. 两条线同时运行,代码互不可见、互不覆盖(git worktree 物理隔离,不靠锁)。
2. 项目文档(需求/缺陷/目标/决策/记忆)**全程单一真源**在主项目根——线是代码的分支,不是项目管理的分支(§3.2)。
3. 线的会话历史落主项目 state.db,worktree 删除后对话记录仍在。
4. 崩溃恢复:线清单以 `git worktree list` + state.db 为真源重建,不依赖前端缓存(R-050 退回点④)。

### 2.2 模型选择隔离

解析优先级定死为五层,高层缺省时落到低层:

```
① 本轮直选(前端下拉,随请求)            —— 已有
② 线/进程持久选择(后端持久化)          —— 新增:替代内存 + localStorage
③ 项目 [models](.kanzei/kanzei.toml)   —— 机制已有,补 UI
④ 全局 [models](~/.kanzei/kanzei.toml) —— 已有
⑤ 内置默认(anthropic:claude-sonnet-5 / ollama:qwen3.5:4b)
```

隔离语义:
- **项目间隔离**:A 项目 primary 指 claude、B 项目指 codex,互不影响(③ 层,git 可见可迁移)。
- **线间隔离**:同一项目内两条线各选各的模型,重启后各自恢复(② 层)。
- fast 角色同样分层:R-136 的 Ollama 一键安装写全局 ④,某项目要更大的本地模型时在 ③ 覆盖。

### 2.3 明确不做的

- 不引入"线"的新后端实体——扩展 ProcessHandle,不另造概念(§3.1)。
- 不做命名模型 preset(`[models.presets]` 之类)——两角色 + 直选已覆盖需求,复杂度不值。
- 不做跨线的自动任务分派/编排——线由用户手工开、手工派活;"agent 自动拆任务开多线"是另一个(更远的)计划。
- 不做图形化 DAG/画布式的线管理界面——页签 + 列表够用(与 R-111 对可视化的克制一致)。

## 3. 核心架构分析

### 3.1 并行单元:扩展进程,不造新概念

R-030 的 ProcessHandle 已经具备:独立 session、独立队列、独立模型覆盖、独立生命周期锁。深并行只需要给它加一个此前恒为 None 的字段的真实赋值路径:

```rust
struct ProcessHandle {
    // ...现有字段不动
    worktree_path: Option<PathBuf>,  // Some = 深并行线;None = 主树进程(现状)
}
```

改动面:
- `process_create` 接受可选 `worktree_name`;给定时先 `worktree_create` 再绑定,失败即整体失败(不留半绑定状态)。
- `run_prompt` 对 worktree 线:放行目录校验(现状会拒),`cwd = worktree_path`、`project_root = 主根`。
- `process_close` 对 worktree 线:只解绑不删树;删树走显式 `worktree_discard`(危险操作分离)。
- UI 文案:进程页签上 worktree 线显示分支名徽标;概念上向用户叫"**开发线**",避免"进程"这个实现词。

### 3.2 worktree 内 `.kanzei` 的归属(最关键决策)

**问题**:`.kanzei/project/*.md`、`.kanzei/memory/*.md`、`.kanzei/kanzei.toml` 都被 git 跟踪(`.gitignore` 只排除 state.db/index.db/inbox 等派生物)。`git worktree add` 会把它们 checkout 成**分支副本**。若放任不管:

1. `discover_project_root` 从 worktree 内向上找,命中 worktree 自己的 `.kanzei` → 线拿到一套过期的需求/缺陷/记忆副本,`project_session_id` 也随 worktree 路径变化 → **这正是 D-170 修过的"项目串了"的 worktree 变体**。
2. 线里的 tracker 写入落在分支副本上,主树同时也在写(自举循环并发是本仓的日常,defect ID 撞车已发生过,见 M-011/M-012)→ 合并时 docstore 整文件重写的冲突几乎必然,且语义上无法自动解决(两边各自分配了同一个 ID)。

**两个选项**:

- **A. 运行时重定向主根(推荐)**:worktree 只是代码的另一个工作目录;`.kanzei` 身份永远属于主项目根。线的 harness 以 `cwd = worktree`、`project_root = 主根` 构造——`ResolveCtx` 本来就有这两个字段,tracker/goals/memory/config/state.db 全部走 `project_root`,代码读写与 bash/git 走 `cwd`。worktree 里 checkout 出来的 `.kanzei` 副本**从不被读写**,分支上永远保持创建时的原样 → 合并时这些文件在分支侧零改动,git 自动取主干版本,**tracker 冲突被结构性消除**。
- B. 分支私有、合并时像代码一样 merge:保留"需求也能分支演化"的理论能力,换来 docstore 格式冲突、ID 撞车、记忆分叉三座大山。没有真实用例支撑这个复杂度。

选 A 的配套约束:
- 线进程携带**显式主根**,worktree 内一律不做根发现(D-170 教训:发现式根解析是事故源,显式绑定是解法)。
- 既有硬 deny `*.kanzei/project/*` 是路径 glob,天然覆盖 worktree 内的副本路径,agent 绕不过去。
- 配置读主根的 `.kanzei/kanzei.toml`(不读 worktree 副本)——避免"分支上改配置"这种无人需要的歧义。

### 3.3 一线一树:写冲突的结构性消除

R-030 风险清单和 R-050 退回意见都指向同一件事:同一工作树多个写入者没有防护。深并行的回答不是加锁,而是**让每个写入者独占一棵树**:

- **硬约束:一个 worktree 至多绑定一条线**;创建线时若目标树已被绑定则拒绝。
- 主工作树维持现状(默认进程 + 可开多进程),但这是历史兼容位——UI 上引导"要并行就开线",主树多进程并跑时显示共享警示。不在本计划里给主树加文件锁(收益低:有了线,主树多进程并跑写代码的场景应当消亡)。
- 唯一残余的共享写点是**主根 `.kanzei` 的 tracker/记忆**(所有线 + 主树 + 自举循环都写它)。docstore 是整文件重写,lost-update 真实存在。P4 给 docstore 加进程级文件锁(Windows 用 `std::fs` 独占句柄即可,毫秒级持有)收口。

### 3.4 模型解析链:改动落点

②层(线/进程持久选择)的存放位置——三个候选:

| 候选 | 评价 |
|---|---|
| **state.db 新表 `processes`(推荐)** | 线是项目的运行时状态,与 sessions 同库同生命周期;D-170 后 state.db 严格按项目隔离,天然继承;R-030 P3"进程列表持久化"与此一并交付(一表两用:线注册 + 模型/子代理开关/profile) |
| app.json | 全局文件存项目级状态,又一个 D-170 式串扰源 |
| 项目 kanzei.toml | 线是短命运行时,写进用户手编辑的配置文件会互相打架 |

③层补 UI:设置页 `[models]` 区加**作用域选择器(全局/本项目)**;写本项目 = `toml_edit` 追加到主根 `.kanzei/kanzei.toml`(复用 `append_allow_rule` 的保排版写法)。D-168 的"本页改动不会生效"提示场景随之消亡——用户可以直接写生效的那层。

迁移:`localStorage["kz-model:*"]`、`kz-manual-models:*` 一次性上迁后端(首次启动读到旧键就写入 ② 层并清除);前端下拉降级为纯回显 + 写入口,不再是真源。CLI 侧 `KANZEI_MODEL` 语义不变(等价 ① 层)。

### 3.5 资源现实(不设计,先可见)

- **provider 限流/花费**:N 条线并跑 = token 燃烧 ×N,共享同一账号限流。本计划不做预算强制(subagent_management.md 的策略层留了位置),只做可见:线页签常驻 running 状态 + 每线 token 计数(episodes 已记,取出来显示)。
- **Ollama 单实例串行**:多条线的 fast 子代理会排队,属预期,文档写明即可。
- **cargo target ×N**:每个 worktree 独立 `target/`(gitignored → 冷启动全量编译,本仓一次数分钟、数 GB 磁盘)。共享 CARGO_TARGET_DIR 会引入构建锁串行与产物混写,**正确性优先,默认各自独立**,创建线时提示磁盘/首编译成本;sccache 之类留给后续可选优化。
- **同时跑测试**:两条线同时 `cargo test` 会打满 CPU——接受,用户自己掌握开几条线。

### 3.6 与 R-086 的依赖关系

R-086(控制事件按会话路由、pending ask 重建)是**多线同时跑的 UX 完整性**前置,不是深并行本身的前置:

- P1(单条线绑 worktree 跑通全生命周期)不依赖 R-086——用户切到哪条线看哪条,与现状多进程一致。
- 多条线**同时 running** 时,后台线的权限询问/结束状态依赖 R-086 的状态机分离,否则挂死(D-055/D-056 的老问题在 N 条线下被放大 N 倍)。
- 结论:不等 R-086 启动 P1,P4 与 R-086 合流收口。若 R-086 先落地更好,纯受益。

## 4. 分阶段方案与验收

### P1 线绑定 worktree(后端打通)——R-050 退回意见第一步

- `process_create` 支持建线(自动 worktree + 分支);`ProcessHandle.worktree_path` 有真实 Some 路径。
- `run_prompt` 放行线目录:`cwd=worktree`、`project_root=主根`;线内 agent 能读写代码、跑测试、在线分支上提交。
- 线清单真源 = `git worktree list --porcelain`(启动时发现、孤树提示回收),废除 `localStorage["kz-worktrees:*"]`。
- 线会话落主项目 state.db(session_id 加 `#w<name>` 后缀,与 `#p<n>` 同构)。
- **验收**:一条真实需求在线上完成"开线→开发→提交→冲突预检→合并→放弃树"全流程,全程主根 tracker 单真源无分叉;worktree 内 `.kanzei` 副本在分支上零改动;删线后会话历史仍可回放;四个 worktree 命令补上测试(R-050 遗留:当前零测试)。

### P2 模型选择隔离收口

- state.db 建 `processes` 表:线/进程注册 + 模型/profile/子代理开关持久化(= R-030 P3 一并交付)。
- 五层解析链落码 + 测试(每层缺省回落逐层验证)。
- 设置页 `[models]` 作用域选择器(全局/本项目);顶栏下拉写入 ② 层;localStorage 一次性迁移。
- **验收**:重启后每项目、每线的模型选择完整恢复;两项目配不同 primary 互不影响(D-170 式双项目用例);CLI 与桌面解析结果一致(同一真源);设置页写项目层后 `models_list` 与徽标立即反映。

### P3 深并行 UX

- 结构化 diff 查看器(按文件树 + 逐文件 diff 渲染,收口 D-096"文件名列表弹 toast");合并/放弃的确认流程与结果反馈;冲突预检结果的可读展示(哪些文件、哪边改的)。
- 线仪表:页签徽标(分支名/running/待询问)、每线 token 计数。
- 对比用例打通:同一需求两条线不同模型,diff 页并排看两个分支各自对 HEAD 的改动。
- **验收**:不离开应用完成 review→merge→清理;合并失败时双方改动保留且有可恢复入口(R-050 原验收);800/1024/1280 布局检查。

### P4 多线并跑完整性(与 R-086 合流)

- 后台线 ask 补发/状态复位(R-086 落地后按线接通);多线并发下 ask/done 不丢不串的验证。
- docstore 进程级文件锁(主根 tracker 的最后一个共享写点,自举并发 ID 撞车的根治)。
- 崩溃恢复:重启后线(worktree+分支+会话+模型选择)完整重建。
- **验收**:两条线同时 running,各自询问互不阻塞、结束状态各自复位;kill 进程后重启,线清单与状态完整恢复;并发写 tracker 的压测不丢条目不撞 ID。

## 5. 风险与教训

1. **R-050 的前车之鉴**:上次就是"部件各自真实、组合从未成立"被退回。本计划每阶段验收都要求**端到端真实轨迹**(不是单元测试拼图),P1 不跑通完整生命周期不进 P2。
2. **worktree 根解析事故面**:任何一处遗漏的 `discover_project_root` 调用都可能让线拿到 worktree 副本身份(D-170 变体)。对策:线路径全程显式传根;补"worktree 内运行时 project_root 必须等于主根"的断言测试。
3. **Windows 现实**:路径长度(worktree 目录名 + target 深层路径)、文件句柄占用导致 `worktree remove` 失败(编辑器/终端开着树内文件)。discard 失败时的现有兜底("已保留以便恢复")要延伸到 UI 提示。
4. **迁移兼容**:localStorage 旧键、既有 `#p<n>` 进程会话、`kz-model` 全局旧键——迁移一次性完成并保留旧键 fallback 一个版本。
5. **自举循环共存**:自举 agent 跑在主树,用户开的线与它并行——这恰好是深并行的第一个真实用户场景,也是 P4 文件锁的真实压力来源。

## 6. 决策点(定案后动工)

| # | 问题 | 选项 | 推荐 |
|---|---|---|---|
| D1 | worktree 内 `.kanzei` 归属 | A 运行时重定向主根 / B 分支私有随代码合并 | **A**(§3.2,冲突结构性消除) |
| D2 | 并行单元 | 扩展 ProcessHandle / 新建独立实体 | **扩展**;UI 文案叫"开发线" |
| D3 | 线级模型选择持久化位置 | state.db `processes` 表 / app.json / 项目 toml | **state.db**(§3.4,兼收 R-030 P3) |
| D4 | 一线一树硬约束 | 强制 / 允许多线共树+加锁 | **强制**;主树多进程仅作历史兼容 |
| D5 | 排期与 R-086 关系 | P1 先行、P4 合流 / 等 R-086 | **P1 先行** |
| D6 | cargo target | 每树独立 / 共享 target 目录 | **独立**(正确性优先,提示成本) |
| D7 | 设置页模型写入 | 加作用域选择器(全局/本项目) / 维持只写全局 | **加作用域**(D-168 根治) |

## 7. 与既有条目的关系

- **R-050**:本文档即其重启方案;定案后 R-050 按 P1/P3/P4 拆批重写验收(或关闭原条、新开子条引用本文档)。
- **R-030**:P3"进程持久化"并入本计划 P2;其风险清单的写冲突项由 D4+P4 回应。
- **R-086**:保持独立推进;本计划 P4 是它的第一个多线消费方。
- **R-115**:localStorage 按项目记模型——P2 迁移后端后其前端实现退役。
- **R-136**:fast 一键安装继续写全局层;项目层覆盖 UI 由 P2 提供。
- **D-096**(diff 显示简陋):并入 P3 收口。
- **D-170**:显式根绑定原则延续到 worktree;`ensure_project_isolated` 不适用于线(线永不独立成项目)。
- **subagent_management.md**:其策略层(并发预算)与 §3.5 相邻,预算强制留给该计划,本计划只做可见性。

## 附录:早期 thread 级只读 POC 设计(2026-08-09,历史)

> 本节内容原为 `frontend_phase3.md` §八,2026-08-08 文档整理时移入本文档作为**历史决策记录**。它是 R-050 定案前的过渡设计,聚焦**同项目双线程只读隔离**(thread 级,不建 worktree);本文档正文(§1~§7)是 R-050 的现行方案(worktree 开发线)。内容冲突时以正文为准。

### 附录 A.1 线程—项目—session—worktree 关系

- `project` 是共享文档、规范和权限规则的资源边界。
- `process` 由 R-030 提供，是绑定项目的独立运行容器；同一项目可拥有多个 process。
- `thread` 是用户可见的并行对话线程，必须绑定一个 process，并拥有独立消息投影、运行句柄、停止边界、权限队列、输入队列和活动轨迹。
- `session_id` 是事件、队列和历史恢复的唯一归属键；线程不得只用 `project_dir` 路由事件。
- `worktree` 仅在只读 POC 之后启用：主线程可使用默认工作树，写线程必须绑定独立 worktree/分支；合并前只能生成 diff，不得自动覆盖主工作树。

### 附录 A.2 线程状态机与边界

```text
created -> idle -> running -> stopping -> idle
                         └-> failed -> idle
created/idle -> closed
```

- `stop(thread_id)` 只取消该线程的 runner、pending ask、steer 和 queue，不得影响同项目其他线程。
- `closed` 线程拒绝新输入；运行中的线程必须先进入 `stopping`，等待句柄收尾后再释放资源。
- 崩溃恢复按 `session_id` 重放最后一致事件：未完成运行恢复为 `failed/recoverable`，不得伪造为成功；待处理权限询问默认失效并要求重新发起。

### 附录 A.3 锁顺序与并发规则

锁顺序固定为：`thread lifecycle -> session admission -> project write lock -> git/worktree lock`；禁止反向获取。

- 只读线程不得持有项目写锁，允许并行读取。
- 需求/缺陷/目标整文件重写和 git 操作必须经过项目写锁；检测到版本/哈希变化时拒绝静默覆盖并返回冲突。
- worktree 创建、合并和清理必须在 git/worktree 锁内完成，失败时保留双方分支和恢复入口。
- POC 阶段只验证消息、权限、队列、活动事件和停止隔离，不写项目文件、不提交 git。

### 附录 A.4 双线程只读 POC 验收矩阵

| 场景 | 线程 A | 线程 B | 通过条件 |
|---|---|---|---|
| 消息隔离 | 发送 prompt A | 发送 prompt B | 两侧只出现自己的 user/assistant 消息，历史恢复不串线 |
| 运行隔离 | running + stop A | 持续运行 B | stop A 后 B 仍运行，事件只路由到对应线程 |
| 权限隔离 | 产生 ask A | 产生 ask B | 回答 A 不改变 B 的 ask；关闭 A 只清理 A |
| 队列隔离 | admission/steer A | admission/queue B | FIFO、取消和 drain 只作用于对应 session_id |
| 活动隔离 | task/工具轨迹 A | task/工具轨迹 B | 面板可按线程过滤，轨迹和失败状态不互相覆盖 |
| 崩溃恢复 | 中断 A | B 正常结束 | A 恢复为 failed/recoverable，B 保持成功，均可继续操作 |
| 只读门禁 | 请求写文件/git | 正常读取 | POC 拒绝写入并保持主工作树与项目文档无变化 |

### 附录 A.5 后续入口

待 R-030 确定 `process_id/session_id` 命令与事件契约后，先实现上述 POC 的内存态双线程测试，再接入 worktree、diff 审查和冲突合并；在此之前不得实现 R-050 的并行写入流程。

### 附录 A.6 POC 验收入口

使用 `.\scripts\r050-poc-check.ps1` 可重复运行 R-050 当前只读准备测试：

1. `cargo test -p kanzei-core`：包含跨 session 事件回放和队列停止隔离测试；
2. `cargo test -p kanzei-app`：桌面端回归；
3. `node --check crates/kanzei-app/ui/main.js`：前端语法检查。

该脚本明确不创建 worktree、不调用真实 LLM、不写项目文件、不执行 git 操作。通过只代表 SessionStore 隔离不变量和回归测试通过，不代表 R-050 并行运行或 R-030 进程契约已经完成。
