# kanzei Memory 控制平面(Decision-Centric Memory Control Plane)

- 状态: 设计基线 + 已交付实现对账(2026-08-18)
- 需求: R-161(遥测)→ R-162(事件触发召回)→ R-163(回放评估)→ R-164(混合检索)→ R-165(Memory Compiler)→ R-166(反事实评估)→ R-167(学习型控制器,占位)；已交付扩展：R-194(废弃 global scope)→ R-195(candidate 生命周期)→ R-213(provenance 真校验)→ R-216(写入三闸)→ R-233(hybrid 召回)→ R-255(MemoryStore 拆域)→ R-286(控制面与 outcome 漏斗)
- 缺陷: D-229(harvest_sop 双端不对称)、D-230(常驻索引装箱顺序)、D-231(归档未落地)；已修复边界：D-366(检索排序边界)、D-368(memory 动态文件围栏并发写)
- 决策: A-011(向量检索翻案)
- 前置文档: [memory_system.md](memory_system.md)(存储形态基线,继续有效)、[memory_decision_sufficiency.md](memory_decision_sufficiency.md)(判据层,本文是其执行架构)

## 0. 定调(2026-08-10 用户拍板,后续不再重议)

1. **Memory 是控制系统,不是 RAG 模块**。优化对象是 Terminal Decision Quality(预算约束下),Recall@K 降级为中间诊断指标。一切设计问「这条记忆缺失/压缩/过期会造成多少决策损失」,不问「记得多像」。
2. **向量检索翻案(A-011)**:废止 memory_system.md §0「不要向量库」。但向量是**第二通道**——fingerprint 与 BM25 优先,dense 只在语义模糊场景触发;无 embedder 时系统必须完整可用。
3. **Embedder 走 provider 体系**:第一实现调 openai 兼容 `/embeddings`(含本地 ollama),复用现有 provider/代理配置。进程内模型(ort/candle/GGUF)只做后续 benchmark challenger,绝不 bundle。
4. **Lifecycle 五态当前实现**:candidate → shadow → active；candidate/shadow 可转 deprecated 或 invalid。旧文档里的 `stale` 只作兼容别名并归一为 deprecated；shadow 不注入生产，active 必须带真实 provenance。
5. **Evidence append-only**:state.db 的 events/episodes 是证据账本,自治的记忆流程(compiler/manager)**永远不得改写**;文献已证实持续 LLM consolidation 会让记忆效用先升后降,raw episode 是最后的兜底召回源。
6. **不做清单**:知识图谱引擎(只借 Graphiti 的 temporal/provenance 数据模型)、每回合 consolidation、默认 cross-encoder rerank、RL(遥测未积累前)、hard DELETE evidence、以语义相似度为 merge 判据。

## 1. 四模块架构

```text
Evidence Ledger      → 发生了什么(state.db events/episodes,append-only)
Memory Compiler      → 能编译出什么可复用控制知识(.kanzei/memory/*.md + provenance)
Recall Controller    → 当前这步决策需不需要记忆、用哪种检索(RecallPolicy)
Counterfactual Eval  → 这条记忆真的改善了决策吗(F(m)/D(S→m'),replay 实证)
```

数据流:

```text
SQLite events/episodes(不可变证据)
        │
        ▼
  Memory Compiler(后台)
        │
        ▼
.kanzei/memory/M-xxx.md(人可读派生记忆,真源)
        │
        ▼
index.db(FTS5 + 向量 + fingerprint,可重建派生物)
        │
        ▼
  RecallPolicy(在线,确定性)→ Memory Packet → LLM 决策
        │
        ▼
state.db 漏斗遥测(recall_events/memory_sources/memory_eval)
```

职责铁律:**Retriever 回答 WHAT,RecallPolicy 回答 WHETHER/WHEN/HOW**。

## 2. 漏斗遥测(R-161,Phase 0,一切的前提)

五段漏斗,每段机械可判:

```text
AVAILABLE(条目存在且 active)
  → RETRIEVED(某次 recall 动作选中它)
  → INJECTED(真进了本次上下文,含注入字节)
  → ACTION_CHANGED(注入后下一动作与失败前动作不同/停止重复)
  → OUTCOME_IMPROVED(该失败类不再出现/条目关闭/测试转绿)
```

