# Sources

## S-001 kanzei websearch 工具实现:crates/kanzei-tools/src/websearch.rs(端点 html.duckduckgo.com/html/,代理策略 tool_proxy) [active]
- 类型: 代码域(工具实现)
- 证据锚: crates/kanzei-tools/src/websearch.rs:9-77
- 说明: websearch 唯一端点=DuckDuckGo HTML;proxy 策略来自 tool_proxy(Env→系统注册表),当前会话无代理环境变量、注册表 ProxyEnable=0,故走直连

## S-002 MemGPT: Towards LLMs as Operating Systems (arXiv 2310.08560) [active]
- URL: https://arxiv.org/abs/2310.08560
- 作者: Charles Packer et al.
- 年份: 2023
- 类型: 文献(一手,论文原文,**摘要级**,2026-08-16 D-412 标注)
- 要点: virtual context management 借鉴 OS 分层内存;main memory(上下文内)+external context(上下文外,按需调入);interrupts 控制流;评估域=长文档分析+多会话聊天

## S-003 Zep: A Temporal Knowledge Graph Architecture for Agent Memory (arXiv 2501.13956) [active]
- URL: https://arxiv.org/abs/2501.13956
- 作者: Preston Rasmussen et al.
- 年份: 2025
- 类型: 文献(一手,论文原文,**摘要级**,2026-08-16 D-412 标注)
- 要点: Graphiti 时序知识图谱引擎;DMR 94.8% vs MemGPT 93.4%;LongMemEval 上最高 +18.5% 精度、延迟 -90%;针对跨会话信息综合与长期上下文保持

## S-004 Generative Agents: Interactive Simulacra of Human Behavior (arXiv 2304.03442) [active]
- URL: https://arxiv.org/abs/2304.03442
- 作者: Joon Sung Park et al.
- 年份: 2023
- 类型: 文献(一手,论文原文,**摘要级**,2026-08-16 D-412 标注)
- 要点: LLM 扩展架构:自然语言记忆流(memory stream)+回忆(retrieval:新近/重要/相关)+反思(reflection)+规划;25 个 agent 小镇仿真;消融证明 observation/planning/reflection 各贡献

## S-005 Mem0: Building Production-Ready AI Agents with Scalable Long-Term Memory (arXiv 2504.19413) [active]
- URL: https://arxiv.org/abs/2504.19413
- 作者: Prateek Chhikara et al.
- 年份: 2025
- 类型: 文献(一手,论文原文,**摘要级**,2026-08-16 D-412 标注)
- 要点: 动态提取-整合-检索(extract/consolidate/retrieve);图增强变体;LOCOMO 基准;相对 OpenAI 26% LLM-as-Judge 提升;比 full-context p95 延迟 -91%、token 省 >90%

## S-006 Retrieval-Augmented Generation for Knowledge-Intensive NLP Tasks (arXiv 2005.11401) [active]
- URL: https://arxiv.org/abs/2005.11401
- 作者: Patrick Lewis et al.
- 年份: 2020
- 类型: 文献(一手,论文原文,**摘要级**,2026-08-16 D-412 标注)
- 要点: RAG 基线:参数化记忆(seq2seq)+非参数化记忆(Wikipedia dense index);NeurIPS 2020;提供 provenance 与知识更新的动机

## S-007 A-MEM: Agentic Memory for LLM Agents (arXiv 2502.12110) [active]
- URL: https://arxiv.org/abs/2502.12110
- 作者: Wujiang Xu et al.
- 年份: 2025
- 类型: 文献(一手,论文原文,**摘要级**,2026-08-16 D-412 标注)
- 要点: Zettelkasten 式动态索引与链接;新记忆生成含多结构属性的笔记;与历史记忆建链;新记忆可触发既有记忆上下文表示更新(记忆演化);NeurIPS 2025

## S-008 Cognitive Architectures for Language Agents (CoALA) (arXiv 2309.02427) [active]
- URL: https://arxiv.org/abs/2309.02427
- 作者: Theodore R. Sumers, Shunyu Yao et al.
- 年份: 2023
- 类型: 文献(一手,论文原文,**正文级**,2026-08-16 D-412 取正文核验)
- 要点: CoALA:模块化记忆(working/episodic/semantic/procedural)+结构化动作空间+决策过程;认知科学谱系;TMLR
- 证据深度: 四类记忆划分**仅见正文**(§2.3 Soar memory:procedural/semantic/episodic 各自定义;§4.1 Memory 分类),摘要只写 "modular memory components" 不含四词——此条目此前误标 V2(摘要级),D-412 取 arXiv HTML 全文核验(episodic×30/semantic×121/procedural×26/working memory×29)后确认正文支撑成立,保留 V2 但必须标注正文级

## S-009 LangGraph Memory overview(官方文档) [active]
- URL: https://docs.langchain.com/oss/python/langgraph/memory
- 类型: 文献(一手,官方文档,**正文级**,D-412 根因确认「取的是正文 HTML,三类映射证据充分」)
- 要点: 短程记忆=thread-scoped 状态+checkpointer;长程记忆=namespace 持久存储;类型表 semantic/episodic/procedural(引 CoALA);语义记忆两形态:Profile(单文档持续更新)vs Collection(文档集);写入时机 hot path vs background(引 Trustcall)

## S-010 Anthropic: Effective context engineering for AI agents(2025-09-29) [active]
- URL: https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents
- 作者: Anthropic Engineering
- 年份: 2025-09-29
- 类型: 文献(一手,厂商工程博客,**正文级**,2026-08-16 D-412 标注)
- 要点: context engineering;context rot(上下文越长效召越差);attention budget;系统提示/工具/MCP/消息历史的上下文策展;memory 是上下文管理的组成部分

