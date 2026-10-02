---
kind: prior_art
topic: memory-frontend-management
status: complete
trigger: explicit_user
entry_refs: R-333
websearch_round_limit: 4
---

# 先行方案对照：记忆前端与管理模型

## 0. 结论摘要

本轮不建议把现有 Memory 页继续堆成“架构图 + 诊断面板 + 条目列表 + 召回账单 + 编辑器”的单页。外部方案的共同有效点不是某个漂亮的卡片，而是把**当前状态、可检索知识、历史事件、证据来源和管理动作**分开，并让用户知道每条内容为何存在、何时会被使用、如何修改或退役。

建议仓内下一版采用“三平面 + 一条证据链”：

1. **工作状态平面**：少量、结构化、当前有效的 profile/决策/项目状态；直接服务下一次任务，不和历史事实混在一起。
2. **长期知识平面**：继续使用 Markdown 文件作为真源，按 fact/sop/habit/preference 管理，保留 provenance、refs、生命周期和墓碑；检索索引只是派生物。
3. **经历与效果平面**：episode、recall、fetched、effect 和上下文账单作为只读时间线/评估数据，不把统计诊断伪装成可编辑记忆。
4. **证据链**：每条记忆从来源事件/文件到条目、召回、采纳和退役都有可跳转链路。

当前仓内已经具备文件真源、生命周期、召回/采纳遥测和候选整理能力；本轮发现的主要问题是前端把这些不同语义压成一个长页面，且有几处前后端/设计基线不一致。外部方案的向量库、知识图谱和托管服务不作为默认真源，仅可作为可替换检索适配器。

## 仓内既有设计

**当前仓内审计（B1）**

### 1.1 global scope 已在设计上废弃，但前端和 Tauri 仍把它当可用域
- 出处: file:docs/design/memory_system.md:18
- 证据等级:V1
- 证据深度:读码核实；`memory_system.md` 明确 global 已废弃且检索/常驻路径不再遍历，全局存量已归档，但 `memory_stores_for` 仍加入 global，筛选器仍提供“全局记忆”，scope=all 仍并发读取 global。
- 复现步骤:打开 Memory 页，在范围筛选选择“全局记忆”或“全部”，再切换项目；观察 global 仍以现行可维护范围出现，且 scope=all 仍发起读取。
- 差异:产品设计把 global 定义成历史兼容/归档边界，当前 UI 却给用户一个看似可维护的现行域；用户无法判断全局条目是否会参与当前项目召回。
- 决策:保留文件层的历史兼容读取，但下一版 UI 默认隐藏 global 现行入口；若必须查看归档，单独标为“历史/只读”。在用户重新确认 global 语义前，不恢复跨项目写入。

### 1.2 删除操作绕过归档/墓碑契约，且 UI 文案承诺不可撤销
- 出处: file:docs/design/memory_system.md:54
- 证据等级:V1
- 证据深度:读码核实；设计要求 deprecated/invalid 搬到 archive 并保留墓碑，Tauri command 却直接 `std::fs::remove_file`，UI 又把动作标成“从磁盘删除，不可撤销”。
- 复现步骤:在 Memory 条目详情点击删除，观察命令直接删除文件且 UI 提示不可撤销；随后检查 archive/墓碑与 INDEX/FTS 是否同步。
- 差异:前端的“删除”不是生命周期退役，可能破坏 git 以外的当场恢复体验，也和 M-059 清理 SOP 的“归档不裸删、清后三处一致”冲突。
- 决策:下一版把动作拆成“标记失效/归档”和明确的“永久清理（需另一个高风险确认）”；默认删除只产生墓碑并刷新 INDEX/FTS。该问题应在方案评审后登记为独立实现缺陷，当前 R-333 不直接修复。

