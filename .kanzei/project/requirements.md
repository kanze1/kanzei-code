# Requirements

## R-174 子代理面板与并发度口径:独立 Running/Finished 面板、单条停止与完整 transcript [doing]
- 优先级: P0
- 复杂度: 中
- 标签: 前端
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(现状逐点读码核实,行号为 2026-08-10 dev HEAD)
- 来源: 2026-08-10 用户看过 Claude Code 的后台子代理面板后定调:kanzei 的子代理要走这个执行模型,而且比它更激进。四个轴——①后台化(跨轮存活、主代理派完不阻塞、完成发通知)②子代理能写(打破只读白名单、自持写租约)③并发度放开(远不止现在的 8)④可对话(给正在跑的子代理发消息带原上下文续跑)——**都要,但必须分级实现**(用户原话:「都要,但是你说的这些点得分级实现,确实改动大,风险多」)。参照物形态(Claude Code 面板实测):独立 Background tasks 面板,分 Running / Finished N 两区;每条显示名称、类型、已运行时长、累计 token、工具调用次数、当前正在用的工具名、View transcript 链接、单条停止按钮;面板有 Clear。本条是四轴里最便宜、可独立交付的一段(可观察 + 并发度口径),不依赖后台化。
- 设计定位: 四轴分级第 1 级——先把子代理变成「看得见、停得住、查得到」的对象;后台化(R-175)与写权(R-176)在此之上叠加
- 既有能力(§1.25 显式标注,不得重复申报为本次产出): 并发度**已经是可配项**——`max_tasks_per_turn` 在 crates/kanzei-harness/src/config.rs:59 是 `Option<usize>` 字段,:90-92 `unwrap_or(8).max(1)` 给默认 8 且**无上限钳制**;设置页已有「单轮子代理数上限」输入框(crates/kanzei-app/ui/index.html:469-470 `set-max-tasks`、ui/16-settings.js:187 与 :420、crates/kanzei-app/src/settings.rs:287-288/497/519/531);往返单测已钉死「没填的键不写进文件 / 没填走内置默认 8」(settings.rs:749-752)、serde default 单测已存在(config.rs:918-920、:936-939 越界回落 1)。因此「从固定 8 改为可配」这件事**无需再做**。
- 关键现状(本组三条需求的共同前置): 桌面端主对话**根本不注册 task 工具**——crates/kanzei-core/src/runner/drive.rs:57 只在 `subagent.is_some() && !config.execution_policy.is_serial_writer()` 时 push `task_spec()`,而 crates/kanzei-app/src/run.rs:107-108 给主对话**无条件**设 `ExecutionPolicy::ReadParallelWriteSerial`(`is_serial_writer()==true`,见 crates/kanzei-harness/src/orchestration.rs:21-23)。所以并行子代理在桌面端当前是**全禁**状态:drive.rs:410-503 的轮内并发批与 crates/kanzei-core/src/runner/subagent.rs:163-176 的读槽登记代码不可达,`max_tasks_per_turn` 配了也没有生效路径。该回归由 **R-173**(阶段编排对象)的阶段感知策略修复,是本条与 R-175/R-176 的共同前置——本条的面板与并发度实测必须在 R-173 修复后才能在桌面端取证。
- 内容: ①并发度口径收口(**不是重做配置**):复核默认 8 是否上调(用户要「远不止 8」),并在设置页把该值与「桌面端当前不生效」的事实对用户说清;溢出分支文案沿用 drive.rs:441-444 既有实现。②新增**独立「子代理」面板**——不再只作为活动面板(#bg-panel,ui/index.html:630-650)里 `bg-type-filter=agent` 的一个筛选项:Running / Finished N 分区,每条显示 名称 / 类型 / 已运行时长 / 累计 token / 工具调用次数 / **当前正在用的工具名** / 单条停止 / 打开 transcript,面板有 Clear。③单条停止通道:现状 ui/06-activity.js:261 注释明写「子代理没有单条停止通道,只能停整轮」,本条要消灭这个缺口。④可查看单个子代理的**完整 transcript**(工具调用序列 + 每次调用的入参与输出),不再只有 R-095 的摘要维度(内部调用数 / 当前步骤 / 成败 / 耗时)。
- 边界: 不做后台化(R-175)、不做写权(R-176);面板本条只需渲染**轮内并发**的子代理,跨轮存活条目待 R-175 提供数据后再接。不改 `max_tasks_per_turn` 的配置通道本身(已可用),只调默认值口径与设置页说明。
- 验收: ①并发度实测:`kanzei.toml [limits] max_tasks_per_turn = N`(N 取远大于 8 的值)后,同轮派发 N 个 task 全部执行、第 N+1 个才落 drive.rs:441-444 的溢出错误,有轨迹或日志证据;②旧配置无该键时行为不变——config.rs 既有 serde default 单测保持绿(若本条上调默认值,须同步更新 :918-920 断言并保留「缺键=内置默认」语义),settings.rs:745-752 往返单测保持绿(保存不丢字段);③面板存在且分区正确,每条的 名称/类型/时长/token/工具调用数/当前工具名 六个字段**均取自真实 RunEvent**(ToolStart/TaskProgress/ToolEnd),冒烟脚本用桩事件逐字段断言渲染出真实值而非常量占位;④单条停止真能停:点击后该子代理不再产出 TaskProgress、以「被停」终态收尾、读槽被释放,有实测证据(仅改 UI 类名/状态不算通过);⑤transcript 有真实数据源:能查看单个子代理的完整工具调用序列与每次调用的入参/输出——§1.25 明令「只展示但未接入真实数据源的界面壳不算完成」,不得以摘要冒充 transcript;⑥前端改动有冒烟断言:`node --check` + `node scripts/ui-runtime-smoke.mjs`,分区切换、单条停止、打开 transcript 三个新交互各有对应断言(§1.3);⑦桌面端可达性:R-173 修复前置回归后,在桌面端主对话实测面板真出现子代理条目(不能只在 CLI 或单测里成立)。
- refs: R-095 R-117 R-173 R-175 R-176
- 依赖: 
- 进展: 批1-3已提交(9179ae8/68ee84ec/25ea2c0),cargo test --workspace 全量全绿。验收①并发度实测✓(集成测试+轨迹)、②旧配置无键行为不变✓(serde default 测试绿)、③面板分区与六字段真实数据✓(冒烟逐字段断言)、④单条停止✓(stop_task + task_cancel_parallel.rs 实测)、⑤transcript 真实数据源✓(TaskTrace.input 渲染,冒烟断言入参)、⑥冒烟断言✓(分区切换/停止/transcript/被停终态/Clear 均有断言)。仅剩验收⑦「桌面端主对话实测面板真出现子代理条目」未闭环——需要构建新版 kzapp 安装,2026-08-11 用户定调:先不装,等下次发版一起实测。本条保持 doing 待发版,不占可执行槽位。
  ①**前置回归已解除**——「桌面端主对话根本不注册 task 工具」那条(本条与 R-175/R-176 共同记录的前置)已由 R-173 批4.5 修掉(`e933262`),验收⑦现在可以真去桌面端取证了。
  ②**验收③已部分交付**——R-173 收尾时把编排派发的勘察/复核子代理接上了活动面板(`ff287c4`):按 `input.phase` 分「勘察/复核」两组、显示角色名与**当前工具名**(取 `kz:task-progress` 的 `trace.name`)、运行时长、内部调用数,超时与失败分开成两种终态,冒烟有 6 组反证锁死。**它刻意复用 `#bg-list` 没有新建平行面板**——本条要做的独立面板应当在此之上演进,不是另起炉灶。仍缺:累计 token、Clear、Running/Finished 两区(现在是按阶段分组,不是按运行状态)。
  ③**验收④单条停止的最小改法已备**:目前 `dispatch_roles` 的 future 集合由屏障统一驱动,没有对外暴露的 per-role cancel handle。改法 = 每角色配一个 `CancellationToken` + 新 Tauri 命令按 role 触发,取消后该角色以 `ScoutOutcome::Failed("cancelled")` 进终态——屏障照常收敛,不会挂住。
  ④**两条形态决策留给本条拍**:(a) 编排派发的 8 条同时也会在**主对话**里各生成一个工具块(`chatToolStart` 无条件调用),信息没丢但每个自主推进轮多 8 个块,可能偏吵;(b) 前端条目的 `id` 就是角色名,而角色跨轮复用,所以当前实现是**每角色只留最新一轮**(跨轮定格的 bug 已修成"原地复位")。要保住历史轮次得让后端给 `role@round` 之类的唯一键。
  另:验收①②的「并发度可配」部分是**既有能力**(见本条「既有能力」字段),不要重做。

- 批次: 3/5

## R-178 模型隔离与线级状态持久化:state.db processes 表 + 设置页作用域选择器 [done]
- 优先级: P1
- 复杂度: 中
- 标签: 后端
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(现状逐点读码核实,行号为 2026-08-10 dev HEAD)
- 调度顺序: 与 R-177 **零耦合**,可并行甚至先做。其中 D7 那半(设置页作用域选择器)**改动面极小、可当天交付**——`settings_save_at_path` 已经是参数化路径的(crates/kanzei-app/src/settings.rs:562),接线点就在那,取活的人可以先拿这份即时收益再做 D3。
- 来源: 2026-08-10 用户对 docs/design/deep_parallel_dev.md §6 逐条拍板后,R-050 关闭拆条的第二条(= 该文 P2)。承接 D3(线级模型选择存 state.db)与 D7(设置页作用域选择器,第一版只覆盖 `[models]`)两条定案。
- 内容: ①D3:state.db 建 `processes` 表(现有表见 crates/kanzei-core/src/store/schema.rs,没有这张),存线/进程注册 + 模型 / profile / reasoning / 子代理开关;`ProcessHandle` 的这几个字段现在是纯内存 `Arc<Mutex<..>>`(crates/kanzei-app/src/state.rs:197-200),重启即丢。②五层解析链落码 + 测试(本轮直选 → 线持久选择 → 项目 `[models]` → 全局 `[models]` → 内置默认,逐层缺省回落)。③`localStorage["kz-model:*"]`、`kz-manual-models:*` 一次性上迁后端,前端下拉降级为回显 + 写入口,不再是真源;保留旧键 fallback 一个版本。④**顺带交付 R-030 遗留的「重启不丢页签」**——R-030 的 2026-08-07 核查把"进程列表不持久化(重启丢页签)"标 P3 暂不处理,至今未做,与本表同源一表两用。⑤D7:`settings_save` 加 `scope` 参数(全局 / 本项目),**第一版只覆盖 `[models]`**;写本项目 = `toml_edit` 追加到主根 `.kanzei/kanzei.toml`。⑥崩溃恢复里「模型/会话重建」那部分归本条(依赖同一张表)。
- 边界: **D7 第一版不放 providers / api key**——它们写进被 git 跟踪的项目 toml 有泄密风险,不一次全开;界面上要说清作用域选择器当前覆盖哪些字段,不留"选了本项目但某些字段仍写全局"的静默歧义。worktree 绑定属 R-177,本条不碰;崩溃恢复里的 worktree/分支重建属 R-177。
- 验收: ①重启后每项目、每线的模型 / profile / reasoning / 子代理开关完整恢复,页签不丢(R-030 遗留项一并核验)。②两个项目配不同 primary 互不影响(D-170 式双项目用例),CLI 与桌面解析结果一致(同一真源)。③五层解析链每层缺省回落各有单测。④localStorage 旧键存在时首次启动上迁并清除,迁移有测试;全仓 grep `kz-model:` 不再作为真源被读。⑤设置页选「本项目」保存后,主根 `.kanzei/kanzei.toml` 真出现 `[models]` 且立即生效(`models_list` 与徽标同步);选「全局」写 `~/.kanzei/kanzei.toml`,两者互不串写,有往返单测。⑥保存不丢字段(conventions §4),旧配置无新键时行为不变(serde default 单测)。⑦D7 覆盖范围在界面上对用户可见,providers/api key 的作用域切换被明确禁用而非静默忽略。
- refs: R-030 R-115 D-168 D-170 D-248

- 批次: 4/4

- 进展: 五批规划实际合为 4 批交付完成:批1 d575549(processes 落库/恢复)、批2 c597d0a(五层解析链收敛 harness 单源)、批3 540f178(localStorage 上迁清除+schema v12)、批4 ba616f7(D7 作用域选择器)。批5 收口即本轮复核+全量:验收① processes.rs 四函数+manual_models 贯通+迁移测试;② resolve_model_chain 桌面 run.rs:107/CLI main.rs:266 共用;③ config.rs:1312 五层缺省回落单测;④ 迁移成功/失败/回显冒烟断言;⑤ 批4 两个 D7 往返单测;⑥ 新参均 Option 缺省走 global 向后兼容,settings 10 测试全绿;⑦ 界面提示+后端按 scope 拦截。cargo test --workspace 全绿(T-1786439420)。关闭。

## R-157 验证与提交节奏引擎化:kanzei.toml 可调参数并注入循环 [doing]
- 优先级: P1
- 复杂度: 中
- 标签: 核心
- 来源: 2026-08-09 用户定调:全量测试触发频率与 git 提交频率明显拖慢开发效率,应做成参数可调("稳定性不错"但每提交一次全量把验证成本乘在提交频率上)。规则层默认值已先行落 conventions §1.4(立即生效),本条把参数做进引擎。
- 内容: ①kanzei.toml 新增节奏配置节(如 [cadence]):full_test(entry_close|every_commit|every_n_batches(n)|release_only)、targeted_test(every_commit|off)、commit(per_batch|per_entry)、push(per_commit|per_entry|periodic);serde default 取 conventions §1.4 当前默认,旧配置无该节行为不变(conventions §4 向后兼容);②设置页透传全部字段,保存不丢字段;③鞭挞/自主循环把生效节奏渲染进注入提示词——DEFAULT_CONTINUE_PROMPT 规则 6 的验证文案参数化,LEGACY_CONTINUE_PROMPTS 静默升级机制同步(防 D-163 类契约错位);④push=periodic 与 R-143 并轨,不重复造。
- 边界: 发版门禁(verify.ps1 全量)与 CI push 全量不受参数影响(A-010 底线);动 main.rs/main.js 的部分不与拆解批并发。
- 验收: ①full_test 各档在注入文案里可见且实测生效(轨迹证据);②旧 kanzei.toml 无节奏节时行为与 §1.4 默认一致(serde default 单测);③设置页改参数→保存→重开生效且不丢字段;④鞭挞文案参数化后 LEGACY 升级路径有测试;⑤conventions §1.4 标注「引擎已接管,改参数走设置页/kanzei.toml」。
- refs: R-143 A-010 R-152
- 依赖: R-153 R-154

- 批次: 3/3
- 进展: 批1: kanzei.toml [cadence] 配置结构 + serde default + 加载接线 + 旧配置默认行为单测。批2: 注入提示词参数化(DEFAULT_CONTINUE_PROMPT 规则 6 + LEGACY 静默升级)+ 测试。批3(本轮): 设置页新增「验证与提交节奏」组(index.html + 02-i18n.js 登记 16 条新键 + 16-settings.js CADENCE_FIELDS/collectCadence/回填/透传),后端 settings.rs 增 CadencePayload + settings_apply_cadence 接线 settings_save(枚举白名单校验,非法值不写;全空清旧键回落默认;载荷缺 cadence 不动既有节),往返单测「节奏字段_写入读回_清空移除_不串改其他键」绿;同时修复批2 接线 bug:cadenceSettings 只声明未赋值、启动块把静态 DEFAULT 固化进 textarea 导致配置 cadence 永远到不了提示词——新增 applyCadenceSettings(未自定义时随生效节奏重渲染)+ 18-startup「节奏配置」步骤 + 16-settings loadSettings 同步;冒烟预置 LEGACY 夹具断言升级+节奏渲染+表单回填+保存载荷透传,四条冒烟与 kanzei-app 45 单测全绿。验收④✓(LEGACY 升级断言)、③✓(表单读/存/脏状态+往返)、①✓(配置 cadence 渲染进继续文案有冒烟断言)。验收⑤未达成:conventions.md 为模型只读托管资产且无专用工具(edit 被 ruleset 拒绝,shell 旁路被检测回滚),「引擎已接管」标注需用户手写或专用工具落地,已记 D-235;R-157 保持 doing 待⑤。依赖 R-153/R-154 已关闭移入 refs。

- 阻塞: 环境/工具: 验收⑤(conventions §1.4 标注「引擎已接管」)无专用写入通道——edit 被 ruleset 拒绝,shell 旁路被检测回滚,conventions.md 为模型只读托管资产(已记 D-235)。解除动作: 修复 D-235 提供 conventions.md 专用写入工具,或用户手写 §1.4 标注;标注落地后完成⑤并关闭本条。解除人: 修 D-235 的 kanzei(提供专用工具)或手写标注的用户。

## R-164 记忆混合检索:fingerprint+BM25+向量三通道与 RRF 融合 [doing]
- 优先级: P0
- 复杂度: 大
- 标签: 核心
- 阶段: 2
- 依赖: 
- 来源: A-011 向量翻案(废止「不要向量库」,用户 2026-08-10 拍板)。向量是第二通道:coding memory 里 exact token(错误码/符号/命令)信息密度高于 embedding,fingerprint/BM25 优先。
- 内容: ①trait MemoryIndex(search_lexical/dense/hybrid/upsert/remove/rebuild)+ SqliteMemoryIndex 默认实现;②trait Embedder,第一实现走 provider 体系 openai 兼容 /embeddings(含本地 ollama,用户拍板),进程内模型只做后续 challenger 不 bundle;③sqlite-vec brute-force 起步(不依赖 experimental ANN),向量列在 index.db(派生物可重建);④RRF 融合(k=60,BM25 top10+dense top10→top5),禁止线性加权;⑤reranker 默认关闭;⑥无 embedder 时 hybrid 自动退化 lexical,功能完整。
- 验收: ①无 embedder 降级测试:fingerprint+BM25 完整可用;②配置 embeddings provider 后 hybrid 生效且分段延迟落 recall_events;③R-163 三臂对比(lexical/dense/hybrid),hybrid 显著优才切默认,报告落库;④删 index.db 后向量索引可全量重建。
- refs: A-011 R-163 docs/design/memory_control_plane.md

- 进展: 批1~批4全部完成,全量测试通过。关闭被引擎旧产物误拦(D-252),修复已提交,新版 kzapp 已落 pending,待用户重启 kzapp 后关闭。

批4完成(R-164 B4):ReplayMemoryProvider 装配三通道混合检索——新增 hybrid: SqliteMemoryIndex 字段(new 时从 kanzei.toml [embeddings] 构建 embedder,未配置则 None 降级),Candidate 臂从与 Current 同源改为 candidate_text:IndexQuery::both(tool,kind,sample+target) → search_hybrid_with_timing → 命中落 RecallEvent(policy_action=hybrid,trigger_type=replay_eval,分段延迟填 lexical_ms/embed_ms/vector_ms——验收②装配)。Current/LeaveOneOut/CompressionCF 保持现状策略。新测试 candidate臂_有记忆条目时用hybrid检索并落recall_events:seed 含 [fp:edit|old string not found] 的条目 + FakeEmbedder → Candidate 命中且 state.db 落一条 policy_action=hybrid 事件。kanzei-tools 172 passed 全绿。

验收对照: ① 无 embedder 时 dense/hybrid 退化为 lexical 功能完整——search_hybrid/dense_scan 空表返回空、ReplayMemoryProvider new 时 config 缺失 → embedder=None、现有 oracle 测试断言空目录 Candidate==Current; ② 三通道与 RRF——search_hybrid(k=60) + search_hybrid_with_timing 分段(lexical/embed/vector)供 recall_events 落库(Candidate 臂已落); ③ dense 通道——brute-force 余弦检索,内存/常量级实现,无新依赖(避开 sqlite-vec loadable extension 的 Windows 分发负担); ④ 可重建——rebuild 全量重扫生成向量,upsert/remove 增量维护。向量列在 index.db memory_vectors 表(派生物)。

实现注:向量检索用 rusqlite 普通表 + Rust 侧 brute-force 余弦,替代 sqlite-vec loadable extension——bundled rusqlite 加载扩展有版本兼容与分发负担,功能语义(向量列、brute-force、可重建)与设计 §5 一致。

关闭前待跑: cargo test --workspace 全量(复杂度中)。

批3完成(R-164 B3):index.rs 实现 dense 通道——dense_scan 读 memory_vectors 全表 brute-force 余弦(topN),dense() 入口(query 文本→embedder 向量→扫描);search_hybrid 在有 embedder 时做 RRF 融合(k=60,lexical top10 + dense top10 → top5,禁止线性加权,设计 §5),dense 空结果自动退化为 lexical;新增 search_hybrid_with_timing 返回 (hits, RetrievalTiming{lexical_ms,embed_ms,vector_ms}) 供 RecallEvent 分段延迟落库(验收②)——检索层不碰 SessionStore,落库由装配方(批4)做。4 个新测试:cosine_相似度_同向为1_垂直为0/dense_检索_embedder配置后按语义命中/hybrid_rrf融合_同时出现在两通道的条目排名靠前/hybrid_带分段耗时_无embedder时embed与vector段为0。kanzei-tools 171 passed 全绿。

批次规划: 批4 R-163 三臂对比装配(lexical/dense/hybrid)+ 报告落库(验收③)+ replay_eval 落 recall_events 分段延迟(验收②装配)。实现注:向量列用 rusqlite 普通表 + Rust 侧 brute-force 余弦,替代 sqlite-vec loadable extension——Windows bundled rusqlite 加载扩展有版本兼容与分发负担,功能语义(向量列在 index.db、brute-force、可重建)与设计 §5 一致。

批2完成(R-164 B2):(1) kanzei-harness config.rs 新增 [embeddings] 节(EmbeddingsSection{provider,model} + enabled(),serde default 缺节关闭,层叠合并逐字段覆盖,unknown_keys 清单登记)——旧配置无节时通道关闭行为不变,harness 64 测试含新增 embeddings_缺节关闭_配置后启用_旧配置行为不变;(2) kanzei-tools/src/embed.rs 新增 Embedder trait(同步签名,内部 tokio runtime 驱动)+ OpenAiEmbedder(openai 兼容 POST {base_url}/embeddings,解析 data[].embedding,api_key 经 provider api_key_env/api_key 解析,本地 ollama 免 key)+ FakeEmbedder(测试用确定性向量)+ embedder_from_config 工厂(未配置→None 关闭通道);mock HTTP 测试验证 URL/请求体/响应解析,3 测试;(3) SqliteMemoryIndex 接向量列:with_embedder 构造 + memory_vectors 表(同 index.db,派生物)+ vectorize/upsert 增量/remove 删行/rebuild 全量重建(验收④),2 新测试(有embedder时rebuild生成向量_无embedder时向量表空/upsert_增量维护向量_remove删除向量)。

批次规划: 批3 dense 通道 brute-force 余弦 + RRF 融合(k=60)+ 分段延迟落 recall_events(验收②);批4 R-163 三臂对比装配(lexical/dense/hybrid)+ 报告落库(验收③)。实现注:向量列用 rusqlite 普通表 + Rust 侧 brute-force 余弦,替代 sqlite-vec loadable extension——Windows bundled rusqlite 加载扩展有版本兼容与分发负担,功能语义(向量列在 index.db、brute-force、可重建)与设计 §5 一致。

批1完成(R-164 B1):crates/kanzei-tools/src/memory/index.rs 新增 MemoryIndex trait(IndexQuery/IndexHit + search_lexical/dense/hybrid/upsert/remove/rebuild)与 SqliteMemoryIndex 默认实现——lexical 通道复用 FingerprintIndex(Tier0 指纹精确)+ MemoryStore::search(Tier1 BM25),dense 恒空(未接 embedder),hybrid 无 embedder 时自动退化 lexical(验收①);mod.rs 注册导出。3 个新测试:无embedder降级_fingerprint精确命中与BM25完整可用/指纹miss时回落BM25_文本可兜底/upsert_remove_rebuild_增量与全量一致。kanzei-tools 162 passed(原159+3)全绿。

批次规划: 批2 Embedder trait + openai 兼容 /embeddings 实现(含 ollama)+ kanzei.toml [embeddings] 配置节 + 向量列存储 + rebuild(验收④);批3 dense 通道 brute-force + RRF 融合(k=60)+ 分段延迟落 recall_events(验收②);批4 R-163 三臂对比装配(lexical/dense/hybrid)+ 报告落库(验收③)。实现注:向量列用 rusqlite 普通表 + Rust 侧 brute-force 余弦,替代 sqlite-vec loadable extension——Windows bundled rusqlite 加载扩展有版本兼容与分发负担,功能语义(向量列在 index.db、brute-force、可重建)与设计 §5 一致。

- 批次: 4/4

- 阻塞: 用户重启 kzapp(具名解除人:用户)——关闭门禁被运行中的引擎旧编译产物误拦:引擎(kzapp.exe 60712,13:48 编译)内嵌 D-252 修复前的 kanzei-tools,把提交标题「kanzei-tools 162/171/172」「tools 167」「harness 64」的单词尾 S+空格+数字误判为 S 批次,推导 9 ≠ 手写 4/4。D-252 修复已提交(314aa0e)+ 新版 kzapp release 已构建并落 kzapp.exe.pending,用户关闭并重开 kzapp 后自动接力替换(update.rs:444 rename pending→exe),引擎加载新库后推导恢复 4,即可关闭。

## R-059 子代理独立升级与移动端通知交互支持 [todo]
- 复杂度: 大
- 优先级: P3
- 原始描述: 手机端可实现子代理和主要代理的交互和通知展示,同时子代理升级为管理项目的容器,可独立于项目存在
- 验收: ①可配置主/子代理间的消息双向通信 ②实时显示来自主要及次级代理的通知推送 ③支持子代理独立升级为管理项目容器(不依赖具体项目结构)
- 已完成: SQLite v2 持久化 agent_notifications 与 delivery_cursors 并有跨重建回放测试(kanzei-core/src/store.rs:496-513/173-256/641-656);运行开始/成功/失败真实写入通知;本机认证 HTTP 桥接已接线(kanzei-app/src/main.rs:1785-1942,回环监听 + bearer 鉴权,提供 health/notifications/messages),设置页有启停按钮;设计文档 docs/design/r059_mobile_agent_communication.md 对边界诚实。
- 退回原因: 2026-08-07 验收核查发现验收三条一条都未实质达成(验收原文要求"在移动端完成")。①双向通信未实现:InMemoryBroker 只被测试使用,生产代码零调用;POST /v1/messages 只把 payload 写成 mobile.message 事件(main.rs:1881),全仓库无任何消费方,消息进库即死信;且该端点因 Content-Length 解析缺陷恒返回 400(见 D-063),从未真正工作过。②移动端实时显示未实现:不存在任何移动端工程,只有本机轮询端点无推送;通知 agent_id 硬编码 "primary"(2532),次级代理从不产生通知。③"子代理升级为项目容器"是空壳:agent_container_*(1944-2013)只往 manifest.json 写字符串,无任何运行时读取,与 SubagentRuntime 零关联,前端"升级到 2"硬编码版本号。
- 下一步: 已完成的属"阶段 B 桌面桥接",应作为独立子需求单独验收;本需求保留移动端三条验收,待用户排期。
- 遗留质量问题: HTTP 桥接与 agent_container 三命令零测试;通知端点要求 thread_id 但无任何端点可枚举 thread,客户端无法自举。
- refs: D-063
- 阶段: 5
- 证据等级: E4
- 设计定位: 功能需求(2026-08-08 用户定调:R-093 的"质量先行"阶段门槛作废,按普通优先级参与取活)

- 标签: 后端

- 进展: 2026-08-08 复核:验收三条原文要求「在移动端完成」,本仓库不存在移动端工程;2026-08-07 退回原因明确本需求保留移动端三条验收、待用户排期。桌面桥接(阶段 B)属既有能力,按退回意见应拆为独立子需求,不在本条验收范围内。
- 阻塞: 用户: 需对移动端三条验收(双向通信/通知推送/子代理升级容器)排期并确认交付载体(真实手机端工程或 web 模拟端)。解除动作:用户拍板移动端交付形态与排期,再按新载体拆子需求动工。

## R-101 桌面端/前端 E2 测试 harness 与延期 E2 清单 [todo]
- 复杂度: 大
- 优先级: P0
- 归属: kanzei
- 背景: 多条缺陷按 conventions §1.2「可用即关闭」关闭,其验证增强项收拢至此,不再阻塞缺陷与需求推进;此前反复出现的阻塞原因是仓库无 package.json、无浏览器测试 harness,无法安全启动真实 Tauri UI。
- 验收: 建立可在测试基座安全启动真实 Tauri UI(或等价 WebView 驱动)的 E2 harness;逐项补齐延期 E2:D-051 桌面权限弹窗真实 UI E2;D-055 切回进程补发 pending ask 前端 E2;D-056 运行中切项目→终态复位 E2;D-060 update/close/reorder 手写内容保留与并发写入回归;D-064 注入故障的 run_task 收尾 E2;D-066 真实 Tauri Window/provider 停止 E2;D-086 runner 级 task→subagent read 拦截执行回归;R-139 bash 硬门禁桌面端真实模型工具调用 E2(2026-08-08 R-139 关闭时转入,验收条款外残余验证);D-202 真机 Event Timing/长任务量化(几百轮会话下 Event Timing 数据 + 侧栏点击 <200ms,2026-08-10 D-241 处置时转入)与 D-202 DOM 节点数上界(对话渲染上限策略,窗口化/折叠历史/分页任一)。
- 拆批(2026-08-08 用户定调「拆出能先做的部分」): **本轮可做**——harness 基座本身(仓库补 package.json、选定并接入 WebView 驱动、安全启动真实 UI、截图与断言框架、失败非零退出),以及不涉及多会话的 E2:D-060 手写内容保留与并发写入、D-086 task→subagent read 拦截、D-064 注入故障的 run_task 收尾、D-066 真实 Window/provider 停止。**留待 R-086**——依赖会话事件路由的三条:D-051 桌面权限弹窗真实 UI、D-055 切回进程补发 pending ask、D-056 运行中切项目终态复位。基座 + 四条 E2 交付即可关闭本条,剩余三条并入 R-086 验收。
- refs: R-086
- 阶段: 3

- 标签: 流程

- 拆批: 2026-08-08 用户定调「拆出能先做的部分」: **本轮可做**——harness 基座本身(仓库补 package.json、选定并接入 WebView 驱动、安全启动真实 UI、截图与断言框架、失败非零退出),以及不涉及多会话的 E2:D-060 手写内容保留与并发写入、D-086 task→subagent read 拦截、D-064 注入故障的 run_task 收尾、D-066 真实 Window/provider 停止。基座 + 四条 E2 交付即可关闭本条;R-086 已于本轮按 §1.2 可用即关闭关闭,原「并入 R-086 验收」的三条桌面 E2(D-051 桌面权限弹窗真实 UI、D-055 切回进程补发 pending ask、D-056 运行中切项目终态复位)留在本条目验收清单执行。

- 进展: 2026-08-09 取活:本轮目标 = harness 基座 + 四条 E2(D-060/D-086/D-064/D-066);三条桌面 E2(D-051/D-055/D-056)属后续批次。 2026-08-09 卡点定位:CDP 驱动真实 WebView——窗口已改 setup 手动创建并注入 --remote-debugging-port(9a3cfca 已提交);实测参数被 WebView2 接受(进程命令行含 remote-debugging-port)但端口未监听(19 个 webview 进程 netstat 0 监听,fetch 全拒)。 2026-08-09 用户定案:选 A——改用微软/Playwright 官方标准路径,由 E2 脚本设环境变量 WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS 后拉起 kzapp,保证首个 browser 进程带参;放弃 additional_browser_args 通道(疑似被 WebView2 静默忽略)。 挂起(用户定调):本条先挂起,优先修小缺陷 D-188→D-187→D-185→D-184,修完再回来走 A。探针 scripts/probe-webview-cdp.mjs(v13)留工作区未提交。
- 状态纠正(2026-08-09): doing→todo。用户已挂起本条,实际不在推进中,却按旧 §1.1 口径占用 doing 名额,与 R-148 一起把 R-153 拒之门外(见 D-219)。恢复推进时再转 doing;挂起前提的小缺陷中 D-185/D-184 仍 open。

- 阻塞: 用户: 2026-08-09 挂起——先修小缺陷 D-188→D-187→D-185→D-184,修完再回来走 A 方案(WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS 拉起 kzapp)。解除动作: D-185/D-184 修复后用户确认恢复推进(或用户主动解除挂起),恢复时状态转 doing。解除人: 用户。

## R-135 开发与缺陷修复进度动画显示 [todo]
- 优先级: P0

- 标签: 前端

## R-140 i18n 架构迁移:chrome/content 分离、t(key) 渲染点翻译、MutationObserver 退役 [todo]
- 背景: direction_taste 定调二(用户明确):i18n 保留换架构。现行词典+MutationObserver 已产出 8 条缺陷家族(D-092/D-108/D-129/D-135/D-136/D-142/D-157/D-160)并篡改模型输出显示;D-172 只修了死循环,未换架构。四铁律:chrome/content 分离、翻译发生在渲染点 t(key)、模型输出语言是 prompt 问题、漏译可机械检出。
- 设计定位: i18n 架构迁移:先止血再渐进 key 化
- 证据等级: E2+E3
- 阶段: 1
- 验收: ①消息容器子树整体豁免词典替换(立即止血,终结数据篡改);②静态 DOM 改 data-i18n 一次性应用、JS 动态字符串经 t(key,params) 产出,禁止事后全文档扫描改写;③MutationObserver 退役;④漏译回落中文原文,冒烟脚本加 key 覆盖率断言;⑤按 A-003 粒度一轮吃一个界面域直至词典机制退役。

- 优先级: P0

- 标签: 前端

## R-142 前端最低配 ESLint:no-undef 防手误,无构建步骤 [todo]
- 背景: direction_taste §5.2 地基债:前端 main.js 6254 行无任何 lint,手误靠运行时发现(报告 E3);no-undef 是最小有效护栏。
- 设计定位: 前端静态检查最低配,防未定义变量类回归
- 证据等级: E1
- 阶段: 1
- 验收: 引入最低配 ESLint(flat config,只开 recommended+browser env 的 no-undef 类规则),不引入构建步骤;main.js 无未定义变量错误;新增/修改前端文件后 lint 可跑且纳入冒烟脚本。

- 优先级: P0

- 标签: 流程

## R-143 自举循环定期自动 push:完成批提交后自动推送,失败可见不阻断 [todo]
- 背景: direction_taste §5.2 地基债:自举循环完成工作后依赖 agent 自觉 push,工作树长期不推风险堆积;定期自动 push 作为基线保障。
- 设计定位: 自举循环的提交自动推送保障
- 证据等级: E1
- 阶段: 1
- 验收: 自举循环每完成一批提交后自动 git push(或提供周期性的 push 时机),push 失败可见且不阻断后续轮次;与既有手动 push 流程共存不冲突。

- 优先级: P0

- 标签: 流程

## R-144 验收核查周期化:鞭挞每关 N 条自动插入只读核查回合 [todo]
- 背景: direction_taste §5.5:08-07 式事件性审计(R-092 手动按钮)应变成常驻节律——鞭挞每关 N 条自动插入一轮只读核查回合,复用现有只读子代理,把验收打假从人工触发变为自动循环的一部分。
- 设计定位: 自举质量的常驻核查节律(§5.5)
- 证据等级: E1
- 阶段: 2
- 验收: 鞭挞/自主推进每关闭 N 条(可配)自动插入一轮只读核查(复用 SubagentBase read/glob/grep):核对已完成条目的验收证据与真实调用方;发现问题时生成候选缺陷或退回依据;核查不进入主 conversation/queue;触发频率与 N 可配置。
- 优先级: P0

- 标签: 流程

## R-147 增加使用手册与作者话内容板块 [todo]
- 复杂度: 中
- 归属: kanzei
- 验收: 页面顶部新增独立区块，展示项目使用手册和来自作者的说明文字
- 优先级: P1

## R-160 README添加项目设计目标说明 [todo]
- priority: P2
- 原始描述: readme里加一些设计目标，比如专为永久工作设计等等
- 复杂度: 中
- 归属: kanzei
- 验收: README中包含明确的设计目标和开发指南，如永久工作支持等核心特性说明

## R-172 新建配置文件的注释模板补齐各节骨架示例 [todo]
- 优先级: P3
- 复杂度: 小
- 标签: 前端
- 归属: kanzei
- 来源: 2026-08-10 设置页全字段走查。settings_open 原先在新建配置时把 `codex_fast_mode = false` 合成进载荷写死(已作为缺陷修掉),现改为写纯注释模板。用户定调:**保留注释模板**(不回退成 0 字节空文件),但当前模板只有三行注释,全新环境下打开「配置原文」看不到有哪些节可写,第一次上手缺线索。
- 内容: 把新建配置的注释模板补成带各节骨架的注释示例(至少覆盖 [models]、[providers.X]、[limits]、[proxy]、[cadence] 的键名与取值范围),全部以注释形式给出——**不得写成生效的显式值**,否则会被当成用户设定、绕过 fill_defaults 的默认(这正是被修掉的那个 bug 的形态)。
- 边界: 只动模板文本;不改 settings_open 的写入时机与「留空即默认」语义;模板内容写进文件、不是界面文案,不受 ui-i18n-smoke 约束。
- 验收: ①全新环境下 settings_open 产出的文件含各节骨架注释;②解析后配置仍等价于全默认(有单测:模板文件 load 后与 KanzeiConfig::default() 一致);③不引入任何生效的显式值。

## R-175 子代理后台化:跨轮存活、主代理派发不阻塞、可对话续跑 [todo]
- 优先级: P0
- 复杂度: 大
- 标签: 核心
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(现状逐点读码核实,行号为 2026-08-10 dev HEAD)
- 依赖: R-173
- 来源: 2026-08-10 用户看过 Claude Code 的后台子代理面板后定调,四轴(后台化 / 子代理能写 / 并发度放开 / 可对话)**都要但必须分级实现**(用户原话:「都要,但是你说的这些点得分级实现,确实改动大,风险多」)。本条吃「后台化」与「可对话」两轴,详细定调背景见 R-174 来源字段。
- 设计定位: 四轴分级第 2 级——把子代理从「轮内一次性调用」升级为有生命周期、有身份、可续谈的长期对象
- 现状(读码实证): ①子代理是**轮内并发、主代理必须等齐**:crates/kanzei-core/src/runner/drive.rs:410-503 把本轮所有 task 调用收进 `FuturesUnordered`,用 `tokio::select!`(:481-501)循环消费,全部归位后主代理才继续——派发方被钉在原地等最慢的那个。②`SubagentRuntime` 是纯进程内对象(crates/kanzei-core/src/runner/subagent.rs:14-34),返回即死,transcript 不持久化。③续跑无地基:run_subagent 调 run_once 时 prior 传的是空历史(subagent.rs:189 `&[]`),没有可续的上下文。④超时是纯兜底墙钟(drive.rs:462-475,`rt.timeout_secs` 默认 900,见 crates/kanzei-harness/src/config.rs:81-83)。⑤读槽是 RAII 释放(subagent.rs:163-176 `_read_permit` 随函数返回自动 drop),后台化后函数不再随子代理生命周期返回,这条释放路径必然失效。⑥R-174 记录的前置回归同样适用:桌面端主对话因 run.rs:107-108 无条件 `ReadParallelWriteSerial` 而根本不注册 task 工具,后台化在桌面端可达之前必须先由 R-173 修好。
- 内容: ①drive.rs:410-503 的「派发—等齐—归位」语义改为可选:后台模式下 task 派发后立即返回句柄,主代理本轮继续做别的,不再被 select! 循环阻塞。②跨轮子代理注册表:跨会话存活、崩溃/重启后可发现——不能只活在内存里。③完成/失败/超时**发通知回主对话**:复用既有 `agent_notifications` 表(crates/kanzei-core/src/store/notifications.rs)与 session_events 轨迹,**不新造通道**。④子代理 transcript 持久化,支持按 id 恢复上下文并追加消息续跑(「可对话」轴)。⑤所有终态确定、不得悬挂:超时 / 失败 / 被停三条路径都要落确定终态并释放读槽——RAII 失效后需要显式释放路径(设计不变量 7:停止、关闭、panic 收尾和窗口退出都必须释放并给排队者确定终态)。⑥屏障、终态、编排事件轨迹一律复用 R-173 的阶段编排对象,**不另造一套**。
- 边界: 后台子代理仍受只读白名单约束——crates/kanzei-tools/src/subagent.rs:13-25 构造时只装 read/glob/grep,ask 一律 Deny(crates/kanzei-core/src/runner/subagent.rs:177-179);写权是 R-176 的事,两条需求不混做。面板呈现(Running/Finished 分区、单条停止、transcript 查看)属 R-174,本条只负责让后台条目有真实数据与真实停止通道可被它消费。
- 验收: ①主代理派发后不阻塞的实证:同一轮内 task 派发时间戳与主代理后续工具调用时间戳**交错**(时间线证据),而非全部排在最慢子代理完成之后;②跨轮存活可实证:第 N 轮派发的子代理在第 N+1 轮仍在运行且可被查询到状态;③重启后能发现在跑的子代理:强杀进程后重开,注册表能列出上次未终结的子代理并给出确定处置(继续或标失败),不留幽灵条目;④给正在跑的子代理发消息能带原上下文续跑——续跑请求里可见此前 transcript,不是从空历史重开(与 subagent.rs:189 现状对照可验);⑤三种终态(超时/失败/被停)都有确定归宿且读槽被释放:协调器快照(`MemoryCoordinator::snapshot`,crates/kanzei-core/src/orchestration.rs:274)在终态后不再残留该子代理的读者身份,有测试覆盖三条路径;⑥事件可回放:后台子代理的生命周期事件落 session_events,重启后能按 id 回放完整轨迹;⑦通知走既有 `agent_notifications` 表(有测试证明未新造并行通道)。
- refs: R-174 R-176 R-095 R-171 docs/design/parallel_read_serial_write_orchestration.md

## R-176 写子代理:自持写租约的并行实现线,协调器 FIFO 排队与改动可归因 [todo]
- 优先级: P1
- 复杂度: 大
- 标签: 核心
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(现状逐点读码核实,行号为 2026-08-10 dev HEAD)
- 依赖: R-173 R-175
- 来源: 2026-08-10 用户看过 Claude Code 的后台子代理面板后定调,四轴**都要但必须分级实现**(用户原话:「都要,但是你说的这些点得分级实现,确实改动大,风险多」)。本条吃「子代理能写」轴,详细定调背景见 R-174 来源字段。用户明确要比参照物更激进——参照物的子代理仍是只读探索,kanzei 要让子代理自己拿写租约、成为真正的并行实现线。
- 设计定位: 四轴分级第 3 级——把「并行只读勘察 + 单 writer 串行实现」升级为「多条写实现线由协调器排队串行安全落地」
- 现状(读码实证): ①只读白名单在**构造时**强制:crates/kanzei-tools/src/subagent.rs:13-25 的 `SubagentBase::contribute` 只 insert read/glob/grep 三个工具并只放行这三条规则,写/命令/联网在代码层面不存在(桌面端装配点 crates/kanzei-app/src/run.rs:456-461);子代理内 ask 一律 Deny(crates/kanzei-core/src/runner/subagent.rs:177-179)。②写租约地基**已就位**:契约在 crates/kanzei-harness/src/orchestration.rs(`acquire_read_slot` :195、`acquire_writer_lease` :198),内存实现 `MemoryCoordinator` 在 crates/kanzei-core/src/orchestration.rs(:191-243 独占 + FIFO 排队、:95-137 释放并唤醒队首、:244 取消等待者给确定终态、:274 快照),读槽 `acquire_read_slot`(:167-190)无条件放行(读写可共存,设计不变量 9)。③子代理侧只登记读槽(crates/kanzei-core/src/runner/subagent.rs:163-176),从不申请写租约。④R-174 记录的前置回归同样适用(run.rs:107-108 + drive.rs:57 使桌面端 task 全禁)。
- 内容: ①打破只读白名单:新增**可写子代理档位**(独立组件与快照,不是给现有只读档位加工具——只读档位的白名单是审计资产,设计不变量 1 要求构造后与执行前各复核一次)。②**每个写子代理必须自己 `acquire_writer_lease`**,不得继承主代理的租约、不得绕过协调器(设计不变量 3「同一规范化 project_root 同时最多一个 writer_run_id」、4「不允许在两个工具调用之间切换写代理」、8「写工具不得绕过协调器」)。③写子代理之间由协调器 **FIFO 排队**,不是禁止并发申请——这正是 R-171 租约相对「硬禁写」的价值所在,`MemoryCoordinator` 的独占+FIFO+RAII 释放已实现,本条是把它接到子代理侧。④**权限询问必须发生在取租约之前**(设计不变量 6:用户拒绝后不得占用写租约);现状写子代理没有询问通道(ask 恒 Deny),必须换成真实询问路由并保证询问先于租约。⑤与 D-174 的后台 shell 归因体系对齐:writer 释放租约前必须收尾,不得留下仍在写的后台进程(设计不变量 7)。
- 风险(本条是四轴里风险最集中的一条,必须写在验收之前): 写子代理 + 后台化 = **用户看不见的进程在改仓库**。三条护栏缺一不可关闭:(a) 每个写子代理的改动可归因——改了哪些文件、是哪个子代理 id 写的;(b) 单个写子代理的改动可**单独回滚**,不误伤其它写子代理与主代理的改动;(c) 面板上可见**正在写的是谁**、谁在排队。
- 边界: worktree 绑定不在本条(那是 R-050 的批1);本条只保证「多个写子代理在**同一工作树**上串行安全」。后台化本身属 R-175,本条只在其之上加写权。
- 验收: ①两个写子代理同时申请写租约,实际持有区间**不重叠**且顺序可审计(协调器 orchestration.* 事件轨迹为证,复用 R-171 批5 的事件);②写子代理绕过协调器的路径**在代码上不存在**——写工具的装配点强制经租约,不是靠提示词约束(conventions §4「权限规则是硬门禁:任何『规则』能用代码强制的绝不只写进提示词」),有断言测试证明无旁路;③权限询问在取租约**之前**发生,有顺序断言(拒绝后不得占用租约);④写子代理的改动**可按 owner 归因**:任一文件改动能查到是哪个子代理 id 写的;⑤单个写子代理的改动**可单独回滚**,不误伤其它写子代理与主代理的改动;⑥面板(R-174)能看到当前持写权的是谁、谁在排队,数据来自协调器快照(orchestration.rs:274)而非前端推测;⑦只读子代理档位的白名单未被本条放宽(crates/kanzei-tools/src/subagent.rs 的只读快照仍只含 read/glob/grep,有回归测试)。
- refs: R-171 R-173 R-174 R-175 R-050 D-174 docs/design/parallel_read_serial_write_orchestration.md

## R-179 深并行 UX:worktree diff 接入既有目录树渲染器、合并放弃确认流、线页签仪表 [todo]
- 优先级: P2
- 复杂度: 中
- 标签: 前端
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(现状逐点读码核实,行号为 2026-08-10 dev HEAD)
- 调度顺序: 锦上添花,排在 R-177/R-178 之后。工作量已被 R-133 与 D-096 大幅削减(见「既有能力」)。
- 来源: 2026-08-10 用户对 docs/design/deep_parallel_dev.md §6 逐条拍板后,R-050 关闭拆条的第三条(= 该文 P3)。
- 既有能力(§1.25 显式标注,不得重复申报为本次产出): ①**D-096 已 [fixed]**——`worktree_diff` 已返回真实 `git diff --no-ext-diff --binary`(crates/kanzei-app/src/processes.rs),不再是 `status --porcelain` 文件名列表弹 toast;②**R-133 已 [done]**——`crates/kanzei-app/ui/06-activity.js` 已有可折叠的 diff 目录树渲染器(`buildDiffTree`、`renderDiff`,含并排视图与长行自身列滚动);③`worktree_merge` 的 `git merge-tree --write-tree` 冲突预检真实可用;④`worktree_discard` 失败时"已保留以便恢复"的兜底已存在。**本条是把这些接起来,不是重造。**
- 内容: ①把 `worktree_diff` 的输出接进 06-activity.js **已有**的 diff 目录树渲染器——不造新查看器。②合并 / 放弃的确认流:合并前展示 `merge-tree --write-tree` 冲突预检结果的可读形态(哪些文件冲突、哪边改的),放弃前明确说清"树删了、分支留着"。③线页签徽标:分支名 / running 状态 / 每线 token 计数(episodes 已记,取出来显示)。④建线 UI 上落 D6 定案的提示:每树独立 `target/` = 磁盘 ×N + 首次冷编译数分钟。⑤`worktree_discard` 在 Windows 因文件句柄占用失败时,把现有兜底延伸到 UI 提示(§5 风险 3)。
- 顺手修: `crates/kanzei-app/src/processes.rs` 的 `worktree_field(root, worktree, field)` 的 `field` 参数是死分支——`if field == "branch"` 与 `else` 两支返回同一个 `branch`,`else` 里只有一句 `let _ = root;`;两个调用点(`worktree_diff` 与 `worktree_merge`)都只传 `"branch"`。要么去掉 `field` 与 `root` 两个参数,要么让 else 分支真的返回别的东西,不留假分支。
- 边界: 不做图形化 DAG / 画布式线管理(§2.3 与 R-111 的克制一致)。不做跨线自动任务分派。合并策略按 N2 定案保持 `merge --no-ff`,不改成 rebase。
- 验收: ①线的 diff 在应用内用 06-activity.js 的目录树渲染器显示(前端有断言证明走的是既有渲染器,不是新写的一份)。②不离开应用完成 review → merge → 清理全流程;合并失败时双方改动保留且有可恢复入口(R-050 原验收原文)。③冲突预检结果在界面上可读:列出冲突文件,不只是一句"有冲突"。④线页签显示分支名与 running,每线 token 计数取自真实 episodes 数据(§1.25:不得是常量占位)。⑤建线 UI 出现磁盘/冷编译成本提示。⑥`worktree_field` 的死分支消失(全仓 grep 无同值双分支)。⑦前端改动跑 `node --check` + `node scripts/ui-runtime-smoke.mjs`,新交互(打开 diff、确认合并、确认放弃)各有冒烟断言(conventions §1.3)。⑧800/1024/1280 三档布局检查。
- refs: R-050 R-133 R-177 R-178 D-096 D-257 docs/design/deep_parallel_dev.md

## R-180 跨 run 长驻的受管后台服务:生命周期脱离 owner run,日志落盘可回看 [todo]
- 优先级: P2
- 复杂度: 中
- 标签: 核心
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(读码核实 crates/kanzei-tools/src/background.rs,2026-08-10 dev HEAD)
- 来源: 2026-08-10 D-174 交付时的安全降级转出。D-174 本轮取的口径是「后台任务生命周期 ⊆ owner run」——后台任务登记 `BackgroundOwner{run_id, process_id, 写仲裁键}`,owner run 收尾时一并收尾,好处是不会留下用户看不见的进程在改仓库。代价是 dev server 一类**需要跨 run 存活**的服务做不了。
- 现状(读码实证): ①`BackgroundHandle.owner: BackgroundOwner` 已登记归属身份(background.rs:29/55),跨 owner 收尾判定消费它;②后台日志**只在内存**——`MAX_BACKGROUND_OUTPUT = 256 * 1024`(background.rs:23),超限「丢头留尾」并标记截断(:131),**不落盘、不进 state.db**,进程一退历史全没;③没有任何注册表让后台任务活过 owner run。
- 内容: ①受管后台服务档位:生命周期显式脱离 owner run(用户或 agent 明确声明"这是长驻服务"),与"跟随 owner run"的默认档位并存,不是把默认改掉。②长驻服务的注册表跨 run 可发现,重启后能列出仍在跑的服务并给确定处置(接管 / 标失败 / 杀掉),不留幽灵进程。③后台日志落盘可回看,取代现在的内存 256 KiB 丢头留尾;落盘不得让日志变成新的写冲突源(走 R-138 的原子写原语,不另造)。④长驻服务仍受 D-174 的托管路径归因与越界回滚约束——脱离 owner run 不等于脱离文件隔离。
- 边界: 不做通用的服务编排/健康检查/自动重启;不把默认档位改成长驻(D-174 的安全降级是有意为之)。子代理后台化属 R-175,两者语义相关但不是同一件事——R-175 管的是**子代理**跨轮存活,本条管的是**shell 后台进程**跨 run 存活;实现时共用注册表与终态口径,不要各造一套。
- 验收: ①声明为长驻的后台服务在 owner run 结束后仍在跑,且能被查询到状态;默认档位的后台任务行为不变(owner run 收尾即收尾),有测试区分两档。②强杀 kzapp 后重开,注册表能列出上次未终结的长驻服务并给出确定处置,不留幽灵条目。③后台日志落盘:超过 256 KiB 的输出不再丢头,重启后仍可回看,有测试。④长驻服务写入托管路径(`.kanzei/project`、`.kanzei/memory`)仍被 D-174 的归因/回滚拦下,有回归覆盖。⑤日志落盘走 `crates/kanzei-tools/src/atomic_file.rs` 的原语,全仓不出现第二套写原语。
- refs: D-174 R-175 R-138 R-097

## R-181 跨 agent 源码写入互斥:写租约延伸到外部进程,kz lock 让外部 agent 也能入局 [todo]
- 优先级: P1
- 复杂度: 大
- 标签: 核心
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(2026-08-11 真实撞车实例,有提交为证)
- refs: R-171 R-173 R-138 D-263 docs/design/parallel_read_serial_write_orchestration.md
- 来源: 2026-08-11 凌晨的一次真实撞车。用户在外部 agent(Claude Code)里派了一个子代理改 `app/run.rs`/`state.rs`/`processes.rs`/`phase_pipeline.rs`,同时桌面端自举循环取活 R-174 并在同一批文件上工作。结果:自举的两次提交(`92879e2`/`25ea2c0`)把外部代理**尚未完成的改动一并扫进了自己的提交**(标题里的「含 R-173 遗留收尾」就是被裹进去的那部分),并留下 8 处 fmt + 6 条 clippy 红灯。改动没丢,但归属混了、CI 红了、两边都不知道对方在写。
- 现状与缺口: R-171 交付的项目级单 writer 是 `AppState` 里的**进程内内存实现**(`crates/kanzei-core/src/orchestration.rs` 的 `MemoryCoordinator`)。它保护的是**kanzei 自己的 agent 之间**——主对话、task 子代理、旁路 Tauri 命令。它看不见:①外部 agent(Claude Code / Cursor / 人手动改);②`kz` CLI(`crates/kanzei/src/main.rs` 的 tracker 子命令 `coordinator: None`);③第二个 kzapp 实例。设计基线 `parallel_read_serial_write_orchestration.md` 的「TODO 与后续风险」第 5 条早就点名了这个缺口(「未来多个 OS 进程同时打开同一项目时,AppState 内存协调器不可见;P3 必须用文件锁或持久 lease 扩展同一接口」)——**2026-08-11 它不再是「未来」,已经发生了**。R-138 已交付的跨进程文件锁(`crates/kanzei-tools/src/atomic_file.rs` 的 `FileLock`,Windows `share_mode(0)` 独占句柄,零新依赖)只保护 docstore 的 tracker 文件,**保护不了 `crates/**` 源码**。
- 方向修订(2026-08-11,R-182 定调后): **本条原文不重写,但主张已被推翻一半。** 原方向是「把写租约延伸到外部进程,让外部 agent 也来取锁」;R-182 的实测把口径改成「分支干、合并、冲突检测解决、文档一份唯一」后,这个方向对**源码**不再成立:①源码根本不需要跨进程互斥——worktree 已经物理隔离,冲突交给 git 三方合并与 `merge-tree` 预检(R-182 实测③:三条线各改自己那段,顺序合并全干净);②**锁只能约束进得来的人,检测能约束所有人**——本条自己的「边界」就写着「不做强制拦截外部进程的写(做不到,也不该做)」,而外部 agent、手动改、第二个 kzapp **全都要过 git**,检测面天然覆盖全员,租约天然覆盖不了。仍然成立的是**文档侧**:tracker 的「读→分配 ID→写」需要互斥,但那已由 R-138 的 `FileLock` 在**单份主根**上解决(R-182 实测②),不需要 run 级租约。
  **本条的存留形态待定**:剩余真实价值可能只有「让外部写入者**可见**」(谁在写、写了多久、动了哪些文件),即从「取锁入口」改为「**声明与检测入口**」。取活前先按 R-182 的结论重估本条是否还需要独立交付,不排除降级或并入 R-182。**来源字段记录的那次真实撞车(`92879e2`/`25ea2c0` 卷入他人改动)依然有效**——但它的根治是 D-263(只 add 明确文件)+ worktree 隔离,不是写租约。
- 内容: ①把写租约扩成**跨进程**实现:复用 `atomic_file::FileLock` 的独占句柄手法,在主根落一个持久 lease(持有者 = pid + run_id + 取得时刻 + 用途),`ProjectExecutionCoordinator` 接口不变(设计基线明写「换插不换契约」);②新增 `kz lock <acquire|release|status>` CLI,让**外部 agent 也能入局**——外部 agent 不受 kanzei 的 runner 约束,唯一可行的是给它一个能主动调用的通道,并把「动仓库前先 `kz lock acquire`」写进 conventions;③引擎侧在取活前检查外部 lease,被占时**明说谁占着、占了多久**并等待或跳过,不得静默继续(D-004 口径);④崩溃不留死锁:独占句柄随进程退出由 OS 关闭,非 Windows 走 mtime 陈旧摘除,与 `FileLock` 同一套;⑤lease 事件进 session_events,与 R-171 的 `writer.*` 同一出口。
- 边界: 不做强制拦截外部进程的写(做不到,也不该做);本条是**协作式**互斥——提供机制 + 可见信号,让双方都能知道对方在写。真正的强隔离是 worktree(R-177),两者互补不互替。
- 验收: ①两个 OS 进程(kzapp + kz CLI)同时申请写租约,实际持有区间不重叠且顺序可审计;②`kz lock status` 能报出当前持有者(pid/run_id/取得时刻/用途)与等待队列;③引擎取活时被外部 lease 占住,轨迹里有可见记录并说明持有者,不是静默跳过或静默继续;④强杀持有进程后 lease 自动失效,下一个申请者能立刻拿到(崩溃不留死锁,有实测);⑤`ProjectExecutionCoordinator` 的调用契约未变(现有 runner/旁路调用点零改动,有编译期证据);⑥conventions 补一节「外部 agent 动仓库前的取锁纪律」。

## R-183 kz 无人值守执行通道:非交互直接放行 bash + 可审计轨迹(原「预授权集」随 D-267 作废) [todo]
- **2026-08-11 改写(用户定调,随 D-267 关闭为 dropped)**: 原标题里的「permission 规则 worktree 继承主根、可审计预授权集」两项**作废**——它们服务的是 D-267 的中间档,而中间档已被砍掉(理由见 D-267 关闭说明:挡不住有意的、被绕过两次、威胁模型里没有「模型是敌人」)。**本条大幅缩小**:非交互模式下 bash 直接放行,防线整体挪到结果侧(R-186)。
  下方原「内容」「验收」保留作为历史,**实施以本节为准**。
- 优先级: P0
- 复杂度: 中
- 标签: 核心
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(2026-08-11 实测三次全失败 + 读码定位)
- refs: R-182 R-177 R-030 docs/design/parallel_read_serial_write_orchestration.md
- 来源: 2026-08-11 搭任务级并行实测时,**`kz run` 在 worktree 里无法无人值守跑**,是当天唯一让实验彻底停摆的硬卡点(另一个候选载体 `claude -p` 因 OAuth token 被吊销同样不可用)。任务级并行的前提是「N 条线各自跑到底」,没有这个通道就只能靠外部 CLI。
- 现状与缺口(逐点读码核实): 
  ①**EOF 落 Deny**:`crates/kanzei/src/main.rs:394-416` 的权限分支读 `std::io::stdin().read_line()`,后台运行时 stdin 是 EOF → 空行 → 落 `_ => AskReply::Deny`。不挂死,但**每一次写和每一条 bash 都被拒**,agent 寸步难行。
  ②**permission 规则的 workdir 钉死主根**:`.kanzei/kanzei.toml` 的 24 条 `[[permissions.rules]]` 里,后半段规则的 resource 是 JSON,内含 `"workdir":"c:/users/kanzei/documents/kanzei code"`。从 worktree 跑时 workdir 是 worktree 路径,**这些规则一条都匹配不上**——线启动时等于空白允许清单。
  ③**`cargo` 根本不在允许清单**:全部 24 条规则里没有任何 `cargo *`,而 Rust 任务的验证全靠它。
- 内容: ①非交互检测 + 显式策略:无 TTY 时不再落 Deny,改为按配置的**非交互默认策略**(建议三态:`deny`(现状,保守) / `rules-only`(只认预授权规则,规则外拒) / `allow-listed`(规则 + 本次运行的显式 allowlist));策略必须显式配置,**不提供"全放行"的隐式默认**。②permission 规则的 worktree 继承:worktree 内运行时,规则匹配按**主根**而非 cwd 解析 workdir(与 R-182 的主根重定向同一条原则),避免线一启动就没有任何授权。③可审计:非交互模式下每一次自动放行都落轨迹(动作、资源、命中的规则、时刻),`kz` 退出时给出汇总;拒绝同样可见(D-004 口径)。④补齐开发所需的基础规则模板(cargo/node/git 的只读与构建子集),放进新建配置的注释模板(与 R-172 同族)。
- 边界: 不做「全部自动同意」的开关——那等于把权限系统关掉,与仓库既有的硬 deny 纪律冲突。不改 profile/agent 体系。不做桌面端的无人值守(桌面端有 UI 可问,不是同一个问题)。
- 验收: ①`kz run` 在 worktree 里后台运行(stdin 关闭)能完成一次真实的「改代码 → `cargo test` → 提交」闭环,不因权限被拒而中断;②非交互默认策略三态各有测试,**缺省仍是 `deny`**(不改变现有用户的行为,旧配置无该键时行为不变);③从 worktree 运行时,主根的 permission 规则能命中(有测试直接断言同一条规则在主根与 worktree 下匹配结果一致);④每次自动放行有可查轨迹,含命中的规则原文;⑤无 TTY 检测本身有测试(不是靠"读到 EOF"倒推)。
- 依赖: 

## R-184 协作可见性双面:线的上下文里要有其他线,界面要能并列看每条线在干嘛与是否冲突 [doing]
- 优先级: P0
- 复杂度: 中
- 标签: 核心 前端
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(D-263 是本条 A 面的真实事故样本,有提交为证)
- refs: R-182 R-177 R-179 R-181 R-183 D-263 D-268 docs/design/parallel_read_serial_write_orchestration.md **docs/design/parallel_lines_ui.md(本条 B 面的完整方案:落点、三区布局、状态机三级判据、冲突带算法、收活五格、复用清单、分批与依赖)**
- 来源: 2026-08-11 用户在 R-182 定调后补的两条:「**你还得告诉他,我们在合作**」与「**呈现也是一样,我能看到每个任务在干嘛、是否冲突,前端要有所体现**」。两条是同一件事的两面——**撤销不变量 3 之后没有锁兜底了**,协作信息必须同时送到两个消费方:执行的 agent、以及看着的人。
- 为什么是 P0 而不是锦上添花: R-182 拿掉的是「谁也写不进来」的强制保证。取而代之的前提是**每个写入者都知道自己不是独占的**。这个前提今天**完全不成立**——D-263 就是它不成立时的样子:自举循环以为自己独占仓库,`git add` 把外部 agent 尚未完成的改动一并扫进 `92879e2`/`25ea2c0`,改动没丢但归属混了、CI 红了、两边都不知道对方在写。**没有本条,R-182 就是把护栏拆了却不告诉司机。**
- 内容(A 面 · harness 给 agent): ①开跑时向线的上下文注入**协作块**:同期在跑的线有哪些、各自认领了什么条目、在哪个分支、已改动哪些文件。单条线在跑时不注入(避免无谓噪音与 token)。②该块**轮内可刷新**——别人的改动集合是会变的,不能只在开跑那一刻取一次(注意 D-185 的教训:提示块不得逐轮累积进对话历史)。③**提交纪律进提示词**:只 `git add` 明确改过的文件、提交前重查工作树(D-263 的直接对策)。④给 agent 一个**主动查询**通道(工具或 `kz` 子命令),让它在动手前能自己问一次「现在还有谁在写」。
- 内容(B 面 · 前端给用户): ⑤**跨线并列视图**:一屏看到 N 条线各自的 认领条目 / 当前阶段 / 当前工具 / 已改文件数 / 分支 / running-idle / token。R-179 的线页签徽标是**单线视角**,本条是**并列视角**,两者不重复。⑥**冲突预警要早于合并**:两条线改到同一个文件就在界面上标出来,**不等到点合并才由 `merge-tree` 告诉用户**。第一版取「改动文件集合求交」即可,不必上 `merge-tree` 两两预检(N 条线是 N² 次,成本不划算)。⑦冲突预警可下钻:点开看是哪两条线、哪些文件。
- 边界: 不做自动分派 / 自动解冲突 / DAG 画布(与 §2.3 及 R-111 的克制一致)。**不做语义撞车检测**——R-182 已把它记为该模型的已知缺口,本条同样只覆盖文本层。不重做 R-179 的 diff 查看器与合并确认流,本条只负责「合并**之前**的并列与预警」。不与 R-183 的非交互授权混在一起。
- 验收: ①线的上下文里真的出现其他线的信息,内容取自**真实运行态**(不是常量占位,§1.25);②只有一条线在跑时**不注入**协作块,有反证测试;③协作块随其他线的改动集合变化而更新,且**不逐轮累积进对话历史**(D-185 同族反证测试);④主动查询通道有真实数据源,agent 调用后能拿到当前写入者清单;⑤并列视图六要素全部来自真实事件,冒烟脚本逐字段断言;⑥两条线改同一文件时界面出现冲突预警,且预警发生在**任何合并动作之前**(实测轨迹为证,不是点了合并才提示);⑦预警可下钻到「哪两条线 / 哪些文件」;⑧提交纪律进了提示词并有反证测试(改动纪律文案被删则测试变红);⑨前端 `node --check` + `node scripts/ui-runtime-smoke.mjs`,并列视图与冲突预警各有冒烟断言(conventions §1.3);⑩800/1024/1280 三档布局检查。
- 依赖: 
- 前置(不写进依赖,按 D-239 教训): 需要 R-177 提供真实的线(`worktree_path` 有真实值)才能端到端取证;R-177 之前可以先做 A 面的注入通道与提交纪律(对当前的「主树自举 + 外部 agent」两方已经立即有用,正是 D-263 的场景)。

- 批次: 2/5
- 进展: 2026-08-11 A 面已由 `e3679a2` 完成交付：协作块来自真实运行态、单线反证、轮内刷新且不累积历史、`collaboration_status` 主动查询、明确文件暂存纪律均有测试。B 面基础版由 `2d432fc` 交付：M/A/B 代号与颜色、跨线并列视图、认领/阶段/工具/改动文件/分支/运行态/token 真数据、文件集合交集冲突预警与下钻、合并前预检、语义层常驻提示；UI runtime 的两条 mutation guard 与 800/1024/1280 浏览器布局实测均通过。保持 doing：`parallel_lines_ui.md` 的 P2 活动记录按 agent、P5 收活五格、P6 设置按 agent、P7 开线耦合预检尚未实现；它们不属于本次“建线入口 + 并列视图 + 冲突预警”的发布范围。

## R-185 并行取活的依赖判定升级为正确性前提:同批派发前必须证伪语义耦合,不是调度优化 [todo]
- 优先级: P0
- 复杂度: 中
- 标签: 核心
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(2026-08-11 三条线实测 + 现有条目字段语义的读文自证)
- refs: R-182 R-184 R-177 D-239 docs/design/parallel_read_serial_write_orchestration.md
- 来源: 2026-08-11 用户在 R-182/R-184 定调后指出的第三条:「分析需求和缺陷的依赖性这个变得更重要了」。本条把它落成条目。
- 为什么它从调度优化升级成正确性前提(本条的全部理由): **串行写的时候,依赖判错是廉价且自愈的**——若 R-A 与 R-B 真有耦合,第二条跑起来会看到第一条的结果,自己就发现了。任务级并行把这个自愈机制拆掉了:两条线各自基于同一个陈旧基线往下做,**到合并时才暴露**。而暴露形态恰恰是 **git 检测不到的语义撞车**——A 把某个签名重构成形态①、B 按形态②写完,两边测试各自都绿,合并干净,语义已经坏了。R-182 的「边界」已明确**不做**语义撞车的事后检测(git 只做文本层),所以**事前的依赖判定是唯一的防线**。判错的代价从「多等一会」变成「合并出一个测试抓不到的坏语义」。
- 现状与缺口: 
  ①**判据今天是人工的**。本轮三条线(D-262/D-257/D-261)是人工挑的「文件面不重叠」,靠读条目正文推断,没有任何机械判据参与。
  ②**`依赖` 字段的语义不够用**。仓里实际存在两种前置——**阻塞依赖**(没它就做不了)与**非阻塞前置**(有它更好,解除权在 agent 手里),但字段只有一个。R-177 与 R-182 都在正文里写了「前置(不是阻塞,解除权在 agent 手里,按 D-239 教训**不写进「依赖」字段**免得调度器整条跳过)」——**用注释绕过字段缺陷已经成了惯例**,这本身就是缺口的证据。
  ③**D-239**(取活口径漂移:伪阻塞/伪可执行/挂起无载体)记的是同一个病在串行下的表现;并行会把它放大。
  ④R-184 解决的是「谁在跑」(且实测确认 `git worktree list` + `for-each-ref` 已免费提供),**不解决「该不该同时跑」**——两者是不同的问题,不要混为一谈。
- 内容: ①同批派发前的**耦合证伪**:给出一组可机械计算的信号,把「这两条能不能同时开」从推断变成判定。候选信号(按成本排序):`refs` 字段互指、条目正文点名的文件/路径面求交、以及**契约面求交**(函数签名、表结构、事件名、配置键——与 R-182 边界里记的语义撞车同一组维度)。②`依赖` 字段**拆成两个语义**:`阻塞依赖`(调度器必须跳过)与 `前置`(可并行,但要在协作上下文里对另一条线**显式说明**),消灭「靠注释绕过字段」的惯例。③判定结果**留痕**:派发时记下「凭什么判定这两条无关」,合并后若真出语义问题,能回查当初的判据错在哪——否则同一类误判会反复发生且无从改进。④判定为**耦合**时给出可执行的处置(串行化、或合并成一条、或明确指定谁先落地由谁重新适配),不是只报一个警告。
- 边界: 不做**全自动**依赖推断——信号用来收窄和提醒,最终判定仍可由人/编排者拍板,但拍板必须留痕(内容③)。不做语义撞车的**事后**检测(R-182 已明确不做,本条是它的事前对策)。不重做 R-184 的协作可见性(那是「谁在跑」,本条是「该不该同时跑」)。不改既有条目的历史 `依赖` 数据语义——迁移时旧值一律视为「阻塞依赖」,保守不激进。
- 验收: ①存在可机械计算的耦合信号并对**真实历史条目**跑出结果:至少能对本轮三条(D-262/D-257/D-261)判定为可并行,且能对一组**已知耦合**的历史条目(如 R-177 与 R-182 的主根重定向两半)判定为耦合,两个方向各有证据。②`依赖` 与 `前置` 两个语义分离落地,调度器只对 `阻塞依赖` 跳过;R-177/R-182 正文里那两段「不写进依赖字段」的注释可以删掉而行为不变(这是本条是否真解决了问题的判据)。③判定留痕可回查:能对任一次并行派发回答「当时凭什么认为这两条无关」。④判定为耦合时给出的处置是可执行的,不是一句警告,有实测轨迹。⑤旧数据迁移保守:既有 `依赖` 值一律按阻塞处理,无行为回归,有测试。⑥与 R-184 的边界在文档上写清,不留「协作可见性顺带解决依赖判定」的误解。
- 依赖: 
- 前置(不写进依赖,按 D-239 教训): 与 R-182/R-184 同族,三条构成任务级并行的最小可用集——R-182 拆掉多余的锁、R-184 让各方知道彼此在写、本条决定**哪些能同时写**。缺任何一条,并行都会以不同方式出事。

## R-186 跨树越界检测与回滚:ManagedSnapshot 范围从托管文档扩到「不属于本线的 worktree」 [todo]
- 优先级: P0
- 复杂度: 中
- 标签: 核心
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(D-174 已交付同哲学的实现;本条是范围扩展)
- refs: D-267(本条是它的替代交付) D-173 D-174 R-183 R-184 R-177 D-258
- 来源: 2026-08-11 用户定调砍掉 bash 权限中间档后的替代方案。原话:「这些直接砍了,没啥用说真的,并行自举本来就是激进的玩法」。
- 为什么是这个形态(本条的全部理由): 并行下真正要防的**不是恶意,是串台**——A 线的命令跑进 B 线的树、把人家**未提交**的活覆盖了。而**命令语法闸门恰恰防不住这个**:`cd ../other && rm -rf` 里没有一个可疑 token,`cargo` 也是合法程序、其 `build.rs` 能干任何事。
  正解沿用本仓既有哲学。设计基线明写着 bash 对托管文档的保护「**在结果侧**(执行前后快照比对 + 隔离留证 + 整体回滚),**不在权限层**」——D-173/D-174 已经把这条路走通并有实现(`crates/kanzei-tools/src/bash.rs` 的 `ManagedSnapshot::capture` / `is_complete`)。它的关键优势是**不关心命令长什么样**,所以 `cd ../other` 与 `cargo run` 里 build.rs 干的坏事**一视同仁抓得到**——这正是闸门做不到的那一半。
- 内容: ①把 `ManagedSnapshot` 的保护范围从「主根 `.kanzei` 托管文档」扩到「**不属于本线的 worktree**」:执行前拍快照的集合 = 托管文档 ∪ 其它线的工作树;②越界写入的处置沿用 D-174 既有形态(隔离留证 + 整体回滚 + 归因到 owner run),**不新造机制**;③「本线的树」由 `ProcessHandle.worktree_path` 给出(前置 R-177),主树进程的"本线"= 主根;④性能:快照集合可能很大,按**只对其它线的树做 mtime 级粗筛、命中再细查**收敛,不得让每条 bash 都全量哈希(D-233 的教训:同步全量读+哈希会把主线程占死);⑤越界事件进轨迹,**同时作为 R-184 冲突带的数据源**——"谁写了不属于自己的文件"与"谁和谁改了同一个文件"是同一份数据,不要采两次。
- 边界: **不做事前拦截**(那是被砍掉的 D-267 的路子)。不保护未纳入任何线的目录(用户自己的其它项目不在范围内——本条只管本仓的树之间)。不做跨机器。`ManagedSnapshot` 对**托管文档**的既有行为**一个字不改**(它是 D-173/D-174 的交付,只加范围不改语义)。
- 验收: ①A 线执行 `cd <B线树> && <写操作>` 后:改动被检测、被隔离留证、被回滚,B 线的工作树**逐字节复原**,有实测轨迹(不是只断言函数返回);②归因正确:轨迹里指出是哪条线(owner run)越的界;③**`cargo run` 里 build.rs 写别人的树**同样被抓——这条是本条相对闸门的核心优势,必须有定向测试;④托管文档的既有保护行为无回归(D-174 既有测试全绿);⑤性能:单条 bash 的快照开销有实测数字,N 条线时不随 N 线性劣化到不可用(给出实测,不接受"看起来还行");⑥越界事件与 R-184 冲突带共用同一份数据,不存在两套采集(机械核验:grep 只有一处采集点)。
- 依赖: 
- 前置(不写进依赖,按 D-239 教训): **R-177**(要有 `worktree_path` 才知道"本线的树"是哪棵)。R-177 之前可以先做托管文档侧的重构与 mtime 粗筛。