## S-011 Letta Docs: Stateful agents(官方文档) [active]
- URL: https://docs.letta.com/concepts/stateful-agents
- 类型: 文献(一手,官方文档,**正文级**,2026-08-16 D-412 标注)
- 要点: Letta(前 MemGPT)stateful agent:长期记忆存 MemFS;记忆跨会话共享(一个会话学到的偏好改善另一会话)

## S-012 A Survey on the Memory Mechanism of LLM based Agents (arXiv 2404.13501) [active]
- URL: https://arxiv.org/abs/2404.13501
- 作者: Zeyu Zhang et al.
- 年份: 2024
- 类型: 文献(一手,综述,**摘要级**,2026-08-16 D-412 标注)
- 要点: LLM 代理记忆机制综述:what/why/how;记忆模块设计(读写/存储/检索/遗忘)与评估;39 页

## S-013 A Survey of Agent Memory in the Second Half: Towards Self-Evolving and Long-Horizon Agents (arXiv 2602.06052) [active]
- URL: https://arxiv.org/abs/2602.06052
- 作者: Jiang et al.
- 年份: 2026
- 类型: 文献(一手,综述,TMLR,**摘要级**,2026-08-16 D-412 标注)
- 要点: agent memory 三维框架:memory substrate(内参 vs 外取)/cognitive mechanism(sensory/working/episodic/semantic/procedural)/memory subject(user-centric vs agent-centric);记忆操作学习策略;TMLR 2026

## S-014 kanzei memory 控制平面设计基线(docs/design/memory_control_plane.md) [active]
- 类型: 代码域(设计基线)
- 要点: Memory 是控制系统不是 RAG;优化对象=Terminal Decision Quality;四模块=Evidence Ledger/Compiler/Recall Controller/Counterfactual Eval;A-011 向量翻案(第二通道);Lifecycle 四态;证据 append-only
- 证据锚: docs/design/memory_control_plane.md:0-10(定调)、:20-49(四模块架构)、:129-152(回放台)、:170-186(Compiler)

## S-015 kanzei Memory 决策充分性设计(docs/design/memory_decision_sufficiency.md) [active]
- 类型: 代码域(设计基线)
- 要点: 写入/遗忘/压缩/检索判据=决策价值而非语义显著度;反事实写入闸、subject 状态语义、复发检测、采纳率排序;八维审计 §5 实证
- 证据锚: docs/design/memory_decision_sufficiency.md:15-22(四条判据)、:50-59(五层映射)、:60-67(方案)

## S-016 kanzei Memory 系统设计(docs/design/memory_system.md) [active]
- 类型: 代码域(设计基线)
- 要点: 文件优先(markdown 真源)+SQLite 只存可重建派生物;scope=global/project(global 已废弃 R-194);category=preference/habit/fact/sop(episode 落 state.db);写读分离(memory_note 投递+manager 决定)
- 证据锚: docs/design/memory_system.md:8-13(定调)、:15-33(scope×category)、:55-62(硬门禁)、:116-125(工程决策)

## S-017 kanzei 记忆核心实现(crates/kanzei-memory/src/memory/store+admission+lifecycle+search+mod) [active]
- 类型: 代码域(核心实现)
- 要点: add 准入链:validate_basic→subject 冲突→交付状态拒收→指纹一致性→novelty 三档→标题判重;source==user 直 active;candidate 需 promote 带证据;状态机 candidate→shadow→active/deprecated;CATEGORIES=[preference,habit,fact,sop];STATUSES=[candidate,shadow,active,deprecated,invalid]
- 证据锚: crates/kanzei-memory/src/memory/store.rs:247-366(add 主流程)、admission.rs:34-169(准入策略)、lifecycle.rs:24-121(状态机+provenance)、search.rs:99-150(classify_novelty)、mod.rs:38-66(枚举)

## S-018 kanzei 检索/触发/反事实评估实现(index.rs+recall.rs+eval.rs) [active]
- 类型: 代码域(检索与评估实现)
- 要点: 检索三通道 fingerprint/BM25/dense,hybrid=RRF(k=60)BM25 top10+dense top10→top5,无 embedder 退化 lexical;decision_weight=0.6+0.7×采纳率(召回≥3);RecallWatch 工具失败触发→[记忆命中] Packet→轮末 ACTION_CHANGED 对账;F(m)=Current−LeaveOneOut 配对差+95%CI,落 memory_eval_agg
- 证据锚: crates/kanzei-memory/src/memory/index.rs:34-40(decision_weight)、:106-110(三通道)、:391-477(dense+RRF hybrid)、retrieval/search.rs:21-90(FTS 候选集)、crates/kanzei-core/src/runner/recall.rs:64-84(RecallPolicy trait)、:89-194(RecallWatch)、crates/kanzei-core/src/store/eval.rs:17-135(F(m) 聚合)

## S-019 kanzei 六臂回放评估台+docstore S/F 文档(replay_eval.rs+docstore.rs) [active]
- 类型: 代码域(评估台/文档存储)
- 要点: 六臂回放:NoMemory/Current/Candidate/Oracle/LeaveOneOut/CompressionCF(测试锚);LLM 真调回放录制工具结果;S-/F- 来源与发现是 docstore 的 DocKind,研究侧证据载体
- 证据锚: crates/kanzei-memory/src/replay_eval.rs:32-99(ReplayMemoryProvider)、:217-237(LlmDecider)、:302-415(六臂测试锚)、crates/kanzei-memory/src/docstore.rs:71-165(DocKind: SOURCES/FINDINGS/MEMORY)