### 1.3 列表、搜索和刷新没有请求代际保护，快速操作可能被旧响应覆盖
- 出处: file:crates/kanzei-app/ui/13-memory.js:63
- 证据等级:V1
- 证据深度:读码核实；`refreshMemory` 与 `loadMemoryList` 均异步等待多个 invoke，完成后直接写入 DOM，没有 request id、AbortController 或当前筛选快照校验；筛选变更、搜索、整理完成后的 refresh 可以交叠。搜索清除还直接调用 `loadMemoryList`，没有取消旧搜索。
- 复现步骤:在 Memory 页连续输入搜索词、快速切换范围并触发整理/刷新；在不同延迟的返回先后下观察列表，旧请求结果可能覆盖最后一次筛选。
- 差异:UI 看起来像当前筛选的结果，实际上旧请求返回顺序可决定最终列表；这是比“视觉不好”更严重的事实投影问题。
- 决策:下一版把页面状态建成单一 view model，所有读取带 generation，只有最新 generation 能提交；写操作完成后以服务端返回的条目/事件为准重放，不依赖并发 refresh 的先后。需要补“快速切换筛选/搜索/整理”的异步回归。

### 1.4 清除搜索不清除详情，项目为空时也会遗留旧项目内容
- 出处: file:crates/kanzei-app/ui/13-memory.js:55
- 证据等级:V1
- 证据深度:读码核实；清除搜索只清空 input 后调用普通列表加载，没有把 `memoryCurrentEntryId` 置空或调用 `hideMemoryDetail`；`refreshMemory` 在 `currentProject` 为空时只更新 `memory-arch`，列表、详情、诊断区仍可能保留旧项目数据。
- 复现步骤:先选中一个条目打开详情，再清除搜索或切换到空项目；观察详情/诊断区是否仍显示旧条目和旧项目内容。
- 差异:用户切换项目或结束搜索后，右侧详情可能继续显示已不属于当前列表/项目的条目，破坏“当前所见即当前作用域”的基本认知。
- 决策:所有作用域、搜索、项目变更都通过同一个 `resetMemoryView` 清理列表选中态、详情和诊断状态；空项目显示统一空态，不只替换架构卡片。

### 1.5 单个面板失败会使整页刷新失败，成功/失败状态没有按区域隔离
- 出处: file:crates/kanzei-app/ui/13-memory.js:68
- 证据等级:V1
- 证据深度:读码核实；六个 Promise 并发调用被一个总 try/catch 包住，任一 `memory_overview`、账单、召回、候选、价值标记或控制面命令失败，后续区域全部不渲染，只显示一个总 toast。
- 复现步骤:让任一诊断类 invoke（例如账单或召回）返回错误，同时刷新 Memory 页；观察列表等核心区域也被总 try/catch 短路，只剩页面级 toast，无法就地重试单一区域。
- 差异:控制面、列表、账单是不同信息域，错误却被建模成一个页面级错误；用户既不知道哪块失败，也无法只重试失败区域。
- 决策:下一版按数据域拆成独立 resource 状态（loading/ready/error/empty），每个区域有就地错误和重试；列表/详情优先可用，诊断数据缺失不能遮蔽核心条目管理。

### 1.6 前端 IPC 字段命名在同一记忆数据流中混用 camelCase 与 snake_case
- 出处: file:crates/kanzei-app/src/memory.rs:94
- 证据等级:V1
- 证据深度:读码核实；同一内部 Tauri API 一部分字段使用 camelCase，另一部分使用 snake_case，违背仓内业务契约的统一命名规则。
- 复现步骤:打开 Memory 页并抓取同一轮 Tauri 返回 payload，逐项对照字段名；观察 `hitsTotal`/`inboxPending` 与 `rounds_total`/`rounds_with_fetch` 同时存在两种命名风格。
- 差异:这会让前端适配层不断记忆例外，增加新页面或 API 迁移时的错接概率；也使结构化 schema 难以机械比较。
- 决策:新设计统一 snake_case；兼容转换只放在边界适配层，不在 `13-memory.js` 同时接受两套名字。该迁移另立实现批次，避免与视觉重做混杂。