三张新表落 **state.db**(必须与 episodes 同库才能 join;index.db 只放可重建索引):

```text
recall_events(episode_id, step_id, trigger_type, trigger_payload,
              policy_action, query, candidate_ids, retrieved_ids,
              injected_ids, lexical_ms, embed_ms, vector_ms, total_ms, at)
memory_sources(memory_id, episode_id, event_start, event_end, source_hash)
memory_eval(memory_id, replay_case, arm, model, prompt_version,
            success, steps, tool_errors, retries, tokens,
            first_divergence_step, at)
```

迁移口径:现有 index.db 的 memory_recalls 停写留读;`fetched` 采纳判定升级为 ACTION_CHANGED 的前身,并同时修掉两个盲区——read 工具读记忆文件路径要回填采纳(现只有 memory_search 回填),CLI 与桌面端必须同源接线(现状 harvest_sop 只在桌面端,见 D-229)。

## 3. 事件触发召回(R-162,Phase 1,最高 ROI)

### 3.1 钩位(已勘察确认)

`kanzei-core/src/runner.rs` 工具结果回喂前已有先例:`redundancy.note_step(&ctx.project_root, &calls, &mut results)`(R-100,runner.rs:2186)——每步在 `messages.push(Message::tool_results(...))` 之前就地改写结果文本、状态按单次运行持有。RecallPolicy 做成同款 watcher:

```rust
recall.note_step(project_root, &calls, &mut results);  // RedundancyWatch 的兄弟
```

**runner 主循环零架构改动。**

### 3.2 确定性策略(第一版无 RL)

```rust
enum RecallAction { NoOp, Fingerprint, Lexical, ReRetrieve }
```

R-294 决策门禁（2026-08-18）：当前生产配置没有 `[embeddings]`，运行时没有启用 dense/hybrid；`RecallAction` 只承诺以上四种已落地动作。`Hybrid` 仍是离线回放与显式 embeddings 配置下的可选实验通道，`PlanInject` 与 `StateAudit` 尚无真实消费方，因此不再列入运行时词表。

v1 只实现 NoOp/Fingerprint/Lexical/ReRetrieve,触发器:

| 触发 | 信号源 | 动作 |
| --- | --- | --- |
| 工具失败 | ToolResult is_error + 错误分类(复用 summarize_failures 的 tool/kind 逻辑,抽成在线可用的分类函数) | Fingerprint 精确匹配;miss 则 Lexical(错误原文+文件名+符号构 query) |
| 重复失败 | 同 (tool,kind) 本轮 ≥2 次,或同 action 签名重复 | ReRetrieve:换 query(加目标文件/意图词),不许原 top-k 重塞 |
| 意图边界 | bash 命令模式(如 cargo test → M-022 类)、条目状态切换 | Fingerprint/Lexical 按 trigger 表 |

预算(代码强制,超时降级不阻塞):Tier 0 fingerprint p95 < 5ms(内存 HashMap);Tier 1 BM25 p95 < 10ms;stuck 恢复路径放宽到 100–200ms。

### 3.3 Memory Packet(注入格式)

追加进对应 ToolResult 文本(与 [冗余提醒] 同机制),不是裸索引行:

```text
[记忆命中 M-009 | sop]
触发: edit 返回 old_string not found(本轮第 2 次)
行动: 先 read 当前文件重建 old_string 再重试 edit,不要继续猜
状态: active · 来源: episode E-1842 步 103-105
```

「触发」行必须写明**为什么现在想起它**——这是把背景知识变成控制规则的关键。同一运行内同条目只注入一次(watcher 内去重),防刷屏。

### 3.4 frontmatter 扩展(宽容读零迁移)

```text
fingerprint: [fp:edit|old_string not found]   # 升为一等字段,兼容正文内旧标记
trigger: tool_failure                          # tool_failure | intent | state_change
valid_from: 2026-08-07
supersedes: M-004                              # 版本链(superseded_by 反向已有)
version: 2
```

