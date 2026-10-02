# 研究记忆

本文件保存可跨回合复用的研究结论。每条结论必须同时记录来源 ID；没有来源的内容不得作为已验证事实注入研究上下文。

## 结论

- 结论：当前网络环境下 websearch(DuckDuckGo 端点)直连与走本地代理 127.0.0.1:12000 均不可达;arXiv API(export.arxiv.org)直连可用;anthropic.com / docs.langchain.com / docs.letta.com / docs.mem0.ai 直连可用;openai.com 返回 403。研究检索通道应默认 webfetch + arXiv API,websearch 不可依赖。
  来源：S-001(websearch 工具实现端点)+ 2026-08-16 本会话网络实测(V3)
  适用范围：任何需要网络检索的研究会话(kanzei 研究模式)
  更新时间：2026-08-16

- 结论：kanzei 记忆系统=decision-centric 控制系统(非 RAG):markdown 文件真源 + SQLite 可重建派生物;scope(project)×category(preference/habit/fact/sop);写入走准入链多闸(source==user 直 active,manager 产物落 candidate);五态生命周期 candidate→shadow→active→deprecated/invalid + provenance 硬门禁(无来源不入 active、episode 真实性校验);检索三通道 fingerprint/BM25/dense + RRF(k=60)+ 采纳率决策权重(0.6+0.7×rate,召回≥3);RecallWatch 工具失败触发注入 [记忆命中] Packet + 轮末 ACTION_CHANGED 对账;F(m) 反事实遗忘成本 + 六臂回放评估台。
  来源：S-014~S-019(V1/V2,锚点见 .kanzei/research/report.md §3)
  适用范围：任何讨论 kanzei 记忆系统机制、对照、改动、回归的研究/开发上下文
  更新时间：2026-08-16

- 结论：agent memory 谱系坐标(对照 kanzei 用):MemGPT/Letta=OS 式虚拟上下文管理;Generative Agents=记忆流+反思(reflection);Mem0=提取-整合-检索流水线(LOCOMO);Zep=时序知识图谱(Graphiti,LongMemEval 优势);A-MEM=Zettelkasten 动态链接;RAG=参数+非参数化记忆(provenance 动机);LangGraph=短/长程记忆 + Profile vs Collection;Anthropic=context rot/attention budget。kanzei 独特位置=「控制论取向、决策价值判据、反事实评估闭环、工程纪律极强的经验记忆系统」。
  来源：S-002~S-013(V2,均为摘要级一手来源——见 sources.md 证据深度标注;CoALA 四类记忆经 D-412 正文核验升正文级)
  适用范围：agent memory 相关研究、kanzei 记忆系统设计决策
  更新时间：2026-08-16

- 结论：kanzei 记忆系统相对文献的差距:无生成式整理(reflection/consolidation,晋升靠复发≥3+指纹机械判定)、无语义相似合并(评估器未落地,merge 保守)、无知识图谱(Zep 式,系设计显式品味决策「不要知识图谱」)、R-167 学习型召回控制器未实现(占位)、记忆主体单一(agent-centric,subject 键为雏形)。优势:provenance 硬门禁、决策价值判据、反事实评估闭环、触发式召回工程化。
  来源：S-014/S-016/S-017/S-018 + 对照 S-002~S-013(V1,摘要级)
  适用范围：kanzei 记忆系统后续演进决策
  更新时间：2026-08-16