### 1.7 现有冒烟覆盖了 fixture 的快乐路径，但当前基线本身不能作为页面无 BUG 证据
- 出处: file:scripts/ui-runtime-smoke.mjs:3366
- 证据等级:V2
- 证据深度:本地运行时实测；2026-08-当前 Node.js v22.14.0 执行 `node scripts/ui-runtime-smoke.mjs` 在 `vm.SourceTextModule is not a constructor` 处失败，随后出现 `neuralFlowEmit` 未注册，未进入完整记忆页断言。既有 fixture 断言覆盖列表、搜索点击、编辑、候选、召回和宽度，但没有覆盖请求乱序、总刷新部分失败、项目切换清理、删除归档契约和 global 废弃语义。
- 复现步骤:在当前 Node.js 环境执行 `node scripts/ui-runtime-smoke.mjs`，观察在 Memory 断言前因 `vm.SourceTextModule is not a constructor` 退出；再执行 `node --experimental-vm-modules scripts/ui-runtime-smoke.mjs`，确认实验参数可进入并通过现有 27 脚本序列，但新增问题路径仍无断言。
- 差异:测试名称和既有注释容易让人以为 Memory UI 已被充分验证，实际执行模型在当前环境先于断言崩溃，且已有断言主要验证静态 payload 下的单一路径。
- 决策:不把既有 fixture 通过当作真实 UI 质量结论；后续先修复/确认 ESM smoke 执行环境，再补当前问题的异步与空态覆盖。失败证据已记录，不在本轮改 harness。

## 外部已有实现

### 2.1 Mem0：自动提取的托管记忆层
- 出处: https://docs.mem0.ai/platform/overview
- 证据等级:V2
- 证据深度:官方文档正文级；正文明确描述 Add → Extract and store → Recall 三步，服务把对话蒸馏为事实并关联实体，按查询返回相关记忆；同时提供审计日志和 workspace governance。
- 差异:Mem0 把记忆视为服务侧抽取/检索产品，真源、编辑体验和治理依赖托管平台；它比仓内文件方案更省基础设施，但透明改写、离线恢复和逐行审阅能力更弱。
- 决策:不采用托管平台作为 Kanzei 真源；采用其“捕获、抽取、召回”分离以及 workspace 级治理/审计的产品分层，映射为仓内 candidate→active、provenance 和可见召回链。

### 2.2 Zep/Graphiti：会话记忆 + 用户知识图谱
- 出处: https://help.getzep.com/v2/memory
- 证据等级:V2
- 证据深度:官方文档正文级；文档说明 `memory.add` 以 session chat messages 为输入并构建 user-level knowledge graph，`memory.get` 按当前 session 上下文返回 context、近期消息和 raw facts，也支持 group graphs 与低层 graph search。
- 差异:Zep 将“最近消息”“抽取事实”“图谱关系”组合成按会话查询的上下文，适合跨会话个性化；仓内设计刻意拒绝知识图谱，强调 Markdown 可审阅和项目隔离，因此可解释性边界不同。
- 决策:不引入图谱作为默认模型；采用“短期会话上下文与长期记忆分开”的结构，并把当前 UI 的 recall rounds 与条目详情连成时间线。若未来需要关系视图，只做可重建的派生视图，不让关系图成为真源。

### 2.3 Letta：核心 Memory blocks + 按需 archival memory
- 出处: https://docs.letta.com/v1-sdk/memory/archival-memory
- 证据等级:V2
- 证据深度:官方文档正文级；文档区分始终可见、可修改的 memory blocks 与 agent 通过工具按需查询的 archival memory，后者支持语义搜索、标签组织，开发者可 list/update/delete；同时把 conversation search 与 archival knowledge 区分。
- 差异:Letta 的“上下文层级”非常适合解释为什么某些状态要常驻、某些知识必须显式取回；但 archival memory 具有 agent-immutable 和服务 API 约束，与仓内人可直接编辑 Markdown、git 恢复的原则不同。
- 决策:采用“常驻工作状态 / 按需长期知识 / 历史对话搜索”三层心智模型；保留仓内文件可编辑性和墓碑恢复，不采用 agent-immutable 作为默认权限规则。