写入侧引擎维护 fingerprint → memory_id 的内存索引(启动扫描 + 写时增量)。

## 4. 回放评估台(R-163,Phase 2)

新模块(kanzei-eval 或 kanzei-core::replay):从 episodes/events 取历史轨迹,record/replay ToolResult(工具不真执行,回放录制结果),LLM 真调(fast 档跑批)。

六臂对照:

```text
A NoMemory      下界          B Current        现状
C Candidate     新策略        D Oracle         人工标定正确记忆 = 上界
E Leave-One-Out 单条记忆消融   F CompressionCF  合并前后行为对照
```

判读:C≪D → 触发/检索有问题;C≈D 仍失败 → 内容或 utilization 有问题。

J(结果函数)层级,**不用 LLM judge 做主判**:

```text
1 terminal 成功(条目关闭/测试转绿) 2 工具失败数 3 重试数
4 重复动作数 5 到达终态步数 6 token 成本;LLM judge 仅评软性 SOP 质量
```

环境固定:repo commit(episode 存 start_commit,可 checkout)、model、prompt 版本、temperature/seed;外部工具不可复现时一律 record/replay。首批 30–50 个 case 从 M-009/M-010/M-019/M-021/M-022/M-023/M-026 的触发历史提取——这些条目 trigger/action 明确,是天然测集。

核心指标分层:terminal_success → steps/tool_failures/retries → trigger_precision/recall、action_change_rate → 各段延迟。

## 5. 混合检索(R-164,Phase 3)

```rust
trait MemoryIndex {          // SqliteMemoryIndex 为默认实现
    fn search_lexical(...); fn search_dense(...); fn search_hybrid(...);
    fn upsert(...); fn remove(...); fn rebuild(...);
}
trait Embedder { fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>; }
```

- dense:sqlite-vec(rusqlite loadable extension),**brute-force 起步**,不依赖 experimental ANN;向量列在各 scope 的 index.db(派生物,可重建)。
- Embedder 第一实现:provider API `/embeddings`(定调 §0.3);无 embedder 时 hybrid 自动退化为 lexical,功能完整。
- Fusion:RRF(k=60),BM25 top10 + dense top10 → top5;禁止拍脑袋线性加权。
- Reranker 默认关闭,只允许 stuck 恢复与离线评估用。
- **启用门禁**:lexical-only vs dense-only vs hybrid 三臂先在 R-163 回放台对比,hybrid 显著优才设为默认——不要默认相信 dense。

### 5.1 R-294 路线结论（2026-08-18）

本轮按启用门禁核查了真实仓库数据，结论是**暂不启用 production hybrid**：

- 当前项目配置中 `[embeddings]` 为 0 个启用项；`EmbeddingsSection::enabled` 只有 provider 与 model 同时存在才打开通道，未配置时 `search_hybrid` 明确退化为 lexical（`crates/kanzei-harness/src/config.rs:151-168`、`crates/kanzei-memory/src/memory/index.rs:431-450`）。
- `crates/kanzei-memory/src/replay_eval.rs` 当前只有 5 个 fixture 测试，Candidate 无配置时与 Current 同源；这不是足以比较 lexical-only/dense-only/hybrid 的生产样本集，不能把 FakeEmbedder 测试结果冒充线上收益。
- 因此默认运行时正式降级为 lexical；`[embeddings]`、`memory_vectors` 与 hybrid RRF 保留为显式配置/离线评估能力，只有补齐真实三臂样本并证明 hybrid 显著优于 lexical 后才能重新启用默认值。

对应词表同步收缩见 §3.2：运行时只承诺 `NoOp/Fingerprint/Lexical/ReRetrieve`；`Hybrid` 仅属于离线/显式 opt-in 通道，`PlanInject` 与 `StateAudit` 不属于当前可调用能力。

## 6. Memory Compiler(R-165,Phase 4)

manager 从 CRUD 升级为编译语义:

