# Memory 决策充分性改造(Control-Sufficient Memory)

- 身份: validated_design
- 状态: 已验证基线；R-145、R-150 已完成
- 日期: 2026-08-09
- 关联需求: R-103 (总纲)、R-145 (done)、R-150 (done)
- 关联缺陷: 无
- 关联决策: 无(不推翻 [memory_system.md](memory_system.md) §0 任何品味决策,本文是其判据层的升级)

## 背景与问题（实施前输入）

用户提供 Control-Sufficient Memory 研究文档(2026-08-09 对话,理论脉络含 MemFly/Memory-R1/AMA-Bench/LongHorizon-Harness 等),核心命题:

> 记忆不是保存过去,而是在容量约束下,保存过去中**足以支持未来正确决策**的信息。

形式化:记忆 M 的好坏不问「能否重建历史 H」,只问「Q(H,a)≈Q(M,a) 是否对所有动作成立」;决策失真 ≤ ε 则决策损失 ≤ 2ε。由此推出四条判据,全部与「语义显著度」无关:

1. **写入**:新信息的价值 = 它改变了多少未来决策(W),不是它语义多丰富;
2. **遗忘**:删除的判据 = 反事实遗忘成本(F:忘了它未来会差多少),不是年龄/LRU/相似度衰减;
3. **压缩/合并**:判据 = 决策保持(合并后不会有场景做错动作),不是摘要质量;
4. **检索**:本质是 token 预算下的决策价值优化(VOI),不是最近邻。

关键推论:`memory importance ≠ semantic salience`。8 个 token 的用户约束(「production DB 只读」)决策价值极高,25 步排障叙事若不改变未来动作则可全忘;同一主题的新状态必须**使旧状态失效**,不能并存。

### 实施前现状对照（gap 分析）

| 环节 | 现状 | 与理论的差距 |
| --- | --- | --- |
| 写入闸 | manager 判 NOOP/ADD 的判准是「durable fact」 | 语义显著度导向;没有「不记会做错什么动作」的反事实判据 |
| description | 只有「何时想起」钩子 | 缺「想起后该改变什么动作」的决策半边 |
| 状态型事实 | 只有 preference 有 upsert(标题前缀);fact 无失效机制 | 同一主题新旧事实并存,过期状态照常进索引(理论:superseded 必须 invalidate) |
| 遗忘/效果 | R-125 已有召回→采纳遥测(fetched),但只落库展示 | 没喂回排序;也没有反向信号——记忆存在但同类失败复发 = 该记忆没进决策 |
| 检索排序 | bm25 × log(1+hits),hits 是搜索命中数 | 命中自增强,与「是否真改变了决策」无关;召回从不被采纳的条目不会沉下去 |

### 已经对齐理论的部分(不改)

- **失败信号机械闸**(重复≥2 或恢复对)本身就是 Q 差异的行为学证据:「X 不行、Y 可以」直接表述了两个动作的价值差,这正是理论要保的信息;
- **preference 全文常驻** = 约束类记忆不参与预算竞争(约束的遗忘成本无界,理应置顶,不与 fact 抢预算);
- **分层架构**天然对应文档五层(见下),不需要动存储形态。

## 目标与非目标

**目标**:把 WRITE/FORGET/COMPRESS/RETRIEVE 四个操作的判据从语义显著度换成决策价值,全部用**机械代理量**实现——判据在引擎侧硬化,弱模型照着走(harness-validated-weak-model 准绳)。

**非目标(明确不做)**:

- 反事实 rollout 估 Q:研究方法,单次成本 ≈ 一次完整运行 × 候选动作数,个人工具不可承受。替代:用既有遥测(召回→采纳、失败复发)作遗忘成本 F(m) 的经验代理;
- 向量库/知识图谱/外部框架/learned critic:§0 品味决策不变;
- 不改文件优先、读写分离、两级 scope、frontmatter 格式(subject 走 extras,向后兼容)。

## 五层映射(文档架构 → kanzei 落位)