### 2.4 LangMem：semantic / episodic / procedural 三类记忆，profile 与 collection 分流
- 出处: https://langchain-ai.github.io/langmem/concepts/conceptual_guide/
- 证据等级:V2
- 证据深度:官方文档正文级；文档区分 semantic facts、episodic past experiences、procedural system behavior，并明确 profile 适合有固定 schema 的当前状态，collection 适合大量可检索知识；召回还应综合相似度、重要性、近期/频繁使用强度。
- 差异:LangMem 直接把“当前状态”和“可增长集合”分开，能避免把 preference、SOP、episode 全塞进同一列表；它通常依赖应用自己的 store 和 LLM 更新器，仓内则已有 Markdown/SQLite 与 manager 工具。
- 决策:采用 semantic/episodic/procedural 的分类思路，但映射到仓内已有 category 和 episode 表，不新增第三套分类枚举；为 preference/项目状态增加结构化 profile 视图，collection 仍由 Markdown 条目承担。

**外部方案逐项覆盖矩阵**

| 方案 | 对象模型 | 捕获/检索 | 编辑/删除 | 来源/权限 | 用户反馈/审计 |
|---|---|---|---|---|---|
| Mem0 | 对话蒸馏出的事实与实体关联记忆 | Add → Extract and store → Recall | 资料入口未把本地人工编辑/删除作为核心 UI；治理由平台承担 | workspace governance 与 audit logs；托管边界 | 审计日志可追踪写入，召回是使用反馈；不等同于用户采纳评价 |
| Zep/Graphiti | session messages、raw facts、user-level knowledge graph、group graph | `memory.add/get` 按 session/context 返回 context、近期消息、事实，另有 graph search | 资料描述以构图/检索为主，未在本入口确认统一人工编辑/删除流程 | session/group graph 是隔离边界；权限细节需另查官方 API | 近期消息与召回 context 可作为反馈输入，但没有仓内 effect/采纳等显式画像 |
| Letta | 常驻可修改 memory blocks + 按需 archival memory + conversation search | agent 工具查询 archival memory，支持语义搜索/标签/list | 官方资料明确 archival memory 可 list/update/delete；blocks 由 agent 维护 | block 常驻上下文与 archival 按需访问分层，agent/API 权限约束明显 | 对话搜索与 archival 查询结果可反馈 agent；没有仓内 git 墓碑模型 |
| LangMem | semantic facts、episodic experiences、procedural behavior；profile/collection | profile 固定 schema，collection 可增长检索；结合相似度/重要性/新近度/频率 | 由应用 store 与 LLM 更新器实现，官方概念指南未规定统一删除 UI | 权限/来源由应用 store 定义，分类本身不提供治理 | 检索强度与使用频率可作为排序反馈；需应用补充审计与采纳记录 |

矩阵只归纳各官方入口明确支持或明确未规定的能力；“资料未规定”不被推断为“不支持”。

## 设计建议与对照决策

### 3.1 文件优先与可恢复真源
- 出处: file:docs/design/memory_system.md:6
- 证据等级:V1
- 证据深度:仓内设计与 SOP 正文核验；Markdown 是真源，SQLite 是可重建派生物，退役应归档并保留墓碑。
- 差异:与 Mem0/Zep/Letta 的服务/数据库中心模式不同，仓内更重透明、git 恢复和人工审阅。
- 决策:保留并强化；前端必须展示 source/refs/生命周期和归档结果，不能用“删除成功”掩盖物理删除或索引悬空。

### 3.2 candidate → active 与 manager 写入边界
- 出处: file:docs/design/memory_system.md:63
- 证据等级:V1
- 证据深度:仓内设计和 manager Tool 实现核验；主 agent 投递 note，manager 负责 add/update/merge/stale，候选不会自动入库。
- 差异:外部方案多把自动抽取当默认体验，仓内把用户/manager 的写入判据和 provenance 放在核心位置，可信度高但 UI 必须把“候选、待确认、已生效”做成明确工作流。
- 决策:保留写读分离；前端从长列表改为“待处理收件箱 + 已生效知识 + 退役档案”三个入口，不把 candidate 和 active 混在普通条目筛选中。

