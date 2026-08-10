# Requirements

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

- 进展: 批1完成(MemoryIndex trait + SqliteMemoryIndex lexical 降级,162 全绿)、批2完成([embeddings] 配置节 + Embedder/OpenAiEmbedder + 向量列存储,167+64 全绿)。

批3完成(R-164 B3):index.rs 实现 dense 通道——dense_scan 读 memory_vectors 全表 brute-force 余弦(topN),dense() 入口(query 文本→embedder 向量→扫描);search_hybrid 在有 embedder 时做 RRF 融合(k=60,lexical top10 + dense top10 → top5,禁止线性加权,设计 §5),dense 空结果自动退化为 lexical;新增 search_hybrid_with_timing 返回 (hits, RetrievalTiming{lexical_ms,embed_ms,vector_ms}) 供 RecallEvent 分段延迟落库(验收②)——检索层不碰 SessionStore,落库由装配方(批4)做。4 个新测试:cosine_相似度_同向为1_垂直为0/dense_检索_embedder配置后按语义命中/hybrid_rrf融合_同时出现在两通道的条目排名靠前/hybrid_带分段耗时_无embedder时embed与vector段为0。kanzei-tools 171 passed 全绿。

批次规划: 批4 R-163 三臂对比装配(lexical/dense/hybrid)+ 报告落库(验收③)+ replay_eval 落 recall_events 分段延迟(验收②装配)。实现注:向量列用 rusqlite 普通表 + Rust 侧 brute-force 余弦,替代 sqlite-vec loadable extension——Windows bundled rusqlite 加载扩展有版本兼容与分发负担,功能语义(向量列在 index.db、brute-force、可重建)与设计 §5 一致。

批2完成(R-164 B2):(1) kanzei-harness config.rs 新增 [embeddings] 节(EmbeddingsSection{provider,model} + enabled(),serde default 缺节关闭,层叠合并逐字段覆盖,unknown_keys 清单登记)——旧配置无节时通道关闭行为不变,harness 64 测试含新增 embeddings_缺节关闭_配置后启用_旧配置行为不变;(2) kanzei-tools/src/embed.rs 新增 Embedder trait(同步签名,内部 tokio runtime 驱动)+ OpenAiEmbedder(openai 兼容 POST {base_url}/embeddings,解析 data[].embedding,api_key 经 provider api_key_env/api_key 解析,本地 ollama 免 key)+ FakeEmbedder(测试用确定性向量)+ embedder_from_config 工厂(未配置→None 关闭通道);mock HTTP 测试验证 URL/请求体/响应解析,3 测试;(3) SqliteMemoryIndex 接向量列:with_embedder 构造 + memory_vectors 表(同 index.db,派生物)+ vectorize/upsert 增量/remove 删行/rebuild 全量重建(验收④),2 新测试(有embedder时rebuild生成向量_无embedder时向量表空/upsert_增量维护向量_remove删除向量)。

批次规划: 批3 dense 通道 brute-force 余弦 + RRF 融合(k=60)+ 分段延迟落 recall_events(验收②);批4 R-163 三臂对比装配(lexical/dense/hybrid)+ 报告落库(验收③)。实现注:向量列用 rusqlite 普通表 + Rust 侧 brute-force 余弦,替代 sqlite-vec loadable extension——Windows bundled rusqlite 加载扩展有版本兼容与分发负担,功能语义(向量列在 index.db、brute-force、可重建)与设计 §5 一致。

批1完成(R-164 B1):crates/kanzei-tools/src/memory/index.rs 新增 MemoryIndex trait(IndexQuery/IndexHit + search_lexical/dense/hybrid/upsert/remove/rebuild)与 SqliteMemoryIndex 默认实现——lexical 通道复用 FingerprintIndex(Tier0 指纹精确)+ MemoryStore::search(Tier1 BM25),dense 恒空(未接 embedder),hybrid 无 embedder 时自动退化 lexical(验收①);mod.rs 注册导出。3 个新测试:无embedder降级_fingerprint精确命中与BM25完整可用/指纹miss时回落BM25_文本可兜底/upsert_remove_rebuild_增量与全量一致。kanzei-tools 162 passed(原159+3)全绿。

