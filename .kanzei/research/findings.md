# Findings

## F-001 kanzei 记忆定位:控制系统非 RAG 模块(decision-centric) [confirmed]
- 域: 代码
- 等级: V1
- 结论: kanzei 记忆系统定位=decision-centric 控制系统而非 RAG 模块:优化对象是 Terminal Decision Quality(预算约束下),Recall@K 只是中间诊断指标
- 证据锚: docs/design/memory_control_plane.md:11(定调1)
- refs: S-014

## F-002 kanzei 存储形态:文件真源+可重建派生物,scope×category 分级 [confirmed]
- 域: 代码
- 等级: V1
- 结论: 存储形态=markdown 文件真源(可编辑可 git 恢复)+SQLite 只存可重建派生物(FTS5/向量/fingerprint);分级=scope(project;global 已废弃)×category(preference/habit/fact/sop;episode 落 state.db)
- 证据锚: docs/design/memory_system.md:8-13,55-62;mod.rs:38-66
- refs: S-016 S-017

## F-003 kanzei 写入管线:准入链多闸+novelty 三档+user 直写直 active [confirmed]
- 域: 代码
- 等级: V2
- 结论: 写入准入链=validate_basic→subject 冲突→交付状态拒收→指纹一致性→classify_novelty 三档(明显新/明显重复/不确定留 LLM)→精确+近似标题判重;source==user 直写直接 active,manager 产物落 candidate 须 promote
- 证据锚: store.rs:247-366;admission.rs:34-169;search.rs:99-150
- refs: S-017

## F-004 kanzei 生命周期:五态状态机+provenance 硬门禁 [confirmed]
- 域: 代码
- 等级: V2
- 结论: 生命周期五态 candidate→shadow→active→deprecated/invalid;provenance 硬门禁=无 memory_sources 证据不入 active、episode_id 必须真实存在(防编造)、证据先落库全部成功才置 active;复发≥3+带指纹+当轮 episode 才自动晋升;超龄 14 天清退
- 证据锚: lifecycle.rs:24-88,93-144;mod.rs:64
- refs: S-017

## F-005 kanzei 检索:三通道+RRF 融合+采纳率决策权重 [confirmed]
- 域: 代码
- 等级: V2
- 结论: 检索三通道统一入口=fingerprint(Tier0 精确)/BM25(FTS5)/dense(向量,brute-force cosine);hybrid=RRF(k=60)BM25 top10+dense top10→top5,无 embedder 自动退化 lexical;排序折入决策权重 decision_weight=0.6+0.7×采纳率(召回≥3 生效,不清零)
- 证据锚: index.rs:34-40,106-110,391-477;retrieval/search.rs:21-90
- refs: S-018

## F-006 kanzei 触发召回:RecallWatch 事件触发+ACTION_CHANGED 遥测 [confirmed]
- 域: 代码
- 等级: V2
- 结论: 事件触发召回=RecallWatch 挂在工具结果回喂前:工具失败→失败计数(同 tool+kind)→RecallPolicy.retrieve(Tier0 fingerprint→Tier1 BM25)→[记忆命中] Packet 追加进结果文本(同轮同条目只注入一次)→轮末 Drop 对账按失败计数是否再涨机械判定 ACTION_CHANGED;miss 也落遥测
- 证据锚: runner/recall.rs:64-84,89-194,196-226
- refs: S-018

## F-007 kanzei 遗忘与评估:F(m) 反事实聚合+六臂回放评估台 [dropped]
- 域: 代码
- 等级: V2
- 结论: 反事实遗忘成本 F(m)=E[J(e;M)−J(e;M∖{m})] 离线聚合:同 case 的 Current vs LeaveOneOut 两臂 success 配对差+95%CI 落 memory_eval_agg;六臂回放=NoMemory/Current/Candidate/Oracle/LeaveOneOut/CompressionCF,LLM 真调+工具结果录制回放
- 证据锚: store/eval.rs:17-135;replay_eval.rs:32-99,217-237,302-415
- refs: S-018 S-019

## F-008 MemGPT:OS 式虚拟上下文管理(上下文外记忆先例) [confirmed]
- 域: 文献
- 等级: V1(摘要级,D-412 口径:摘要级封顶 V1)
- 结论: MemGPT 提出 virtual context management:借鉴 OS 分层内存,main memory(上下文内)+external context(上下文外按需调入),interrupts 控制流;评估域=长文档分析+多会话聊天——kanzei 的上下文注入/召回与 MemGPT 同属上下文外记忆分层思想
- 证据锚: arXiv 2310.08560 摘要(摘要级)
- refs: S-002

## F-009 Generative Agents:记忆流+反思的生成式记忆整理 [confirmed]
- 域: 文献
- 等级: V1(摘要级,D-412 口径:摘要级封顶 V1)
- 结论: Generative Agents=自然语言记忆流(memory stream)+检索(新近/重要/相关)+反思(reflection 高层推论)+规划;消融证明 observation/planning/reflection 各贡献——记忆的生成式整理(reflection)是 kanzei 尚未具备的能力(kanzei 靠 recurrence+指纹机械晋升)
- 证据锚: arXiv 2304.03442 摘要(摘要级)
- refs: S-004

## F-010 Mem0:提取-整合-检索流水线+LOCOMO 基准 [confirmed]
- 域: 文献
- 等级: V1(摘要级,D-412 口径:摘要级封顶 V1)
- 结论: Mem0=动态提取-整合-检索(extract/consolidate/retrieve)+图增强变体;LOCOMO 基准上四类问题全面超越六类基线;比 full-context p95 延迟 -91%、token 省 >90%——「对话即记忆源、按需提炼」与 kanzei 的「失败信号+轮末收尾」写入触发是两条路线
- 证据锚: arXiv 2504.19413 摘要(摘要级)
- refs: S-005

## F-011 网络环境:websearch(DDG)不可用,arXiv API 直连可用(研究通道结论) [confirmed]
- 域: 环境
- 等级: V3
- 结论: 当前网络环境:DuckDuckGo(websearch 唯一端点)直连与走本地代理 127.0.0.1:12000 均超时不可达;arXiv API(export.arxiv.org)直连可用;anthropic.com/docs.langchain.com/docs.letta.com/docs.mem0.ai 直连可用;openai.com 403——研究检索通道应为 webfetch+arXiv API,websearch 不可依赖
- 证据锚: 本会话网络实测 + S-001
- refs: S-001