### 3.3 召回→采纳→效果画像
- 出处: file:docs/design/memory_decision_sufficiency.md:65
- 证据等级:V1
- 证据深度:仓内设计、Tauri 投影和前端渲染点核验；当前已有 recalled/fetched、零采纳候选、effect 聚合和 context bill。
- 差异:这是仓内相对外部产品更强的可解释性基础，但目前 UI 把效果指标放在诊断折叠区，用户管理条目时看不到“为什么被召回、是否被使用、是否仍有价值”的完整路径。
- 决策:把效果摘要提升为条目详情的证据页签；诊断原始账单保持折叠，不让指标卡片抢占普通管理任务的主视野。

### 3.4 既有 research 工作台的可复用交互原则
- 出处: file:docs/design/research_workspace.md:25
- 证据等级:V1
- 证据深度:仓内已验证设计正文核验；其原则是结果优先、来源卡片、双向引用、结构化数据不降级为字符串、空态给指引。
- 差异:Memory 页目前仍把来源、状态、统计和正文压在一张条目卡/详情里，没有 research 工作台那样的结果→证据→操作层级。
- 决策:复用这些既有能力而不复制 research 页面：详情采用“摘要/正文、证据与来源、召回效果、生命周期动作”页签；URL/file:line/source/refs 必须可点，空态必须解释下一步。

## 4. 建议的下一版信息架构

### 4.1 顶层入口

- **收件箱**：candidate、失败整理、待确认合并/晋升；只显示需要决策的东西。
- **知识库**：active 的 fact/sop/habit/preference，支持检索、范围、状态和标签；默认不显示历史 global。
- **档案**：deprecated/invalid、episode 和原始召回历史，默认只读。
- **评估**：召回/采纳/效果/上下文账单，作为诊断和反馈，不与知识编辑混排。

### 4.2 条目详情

详情首屏只回答：这是什么、当前是否有效、来源是什么、最近是否被使用。其他信息按页签展开：

- 内容：标题、召回钩子、Markdown 正文；
- 证据：source、refs、创建/更新事件、跳转原文；
- 效果：召回次数、正文采纳、最近命中、效果画像；
- 生命周期：编辑、合并、标记失效/归档、恢复；永久清理必须另行确认。

### 4.3 交互与数据不变量

- 所有异步资源带 generation，只允许最新请求提交；
- 项目/范围/搜索变化统一清除旧选中态和详情；
- 各数据域独立展示 loading/error/empty，重试不刷新整页；
- 任何“删除”默认是可恢复归档，不允许直接 `remove_file`；
- 前端 IPC 统一 snake_case；
- 真实结构化字段不拼成不可操作长字符串；
- 所有筛选和详情状态可由 URL/显式状态恢复，重启后不依赖内存变量；
- 既有 `memory_search` 的 lexical/hybrid 选择继续由后端决定，UI 只展示解释和结果，不在前端复制排序规则。

## 5. 待用户拍板的设计边界

1. **global**：是否完全从现行 UI 移除，只保留历史归档查看；本轮建议是移除现行入口。
2. **自动捕获范围**：只接受轮末 manager 候选，还是允许用户在条目详情中直接创建；本轮不新增第三条自动写入路径。
3. **编辑与退役**：是否允许用户直改所有字段；本轮建议允许改内容但默认走归档/墓碑，不提供无提示物理删除。
4. **profile 视图**：是否把 preference/项目当前状态从普通条目集合中提升为固定 schema 的“当前状态卡”；本轮建议采用，但 schema 和迁移顺序待确认。
5. **图谱/向量**：本轮建议不把图谱或外部托管向量服务作为真源；若需要，只作为可重建检索适配器，须另开设计决策。

## 6. 本轮范围结论

- 已覆盖当前 Memory 页入口、核心渲染/交互、Tauri memory command、现有冒烟断言和相关仓内设计。
- 已对照 4 个有官方正文资料的外部方案：Mem0、Zep/Graphiti、Letta、LangMem。
- 本轮只完成审计与设计研究，不修复代码；删除归档、异步代际、项目切换清理、错误隔离、字段命名和 global UI 语义应在用户确认后拆成实现条目。