批次规划: 批2 Embedder trait + openai 兼容 /embeddings 实现(含 ollama)+ kanzei.toml [embeddings] 配置节 + 向量列存储 + rebuild(验收④);批3 dense 通道 brute-force + RRF 融合(k=60)+ 分段延迟落 recall_events(验收②);批4 R-163 三臂对比装配(lexical/dense/hybrid)+ 报告落库(验收③)。实现注:向量列用 rusqlite 普通表 + Rust 侧 brute-force 余弦,替代 sqlite-vec loadable extension——Windows bundled rusqlite 加载扩展有版本兼容与分发负担,功能语义(向量列在 index.db、brute-force、可重建)与设计 §5 一致。

- 批次: 3/4

## R-165 Memory Compiler:manager 升级为证据编译与生命周期管理 [todo]
- 优先级: P0
- 复杂度: 大
- 标签: 核心
- 阶段: 2
- 依赖: 
- 来源: 同 R-161。范式反转:evidence 不可被 LLM 持续改写(文献:持续 consolidation 使记忆效用先升后降),manager 从 CRUD 升级为编译语义。
- 内容: ①动词升级 OBSERVE/PROPOSE/VERIFY/PROMOTE/SUPERSEDE/DEPRECATE,evidence(state.db events/episodes)对自治流程 append-only;②novelty gate 三档:明显新→PROPOSE、明显重复→NOOP、不确定→才起 LLM 判断;③转换三问检查:coverage/preservation/faithfulness;④后台触发扩展(现只有轮末):compaction 边界、recurrence(第 2 次才 candidate、第 3 次+修复成功才 promote)、idle debounce、memory pressure;⑤lifecycle 轻量四态 candidate→active→deprecated|invalid(stale 兼容映射 deprecated,shadow 留给 R-166);⑥provenance 硬约束:PROMOTE 必须带 memory_sources 行,无来源不入 active;⑦归档落地修 D-231;⑧merge 保守闸:评估器落地前只合并同 fingerprint 或用户确认的。
- 验收: ①无 provenance 不入 active(引擎拒绝有测试);②recurrence 三段晋升有单测;③deprecated/invalid 移入 archive/ 且默认检索不可见;④novelty 三档分流有计数遥测;⑤evidence 表无任何自治写路径(代码审计+测试)。
- refs: R-105 D-231 docs/design/memory_control_plane.md

- 进展: 2026-08-10 口径清理:原「依赖: R-162」被 list 判为 blocked,实为非阻塞内部依赖,清出依赖字段。解锁条件: 完成 R-162 后推进。