| 文档层 | 职责 | kanzei 落位 | 状态 |
| --- | --- | --- | --- |
| Event Log | 完整 append-only 真源 | state.db 轨迹 + episode 表 | 已有 |
| Verified State | 当前为真的事实,就地覆盖 | tracker(.kanzei/project/*)+ preference upsert + **本次新增 subject 状态事实** | 本次补齐 |
| Decision Memory | 仍会改变未来决策的约束/事实 | preference(常驻)+ fact | 判据本次改造 |
| Procedural Memory | 经验→策略改进 | sop + habit | 判据本次改造 |
| Working Memory | 预算内注入什么 | INDEX 常驻 + prompt_hints + 上下文账单 | 排序本次改造 |

## 最终方案(P1,本次交付)

1. **写入闸改反事实判据**(manager 系统提示词):ADD/NOOP 的单一判准 = 「没有这条,未来的 agent 会做错哪个动作?」说不出具体动作就 NOOP。description 升级为双钩子:何时想起 + 想起后改变什么(例:「处理 edit 替换失败/换行符问题时必读:先 read 重读再改」)。
2. **状态语义(subject)**:`memory_add` 新增可选 `subject`(稳定主题键,如「安装通道」「当前开发分支」);引擎强制**同 scope+category+subject 至多一条 active**,冲突时拒绝并指路 `memory_update` 既有条目——状态就地覆盖,绝不并存;`force` 不可绕过(状态不变量,不是风格偏好)。subject 存 frontmatter extras,旧条目不受影响。
3. **复发检测(记忆无效的反向信号)**:失败笔记自带 `[fp:tool|kind]` 指纹;manager 建条目时把指纹**原样带进正文**(提示词要求,精确子串即可,弱模型可执行)。轮末采集时,若指纹已存在于某条 active 记忆而同类失败仍复发 → 投**修订笔记**点名该条目(「记忆在,坑还在 = 它没进决策」),要求补判据/改钩子,而不是原坑重投。
4. **检索排序折入采纳率**:memory_recalls 聚合出每条 (召回次数, 采纳次数);召回≥3 时得分乘 `0.6 + 0.7×采纳率`(0%→×0.6,100%→×1.3)。反复被召回但从不被采纳的条目 = 语义显著但决策无关,正是理论要沉底的那类。
5. **观测面**:`memory_stats` 增加各 scope 召回/采纳汇总与「零采纳候选」清单(召回≥3 采纳=0),供空闲整理与 UI 消费。
6. **注入文案同步**:dev/memory 源的收尾指引从「Facts only」改为决策判据表述——只记会改变未来动作的东西。

## P2(R-150,移交自举循环)

- 空闲整理接决策价值:零采纳候选与复发告警进整理清单(降级/修订走既有墓碑机制,不静默删);
- Memory UI 页展示采纳率与复发告警;
- 轨迹实证与 R-145 并轨:发版后取「写入→命中→避免重复探索」的轨迹证据,即文档 Memory Value Density(每 token 记忆贡献多少决策价值)的实证形式。

## 技术选型与取舍

- **采纳率降权下限 0.6、不清零**:召回样本天然有偏——prompt_hints 只注入索引行,模型「看行即用、不拉正文」会被记为未采纳;所以只降权不淘汰,淘汰决定留给人与整理流程(墓碑可逆)。
- **复发指纹用精确子串,不用 FTS 相似度**:弱模型只需「原样复制一个 token」,引擎侧全精确、零阈值、可单测;manager 不配合时自然退化为现状(重复投坑),无损。
- **subject 冲突返回既有条目而不是静默 upsert**:写路径归 manager,引擎只做硬门禁;自动改写会绕过「谁改的、为什么改」的溯源。preference 的 upsert 是用户直写通道,语义不同,保持不动。
- **不加新 category**:「约束」由 preference 承载(已常驻),「状态」由 subject 承载(fact 的属性,不是新类目)——词表不膨胀,弱模型不用学新枚举。

## 实施边界与调用方

- `crates/kanzei-memory/src/memory/admission.rs`：准入校验、subject 不变式、近似去重、指纹与 tracker 交付状态拒收；`MemoryAdmission` 可脱离 store 单测。
- `crates/kanzei-memory/src/memory/lifecycle.rs`：`MemoryLifecycle::promote_guard`、`should_promote`、`should_deprecate`；真实 episode provenance、shadow 与 candidate 老化在此收口。
- `crates/kanzei-memory/src/memory/retrieval/search.rs` 与 `crates/kanzei-memory/src/memory/index.rs`：搜索候选、BM25/dense/hybrid、决策权重与命中记录；R-255/D-366 后排序只在检索侧。
- `crates/kanzei-memory/src/memory/mod.rs` / `inbox.rs`：轮末失败收集、复发分支、批量 inbox、checkpoint 与 memory control-plane 事件。
- `crates/kanzei-memory/src/memory/manager.rs`：`memory_add`、`memory_promote`、`memory_update`、`memory_merge`、`memory_stale` 写工具与 manager 提示词。
- `crates/kanzei-memory/src/memory/telemetry.rs` 与 `crates/kanzei-core/src/store/telemetry.rs`：召回/采纳画像及 action_changed/outcome_improved 分段遥测。
- `crates/kanzei-app/src/memory.rs`、`crates/kanzei-app/ui/13-memory.js` 与 CLI run 收尾方是真实消费者；R-203 旧的记忆工具路径已迁入 `crates/kanzei-memory/src/memory`，本文不再引用旧落位。

## 边界拍板(2026-08-09 用户逐项确认)

1. subject 状态语义:**引擎硬门禁**(同 scope+category+subject 至多一条 active,force 不可绕);
2. 复发检测:**manager 带指纹进正文**(精确子串,零新表;存量条目无指纹时退化为现状);
3. 采纳率排序:**温和降权**(召回≥3 生效,×0.6~×1.3 不清零);参数 0.6/0.7 与阈值 3 为初始值,**待真实召回数据分析后复核**;
4. P1 范围:**六项全做**。

## 变更记录

- 2026-08-09 草案:依据用户提供的 Control-Sufficient Memory 研究文档完成 gap 分析与 P1/P2 分期;边界确认前不动代码。
- 2026-08-09 边界拍板:用户逐项确认上述四点,转设计基线,当日实施 P1。
- 2026-08-09 实证修正:真实召回数据(37 轮)显示 preference 走 prompt_hints 召回路径且采纳率结构性无意义,search() 豁免 preference 的 decision_weight;同批发现 D-214(SOP 候选滞留全局 inbox)与「read 不计采纳」遥测缺口(挂 R-150)。
- 2026-08-09 全环节评审后硬化(D-215/D-216 当日修复):①update/merge 引擎兜底指纹与 refs(fp_markers 提取、update 丢指纹拒绝、merge 自动搬运);②注入与 hints 统一口径(resident_index 共用预算走查,常驻条目短指向、折叠条目全行、preference 不进 hints 与遥测);③登记 D-217(stale 归档搬运不存在)与 hits 因子去留(并入 R-150 复核)。
- 2026-08-10 R-150 参数复核结论:①**hits 因子退役**——搜索命中自增强(常被搜到→排更前→更常被搜到),与采纳率权重「召回未采纳→沉底」方向冲突,理论 importance ≠ semantic salience;排序权重只留 bm25 + 采纳率决策权重,hit_count 降为观测(SearchHit.hits 仍回传,UI 与遥测可看,不再乘进 score)。②**0.6/0.7/阈值 3 保留**:真实采纳率分布尚不足(37 轮实证主要暴露 preference 结构性无意义,已豁免),且两个低估通道未闭合(「看索引行即用」「直接 read 记忆文件不经 memory_search」),此刻调参没有可靠信号;待 R-145 轨迹实证补足数据后再复核。③**read 钩子缺口确认**:给 read 加记忆目录钩子回填 mark_recall_fetched 是消除低估通道的机械手段,列入 R-145 并轨实施,本轮只记录不实现。④验收①「空闲整理清单」落地:memory_value_flags(零采纳 recalled≥3&fetched=0 + 复发候选 recalled≥3)进 Memory UI,处置走既有墓碑机制不静默删。
- 2026-08-10 R-145 闭环实证(真实自举轨迹,state.db + index.db immutable 只读查询):①**写入→命中→避免重复探索成立**:199 episodes / 413 条召回 / 156 采纳(37.8%);M-006(前端动态 i18n 源文案)召回 110 采纳 25、M-009(edit 未命中→read 重读)召回 49 采纳 24、M-022(验证 Rust 变更跑 test_record)召回 54 采纳 14——高频条目写盘后被后续轮检索命中且正文被拉取,直接佐证避免重复探索(如 M-009 后 edit 未命中即 read 重读,不再摸索)。②**注入 token 有界**:173 轮有记忆注入账单,字节 p25=3519/p50=3667/p75=3732/max=3941——窄区间稳定,不随 25+ 条目线性膨胀,无信息缺失返工(采纳率>0 且注入有上界)。③R-150 复核缺口仍开放:read 钩子回填 mark_recall_fetched 未实施,采纳率仍有低估通道;参数 0.6/0.7/阈值 3 维持。

## 验证证据

- R-145 闭环实证已完成：真实自举轨迹记录 199 episodes、413 次召回、156 次采纳（37.8%）；M-006/M-009/M-022 的后续命中与正文拉取记录支持“写入→命中→避免重复探索”。同批 context_report 记录 173 轮注入账单，字节 p25=3519、p50=3667、p75=3732、max=3941，确认 token 有界且无信息缺失返工。
- R-150 消费已完成：`memory_value_flags` 产出零采纳与复发候选，Memory UI 展示召回/采纳数据并沿用墓碑处置；800/1024/1280 三档宽度和四条前端冒烟通过。
- 以上证据与 R-145/R-150 tracker 关闭记录一致；本节不再把已完成实证写成“无验证”的草案计划。

## 后续边界与风险（实施后）

- R-150 的空闲整理清单、Memory UI 消费与参数复核已完成；R-145 的真实轨迹证据已回填变更记录和 tracker。
- 历史实证输入（保留）：2026-08-09 数据分析确认 preference 会被 prompt_hints 召回但正文常驻，因此 `search()` 对 `category=preference` 豁免 `decision_weight`；hits 因子已退役，0.6/0.7/阈值 3 保留待真实数据复核。
- 历史边界说明（保留）：当时已知直接 `read` 不计采纳、索引行即用也可能低估采纳率；本次把该结论改写为实施后的非阻塞缺口，而非删除原风险。
- 已知非阻塞缺口：直接 `read` 记忆文件不回填 `mark_recall_fetched`，采纳率可能被低估；该缺口在 R-145/R-150 关闭范围外，未来若要修复需另立条目。
- 旧条目无指纹时复发检测退化为现状而非变坏；随 manager 增量补齐，保持可追溯。