```text
OBSERVE(读 inbox/episode 原料)→ PROPOSE(candidate 条目)
→ VERIFY(novelty gate + 转换检查)→ PROMOTE(candidate→active)
→ SUPERSEDE(版本链替代)→ DEPRECATE(降级,永不 hard delete)
```

- Novelty gate 三档(SAGE 式,省 LLM 成本):明显新 → 直接 PROPOSE;明显重复 → NOOP;不确定 → 才起 LLM 合并判断。
- 转换检查(TrustMem 三问):coverage(漏了关键信息吗)/preservation(破坏既有正确信息吗)/faithfulness(写了证据里没有的东西吗)。
- 后台触发扩展(现状只有轮末):compaction 边界(工作记忆正在丢信息,顺手检查该沉淀什么)、recurrence(同类事件第 2 次才升 candidate、第 3 次+修复成功才 PROMOTE,替代「一次失败即总结」)、idle debounce、memory pressure(active>500 或检索精度下降才起重整理)。
- provenance 硬约束:每条 PROMOTE 的条目必须带 memory_sources 行(episode 区间),无来源不入 active。
- 归档落地(修 D-231):deprecated/invalid 条目由整理流程移入 `archive/`,墓碑保留,FTS 只在 status=any 时可见。
- merge 判据切换:语义相似只产生**候选**,PROMOTE 前置条件是 R-166 的 D(S→m')<ε(评估器未落地前,merge 一律保守:只合并 fingerprint 相同或用户确认的)。

## 7. 反事实评估器(R-166,Phase 5)

```text
F(m)  = E[J(e; M) − J(e; M∖{m})]      遗忘成本/干预遗憾
D(S→m') = E[J(e; M) − J(e; (M∖S)∪{m'})]  合并失真
```

- **离线定向回放,绝不在线算**:每条 memory 维护 Q(m)=触发匹配的历史 episode + near-miss + negative control;周期性跑 with/without,落 memory_eval,维护 effect_mean/effect_ci/eval_n/last_eval。
- 只有 low value + high confidence 才进 deprecate 候选;age ≠ forgettable(硬约束类条目三个月不触发也不许按时间衰减淘汰)。
- shadow 态已由 R-286 控制面与 R-166 评估路径接入(五态齐):candidate → shadow(可被评估、不注入生产)→ active；deprecated/invalid 保留可追溯墓碑。
- merge 由 D<ε 把关——压缩从文本操作变成有测试的行为等价变换。

## 8. 学习型召回控制器(R-167,占位)

contextual bandit 调度 RecallAction(state:goal/tool/error/phase/stuck 计数/上次召回结果;reward:任务成功−失败成本−token−延迟)。先决条件:R-161 遥测 + R-163 回放台积累的 reward 数据。此前一律确定性策略。

## 9. 实现顺序与依赖

```text
R-161 遥测 ──→ R-162 事件触发召回 ──→ R-164 混合检索
   │                │                     ↑
   └──→ R-163 回放评估台 ─────────────────┘(启用门禁)
              │
R-165 Compiler(依赖 R-162 的 trigger 字段)
              │
R-163 + R-165 ──→ R-166 反事实评估器 ──→ R-167(远期)
```

排序理由:没有 R-161 就无法区分「真变好」与「感觉变好」;R-162 是已证明知识存在、只缺 runtime dispatch 的最高 ROI 改动;R-164 在回放台对比前不许默认启用。

## 10. 与既有文档的关系

- memory_system.md:存储形态(文件真源/派生物/写读分离)全部沿用;§0「不要向量库」被 A-011 废止,其余品味决策不动。
- memory_decision_sufficiency.md:其判据层(决策价值、复发检测、subject 不变量)是本文的理论前身;R-149/R-150/R-151 的产出(recall 遥测、零采纳候选、机械捕获占位)分别被 R-161/R-166/R-165 吸收扩展,不冲突。
- 既有代码资产映射:summarize_failures→在线错误分类器;[fp:] 指纹+find_active_by_marker→Tier 0 dispatch;RedundancyWatch→RecallWatch 钩位先例;memory_recalls.fetched→漏斗前身;manager 迷你 run+inbox→Compiler 宿主;episodes(含 overflow/metrics)→回放原料。