## R-166 记忆反事实评估器:遗忘成本 F(m) 与合并守恒 D(S→m') 落地 [todo]
- 优先级: P0
- 复杂度: 大
- 标签: 核心
- 阶段: 2
- 依赖: 
- 来源: 同 R-161。理论锚点 DeMem(决策失真,安全合并=存在共同近优动作,而非文本相似);kanzei 有可执行 verifier,J 不靠 LLM judge。
- 内容: ①F(m)=E[J(e;M)−J(e;M∖{m})] 离线定向回放,绝不在线算;②每条 memory 维护 Q(m)=触发匹配 episode+near-miss+negative control;③周期性 with/without 回放,memory_eval 维护 effect_mean/effect_ci/eval_n/last_eval;④merge 由 D(S→m')<ε 把关,压缩变成有测试的行为等价变换;⑤shadow 态引入(五态齐):可被评估、不注入生产;⑥只有 low value+high confidence 进 deprecate 候选,age 不作为独立淘汰判据。
- 验收: ①每条 active 记忆可查 F(m) 估计与置信区间;②至少一次真实 merge 经 D<ε 判定放行或拒绝且判定依据落库;③shadow 条目不注入生产但被评估(测试);④代码中无按时间衰减的淘汰路径。
- refs: R-149 R-150 docs/design/memory_control_plane.md

- 进展: 2026-08-10 口径清理:原「依赖: R-163/R-165」被 list 判为 blocked,实为非阻塞内部依赖,清出依赖字段。解锁条件: 完成 R-163/R-165 后推进。

## R-150 记忆决策价值 P2:空闲整理与 UI 消费零采纳与复发清单 [todo]
- 优先级: P1
- 复杂度: 中
- 标签: 前端
- 阶段: 2
- 依赖: R-149
- 来源: 同 R-149,P2 移交自举循环。
- 内容: 消费 R-149 产出的决策价值信号:①空闲整理(sleep-time)把「零采纳候选」(召回≥3 采纳=0)与「复发告警」纳入整理清单,处置走既有墓碑机制(降级/修订/归档),不静默删;②Memory UI 页展示每条目的召回/采纳率与复发告警,零采纳候选有显式标记;③与 R-145 并轨:发版后取自举轨迹验证「写入→命中→避免重复探索」闭环,并复核 R-149 降权参数(0.6/0.7/阈值 3)是否合适——复核须计入两个采纳率低估通道:「看索引行即用」与「直接 read 记忆文件不经 memory_search 不计采纳」(后者可考虑给 read 加记忆目录钩子回填 mark_recall_fetched);同批决定 hits 因子去留——hits 奖励「常被搜到」(自增强)与采纳率权重惩罚「召回未采纳」方向冲突,候选处置:退役或降为平局破除器。
- 验收: ①空闲整理清单包含零采纳与复发两类候选且处置有墓碑;②Memory 页可见召回/采纳数据(800/1024/1280 三档可用);③降权参数复核结论落回 docs/design/memory_decision_sufficiency.md 变更记录。
- refs: R-103 R-107 R-125 R-145

## R-132 mem单页手动触发整理功能 [todo]
- priority: P1
- 原始描述: mem单页应该有个可以手动触发的整理，这个需要详细设计，先记录吧
- 复杂度: 中
- 归属: kanzei
- 验收: mem单页提供手动触发整理的入口，触发后执行整理流程并给出结果反馈

- 标签: 核心

## R-145 Memory 闭环实证:发版后轨迹命中与 token 基线对比 [todo]
- 优先级: P1
- 内容: 承接 R-105 验收①(连续自举轮次完整闭环实证:轮末写入→后续轮命中→避免重复探索,以轨迹为证)与 R-106 验收①(同类任务每轮注入 token 较基线下降且无因信息缺失导致的返工)。两者均需发版后在真实自举循环中取轨迹对比,不可本机验证;代码项已随 R-105/R-106 交付,本条目只跟踪实证落地。
- 复杂度: 小
- 标签: 流程
- 阶段: 5
- 验收: 自举循环发版运行 N 轮后,提供轨迹证据:①轮末记忆写入被后续轮检索命中且避免重复探索;②同类任务注入 token 较基线下降且无信息缺失返工。证据形式:episodes 落库记录、context_report 账单查询结果、轨迹摘录。

## R-151 用户约束的机械捕获通道:对话定调不再靠主 agent 自觉投 note [todo]
- 优先级: P2
- 复杂度: 中
- 标签: 核心
- 阶段: 2
- 依赖: 
- 来源: 2026-08-09 R-149 全环节评审结论:论文里决策价值最高的信息形态(用户在对话里随口说的约束,如「以后别动 production」)目前完全依赖主 agent 自觉 memory_note,是写入环节唯一没有机械通道兜底的缺口;用户拍板「占位,等 R-150 遥测数据积累后再评估值不值得做」。
- 内容: 占位。方向:轮末由引擎对本轮用户消息做机械提取(候选形态:祈使+否定/「以后」「必须」「不要」类定调句),投 preference/habit 候选进 inbox,由 manager 判 NOOP/ADD——引擎只采集不判语义,与 harvest_failures 同哲学。是否立项取决于 R-150 遥测:若真实轨迹里出现「用户说过但没进记忆、后续违反」的实例,则升优先级动工;若 memory_note 自觉率足够,关闭本条。
- 验收: 先出判定报告(基于 R-150 遥测与轨迹实证,给出做/不做结论与依据);若做,再补机械提取的功能验收。
- refs: R-149 R-105

- 进展: 2026-08-10 口径清理:原「依赖: R-150」被 list 判为 blocked,实为非阻塞内部依赖(解除权在 agent,完成 R-150 即可),按 §1.1/§1.35 清出依赖字段。解锁条件: 完成 R-150 遥测后按验收出判定报告(做/不做结论)。

## R-167 学习型召回控制器占位:bandit 调度 recall 动作 [todo]
- 优先级: P2
- 复杂度: 中
- 标签: 核心
- 阶段: 2
- 依赖: 
- 来源: 同 R-161,MemCon 方向。占位:确定性 RecallPolicy 数据积累后才评估是否值得上 contextual bandit(state:goal/tool/error/stuck 计数;reward:任务成功−失败成本−token−延迟)。
- 内容: 占位。是否立项取决于 R-162 落地后的 trigger_precision/recall 实证——确定性规则已够好则关闭本条,不硬上学习组件。
- 验收: 先出判定报告(基于 R-161/R-163 数据,给出做/不做结论与依据);若做,再补功能验收。
- refs: R-162 docs/design/memory_control_plane.md
- 进展: 2026-08-10 口径清理:原「依赖: R-161/R-162/R-163」被 list 判为 blocked,实为非阻塞内部依赖,清出依赖字段。解锁条件: 完成 R-161/R-162/R-163 后推进。

## R-171 多进程代理编排 P0:并行查、项目级单写与工具串行强制 [todo]
- 优先级: P0
- 复杂度: 大
- 标签: 核心
- 阶段: 2
- 调度顺序: 紧跟 R-161～R-167 memory system 开发序列之后；这是开发顺序，不登记为阻塞依赖，memory 序列收口后直接取活。
- 来源: 2026-08-10 用户定调子代理计划的核心原则为「并行查，串行写」，并要求收束仓库多进程代理流接口；完整设计见 docs/design/parallel_read_serial_write_orchestration.md。
- 内容: ①新增 `ReadParallelWriteSerial` 执行策略与项目级 `ProjectExecutionCoordinator` 接口；②勘察/复核阶段允许 task 只读子代理并行，全部进入终态后经过汇总屏障；③同一规范化 project_root 同时只允许一个 writer run，租约跨实现/集成阶段和连续工具调用持有；④writer 阶段禁用 task，普通工具强制按模型调用顺序 FIFO 串行；⑤ProcessHandle 共享项目协调器，ToolCtx 分离 worktree_key 与 project_write_key 并携带 run/process 身份；⑥quick_req、tracker、goal、memory、test_record、Git/worktree 等独立写入口全部接入同一仲裁；⑦写队列、租约、阶段、取消和恢复事件落现有 session/run 轨迹。
- 边界: P0 覆盖当前应用内多个 ProcessHandle；不做图形化 DAG、不开放子代理通用写权限、不在本批实现跨机器调度。worktree 保留隔离、diff、恢复和交付能力，但不能绕过项目级单 writer。
- 验收: ①至少两个只读子代理真实重叠执行且工具白名单无写入口；②汇总屏障前 writer 不启动，失败/超时都有终态；③writer 阶段普通工具 max in-flight=1 且结果按调用顺序归位；④两个 ProcessHandle 竞争写权时租约区间不重叠，同一 writer 的连续写之间不能插入第二个 writer；⑤quick_req/tracker/test_record/Git/worktree 写入无法绕过协调器；⑥停止、关闭、panic 收尾后租约可靠释放；⑦一条真实需求留下「并行勘察→串行实现/集成→并行复核→串行修正」完整轨迹。
- refs: R-050 R-117 R-138 R-141 D-227 docs/design/parallel_read_serial_write_orchestration.md

## R-050 并行对话线程与分支工作树:隔离运行、冲突检测与合并 [todo]
- 复杂度: 大
- 优先级: P2
- 来源: 用户反馈:历史对话或新开线程并行推进项目,类似 git 分支/树,最后解决冲突合并
- 验收: 设计文档明确线程/项目/工作树关系、锁顺序、取消与崩溃恢复;两个线程可独立运行且互不串消息/权限/活动/停止;写入冲突能在提交前检测并阻止自动覆盖;worktree 模式可查看 diff、选择合并或放弃;合并失败保留双方改动和可恢复入口
- 已完成: 线程隔离(=R-030 进程页签)真实可用,消息/权限/队列/活动/停止按 session 隔离并有 POC 测试;worktree 后端命令 create/diff/merge/discard 存在,merge 前的 `git merge-tree --write-tree` 冲突预检真实实现(kanzei-app/src/main.rs:671-684);设计文档 deep_parallel_dev.md(含附录早期 POC)继续承载 worktree/模型隔离方案,多进程调度与写入纪律以 parallel_read_serial_write_orchestration.md/R-171 为准。
- 退回原因: 2026-08-07 验收核查发现核心组合未成立,勾不该打。①worktree 与线程完全脱节:ProcessHandle.worktree_path 恒为 None(main.rs:164/523,全仓库无 Some 赋值),process_create 不接受 worktree 参数,run_prompt 校验进程必须属于主项目目录(2605-2607)——没有任何线程能在 worktree 里运行,所有并行线程写同一工作目录;应用内无流程会在 worktree 分支产生提交,"合并"在闭环内空转。②多进程同一工作树无任何写冲突检测,设计承诺的项目写锁/git 锁/docstore 版本哈希在代码中完全不存在。③"可查看 diff"实为 git status --porcelain 文件名列表弹 toast(见 D-096)。④崩溃恢复仅设计文字,worktree 清单存 localStorage 不从 git worktree list 发现。
- 下一步: R-171 先在 memory system 开发序列之后交付项目级单 writer 与串行工具地基；R-050 的 worktree 绑定、diff 与恢复仍按 deep_parallel_dev.md 分阶段推进,且该文 §6 其余 D1~D7 未经用户定案前不得动工。
- 遗留质量问题: worktree 四个命令零测试;worktree_field 的 field 参数是无效分支(main.rs:605-610 两分支返回同值);frontend_phase3.md 的 POC 章节重复粘贴两遍且第一遍路径写错。
- refs: R-030 D-096
- 阶段: 5
- 证据等级: E2+E3
- 设计定位: 功能需求(2026-08-08 用户定调:R-093 的"质量先行"阶段门槛作废,按普通优先级参与取活)

- 标签: 核心

- 进展: 2026-08-10 口径更新:本条 worktree/模型隔离部分的门禁仍成立,保持 todo;项目级单 writer 与串行工具已拆为 R-171,不受本条未定决策阻塞。
- 阻塞: 用户: 需先对 docs/design/deep_parallel_dev.md §6 中除“项目级单 writer”外的 worktree/模型隔离决策逐条定案。解除动作:用户审阅剩余决策点并拍板后,本条 worktree 实施部分解除。

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

## R-117 子代理运行状态的可观察性 [todo]
- 复杂度: 中
- 优先级: P3
- 原始描述: 添加触发后弹出浮层显示最近开发和当前进展列表
- 范围界定: 2026-08-08 用户澄清真实意图是"子代理能对当前运行状态进行观察",并明确表示在 R-095 的呈现优化落地后不确定是否仍需要独立入口。
- 待定: 本条挂在 R-095 之后再定去留。R-095 交付后由用户判断:若活动面板的筛选折叠与后台任务操作已足够观察子代理状态,则本条关闭;若仍缺子代理各自的进度维度,则按缺口重写验收。
- 依赖: 

- 标签: 前端

- refs: R-095
- 进展: 2026-08-10 复查:R-095 已交付(done),其验收⑤明确覆盖子代理状态观察——活动面板子代理条目给出内部调用数与当前步骤,入参/输出/成败/耗时齐备。本条原始诉求「子代理能对当前运行状态进行观察」已被 R-095 覆盖;去留按「待定」字段由用户拍板(关闭或按缺口重写验收),agent 不擅自决定。依赖 R-095 已关闭,移入 refs。

## R-128 全部阻塞时停止鞭挞的逻辑设计 [todo]
- priority: P2
- 原始描述: 如果全部阻塞，应该要停止鞭挞，需要更多的设计鞭挞停止的逻辑
- 复杂度: 中
- 归属: kanzei
- 验收: 当全部条目处于阻塞状态时,系统自动停止鞭挞,不再触发催办;阻塞解除后可恢复

- 标签: 核心

- 进展: 2026-08-10 用户指令「把继续文案拆解了，鞭挞相关的核心部件拆解到 harness 里，评估一下保留继续文案的必要性」。已完成评估:docs/design/continue_prompt_dissection.md(草案,架构索引已登记)。结论:保留继续文案但降级为「用户意图载体」——规则 1-6/TAIL/开发重心拼接/LEGACY 全部剥离(真源已在 system prompt 与 kanzei.toml cadence),鞭挞核心部件(空转检测/连数/NUDGE/调度/暂停/停止原因状态机)下沉 harness。本条目验收「全部条目阻塞时自动停止鞭挞、阻塞解除后可恢复」为引擎化方案的一个判定分支,实施并入「鞭挞状态机引擎化」新条目(待用户拍板方案后开做)。

## R-129 单页阅读信息记忆困难优化 [todo]
- priority: P3
- 原始描述: 记忆单页阅读信息太复杂，有阅读障碍
- 复杂度: 中
- 归属: kanzei
- 验收: 提供分段展示/摘要功能帮助用户理解单一页面内容，减少认知负荷

- 标签: 前端

## R-130 测试用例记录触发机制与缺陷迁移 [todo]
- 原始描述: 测试用例相关的记录似乎没有触发机制，然后是把测试移动到缺陷下面，然后需要一次性记录存性
- 复杂度: 中
- 归属: kanzei
- 验收: 实现基于事件的或手动触发的测试用例记录机制，并在系统中建立测试到缺陷的映射关系，完成现有机现有测验数据的批量导入和初始化。
- 优先级: P2

- 标签: 后端

## R-133 diff树渲染优化 [todo]
- 原始描述: diff树的显示很丑，标记颜色并且不要重叠
- 复杂度: 中
- 归属: kanzei
- 验收: 实现color标记的git diff树，解决重叠问题确保视觉清晰
- 优先级: P2

- 标签: 前端

## R-135 开发与缺陷修复进度动画显示 [todo]
- 优先级: P0

- 标签: 前端

## R-137 Anthropic thinking 块协议回放:signature 原样回传,多轮工具不再 400 [todo]
- 背景: direction_taste 复刻清单·高:CC 按协议要求回放 thinking 块;kanzei 现状 anthropic.rs:97 Part::Reasoning => None 丢弃全部 Reasoning,thinking+工具第二轮必 400(R-094 只做了请求侧思考强度,未做响应侧回放)。
- 设计定位: 复刻 CC 基线行为:thinking 块按协议要求回放
- 证据等级: E2
- 阶段: 1
- 验收: anthropic 通道多轮工具调用时:①thinking 块的 signature 在后续请求中原样回传;②thinking+工具第二轮不再 400;③非 thinking 模型的 reasoning 文本以可见 assistant 文本保留(与 R-094 结论一致);④补 anthropic 多轮含 thinking 的协议契约测试。

- 优先级: P0

- 标签: 模型

## R-138 docstore 原子写与跨进程文件锁:tmp+rename + 独占句柄,并发写不丢不撞 [todo]
- 背景: direction_taste §5.2 地基债:docstore 整文件重写无原子替换与跨进程锁,D-064 类 lost-update 真实存在;deep_parallel_dev §3.3 P4 也要求 docstore 进程级文件锁收口主根 .kanzei 的最后一个共享写点。
- 设计定位: tracker 文档写入的原子性与并发安全
- 证据等级: E2
- 阶段: 1
- 验收: docstore save 改 tmp+rename 原子替换(临时文件与目标同目录);跨进程文件锁(Windows std::fs 独占句柄,毫秒级持有);并发写 tracker 的压测不丢条目不撞 ID;失败时保留现场可重试。

- 优先级: P0

- 标签: 核心

## R-140 i18n 架构迁移:chrome/content 分离、t(key) 渲染点翻译、MutationObserver 退役 [todo]
- 背景: direction_taste 定调二(用户明确):i18n 保留换架构。现行词典+MutationObserver 已产出 8 条缺陷家族(D-092/D-108/D-129/D-135/D-136/D-142/D-157/D-160)并篡改模型输出显示;D-172 只修了死循环,未换架构。四铁律:chrome/content 分离、翻译发生在渲染点 t(key)、模型输出语言是 prompt 问题、漏译可机械检出。
- 设计定位: i18n 架构迁移:先止血再渐进 key 化
- 证据等级: E2+E3
- 阶段: 1
- 验收: ①消息容器子树整体豁免词典替换(立即止血,终结数据篡改);②静态 DOM 改 data-i18n 一次性应用、JS 动态字符串经 t(key,params) 产出,禁止事后全文档扫描改写;③MutationObserver 退役;④漏译回落中文原文,冒烟脚本加 key 覆盖率断言;⑤按 A-003 粒度一轮吃一个界面域直至词典机制退役。

- 优先级: P0

- 标签: 前端

## R-141 ToolCtx 显式主根绑定:消除发现式取根与 worktree 锁键歧义 [todo]
- 背景: direction_taste §5.4 与 D-170 教训:ToolCtx::new 仍发现式取根(harness/src/tool.rs:13-17),worktree 线若命中 worktree 内 .kanzei 副本会拿到过期身份;并发锁键语义(tool.rs:19-28)只拼 project_root,两棵树同路径会撞锁。deep_parallel_dev §3.2 明确选 A:显式主根、不做根发现。
- 设计定位: 深并行前置:线进程显式携带主根,消除发现式根解析事故面
- 证据等级: E2
- 阶段: 1
- 验收: ToolCtx 构造支持显式传入 project_root(不再无条件 discover);线路径全程显式传根;补断言测试:worktree 内运行时 project_root 必须等于主根;并发锁键区分 worktree 实例。

- 优先级: P0

- 标签: 核心

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
