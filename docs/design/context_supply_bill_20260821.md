# 上下文供给账单（R-312 B1）

- 状态：B1 测量完成；B2 设计待做；B3 评审与实施条目待做
- 日期：2026-08-21
- 关联需求：R-312
- 数据范围：本项目 `.kanzei/state.db` 的真实 `episodes.context_json`，按每个真实 session 的最新 episode 取样；不是测试夹具、不是静态 prompt 副本。

## 1. 口径与证据锚

现有生产链已经把每轮 system 注入拆成可读账单：

- `crates/kanzei-core/src/runner/drive/assembly.rs:87-106`：合并 stable/refreshable system，并记录 `agent/system` 与 `tools/schema` 字符数。
- `crates/kanzei-core/src/runner/drive/assembly.rs:108-123`：记录 `memory/hints` 与 `scout/brief`，且二者不进入持久化 messages。
- `crates/kanzei-core/src/runner/event.rs:160-166`：`RunSummary.context_report` 是每轮账单载体。
- `crates/kanzei-core/src/store/episodes.rs:12-35,83-106`：将 `context_report` 序列化到 `episodes.context_json`，并可按 session 回放。
- `crates/kanzei-app/src/memory.rs:395-411`：`memory_context_bill` 读取最近 episode 与账单；`crates/kanzei-app/ui/13-memory.js:9-15` 是真实 UI 消费方。

读取命令（只读）：

```text
python -c 'import sqlite3; ... select e.session_id,e.episode_id,e.created_at,e.input_tokens,e.context_json from episodes e join (select session_id,max(created_at) m from episodes group by session_id) ...'
```

数据库实测：1032 个 episode、11 个 distinct session、时间范围 `1786129158964..1787298922279`。以下选择 7 个 session 的最新 episode；另外 4 个 session 的最新 `context_json` 为空，列入排除说明而不补造数据。

`context_json` 的数字是字符数。为便于收益估算，本账单同时给出 `字符数 / 4` 的粗 token 估计；它不能替代 provider 的真实 `input_tokens`，因为后者还包括历史 messages、协议封装和工具结果。本次保留 episode 中的真实 `input_tokens` 作为对照列。

## 2. 七个真实 session 的最新账单

| session | episode | 真实 input_tokens | context 字符总数 | 粗 token（字符/4） | 主要块与占比 |
|---|---:|---:|---:|---:|---|
| `ses_project_c0b8d633186c2464` | 1032 | 630,908 | 91,156 | 22,789 | tools/schema 43,802（48.1%）；agent/system 20,745（22.8%）；dev/conventions 13,604（14.9%）；dev/design-index 7,900（8.7%）；dev/memory 3,544（3.9%） |
| `ses_project_c0b8d633186c2464#p11` | 442 | 660,335 | 64,092 | 16,023 | tools/schema 32,864（51.3%）；agent/system 14,430（22.5%）；dev/conventions 11,947（18.6%）；dev/memory 3,601（5.6%） |
| `ses_project_c0b8d633186c2464#p10` | 412 | 29,115 | 69,522 | 17,381 | tools/schema 32,640（46.9%）；agent/system 18,023（25.9%）；dev/conventions 11,376（16.4%）；dev/goals 2,324（3.3%）；dev/memory 3,601（5.2%） |
| `ses_project_c0b8d633186c2464#p8` | 401 | 69,697 | 61,283 | 15,321 | tools/schema 32,351（52.8%）；dev/conventions 11,376（18.6%）；agent/system 9,711（15.8%）；dev/memory 3,601（5.9%） |
| `ses_project_c0b8d633186c2464#p5` | 390 | 25,270 | 65,912 | 16,478 | tools/schema 31,346（47.6%）；agent/system 15,763（23.9%）；dev/conventions 11,096（16.8%）；dev/memory 3,601（5.5%） |
| `ses_project_c0b8d633186c2464#p2` | 218 | 45,460 | 49,247 | 12,312 | tools/schema 22,240（45.2%）；dev/conventions 9,488（19.3%）；agent/system 6,568（13.3%）；dev/memory 3,941（8.0%）；dev/project-docs 3,306（6.7%） |
| `ses_project_ce2fce953a5e4103` | 41 | 1,230 | 14,785 | 3,696 | 无 tools/schema；agent/system 4,392（29.7%）；dev/memory 3,102（21.0%）；dev/conventions 3,081（20.8%）；dev/project-docs 1,592（10.8%） |

### 2.1 聚合读数

在包含 `tools/schema` 的 6 个现代 dev session 中：

- 平均 system 注入账单：66,868.7 字符，约 16,717 粗 token。
- `tools/schema` 平均占 48.6%；范围 45.2%–52.8%。这是当前最大且稳定的单块。
- `agent/system` 平均占 20.7%；范围 13.3%–25.9%。这一块混合了 agent 固定提示、引擎规则和本轮动态内容，不能直接等同于状态机字段。
- `dev/conventions` 平均占 17.4%；范围 14.9%–19.3%。
- `dev/memory` 平均占 5.7%；当前数据没有显示它是最大负载。
- `memory/hints` 出现时仅约 0.3%（现代 6 session 平均），但这是按命中情况变化的动态块，不能从单次平均推断所有任务。

## 3. 状态机自由文本字段的测量边界

本轮没有把 `进展`、`对账`、`停车` 从历史 `agent/system` 或 `resolved-control-state` 中反推成数字，原因是当前持久化的 `context_report` 只记录块级来源，不记录 JSON 字段级来源与写入事件。现有数据能证明：

1. `agent/system` 和 `dev/project-docs` 被计入了账单；
2. `resolved-control-state` 是 `agent/system` 的组成内容（装配点：`crates/kanzei-app/src/run/assembly.rs:193-203`，渲染点：`crates/kanzei-tools/src/work.rs:1175-1188`）；
3. 但当前 `episodes.context_json` 无法证明其中多少字符来自 `进展`/`对账`/`停车`，也无法按真实 session 统计这些字段的写入次数。

因此本节是明确的测量缺口，不把静态 tracker 文件行数冒充会话频次。B2 必须评估字段级机械计数方案：在不改变 R-104 记忆口径的前提下，分别记录字段写入事件、注入字符/token 和被压缩/重取次数。

## 4. B1 结论（供 B2 设计使用）

- 账单基础设施已经存在并有真实消费者，本次不是重复申报；本次交付是对真实 state.db 的跨 session 读取、聚合和落档。
- 当前现代 dev 注入的主要固定成本是工具 schema（约 48.6%），其次是 agent/system（约 20.7%）与 conventions（约 17.4%）。因此“只压缩自由文本字段”不能假设收益最大，必须与工具 schema 的可重取/分层策略一起评估。
- `dev/memory` 约 5.7%，现有数据不足以支持改变 R-104 记忆注入口径；按 R-312 边界保留既有口径。
- 账单块的字符数和 provider `input_tokens` 差异很大，后续收益估算必须分开写：块级账单用字符/4 粗估，真实成本用 episode 的 provider usage，不能把两列相加。
- 7 个 session 中有 6 个现代 dev session，已覆盖要求的至少 5 个真实 session；旧 session 的空账单保留为数据质量信号，不填零冒充“无注入”。

## 5. 待 B2 回答的问题

1. 机器字段：测试记录号、提交号、批次、观察锚点等哪些字段可由引擎代填，哪些仍需模型提供判断；机械代填失败如何显式暴露。
2. 注入分层：WIP 条目全文与依赖闭包保留，其他条目降为索引行后，如何保持调度权威与恢复证据；工具 schema 是否改为按需/分层可重取。
3. 沉档：进展/对账历史按批次折叠时，当前批次视图、完整历史和可回放锚点如何同时保留；不得制造不可删除的游离字段历史。
4. 压缩协同：`context_compaction.md:45-53,77,101-104` 已规定 L0 机械清理、L1 纪要和机械事实清单；R-312 需要补上注入块的排除/重取规则，以及 file:line 锚点腐烂后的恢复策略。

## 6. B2 四方向候选设计（草案，待用户拍板）

以下是方案对照，不是已实施行为。收益数字是场景估算，不把估算当成实测，也不把四项简单相加。

### 6.1 方向一：机器可代填字段，模型只写判断

**沿用的既有能力**：`test_record` 已在 `crates/kanzei-tools/src/test_record.rs:121-148` 扫描 active/archive 并分配未占用的 `T-<epoch>`；close telemetry 已能从真实 close 过程记录测试、提交、批次和缺环；`work.rs:140-178` 已有 `decision_locked`、`resume_reconcile` 和仓库观察锚点的结构化字段。

**候选方案**：新增一个“机械事实信封”概念，但不让模型填写它：

- 引擎代填 `test_record_id`、当前 commit/full hash、`recorded_at`、`observed_head`、`observed_worktree_hash`、工具实际返回的测试命令与状态。
- 模型只提交判断性字段：为什么选择方案、验收逐项结论、失败根因、用户拍板内容、停车/阻塞的语义理由。
- 批次总数仍由模型决定（它是计划判断），完成进度由 commit marker、测试记录和 tracker 状态交叉计算；不以“提交数=批次数”替代现有批次语义。
- 旧条目没有机械信封时只显示“无历史机械信封”，不回填伪证据；close 仍以现有证据门禁为准。

**取舍**：优点是去掉模型抄写时间戳、hash、T 编号造成的机械负担，并避免同一事实在进展/对账/提交信息中多份漂移；代价是 tracker、test_record、git finalize 和 close 必须同批接线，且需要兼容旧条目和写失败回滚。不能把模型输入 schema 简单删掉后再由 UI 猜字段，否则会形成新的双真源。

**收益估算**：若一次收尾需要手写 300–800 字机械元数据，按字符/4 约 75–200 token；机械信封只保留一份并在注入时按需展开，估算每次收尾减少 50–150 token 的模型生成/复述负担。该范围是低置信场景估算，当前 `context_report` 没有字段级写入计数，不能宣称已实测。

### 6.2 方向二：注入分层，当前 WIP 全文、其余索引

**现状证据**：B1 的 6 个现代 dev session 中，`tools/schema` 平均 48.6%、`agent/system` 20.7%、`dev/conventions` 17.4%、`dev/memory` 5.7%。`resolved-control-state` 的结构包含 selected 全条目、WIP/blocked/parked 摘要和依赖裁决（`work.rs:140-178`），当前每轮由 `assembly.rs:193-203` 注入。

**候选方案**：分四层，但不改变权威来源：

1. **稳定层**：身份、硬不变式、工具可用性和少量当前档位规则；保持每轮注入。
2. **工作层**：当前 selected WIP 条目全文、仍未完成依赖闭包、当前批次的验收/判断字段；保持全文。
3. **索引层**：其他活动需求/缺陷、停车条目、已完成依赖，只给 `ID·状态·标签·一句话标题·解除条件摘要`，需要正文时由 `req get`/`defect get` 重取。
4. **按需层**：历史归档、完整进展/对账、工具输出和设计正文；只在真实需要时读取，不随默认 system 常驻。

**边界与取舍**：这项只改变 tracker/context 的注入形态，不改变 R-104 记忆检索口径；也不默认削减 D-201 要求的 conventions 全量注入。B1 的 17.4% conventions 占比足以证明它值得优化评审，但 R-312 的边界要求先经用户拍板。`tools/schema` 是 48.6% 的最大块，却是模型调用工具所需契约，不能把“工具不可见”冒充分层；若要按需暴露 schema，必须另立实施条目并补 provider/tool-call 回归。

**收益估算**：当前现代 dev 注入账单平均约 66,868.7 字符，但 `resolved-control-state` 与 tracker 各子块尚未单独计量。以非当前条目每条从 700–1,500 字符全文降为 80–160 字符索引行的场景估算，每减少 5 条约节省 3,100–7,100 字符，即约 775–1,775 粗 token/轮；实际收益随 WIP/依赖数量变化，置信度中低。工具 schema 48.6% 不计入这项收益，避免重复计算。

### 6.3 方向三：进展/对账历史按批次折叠，默认只看当前批次

**沿用的既有能力**：docstore 已将终态条目移入 archive（`docstore/archive.rs:159-227`），并在归档修正时合并既有“进展”而不是静默丢弃（`archive.rs:283-322`）。但当前 tracker update 的自由文本仍可能把多批历史放在同一字段，`req get` 默认也没有“当前批次视图/完整历史视图”分离。

**候选方案**：把“当前工作视图”和“完整审计历史”分开：

- active 条目只保留当前批次判断、当前验收对账摘要和 `history_ref`；完整批次段落进入由引擎管理的 append-only history/event 存储，或进入已有归档写通道可回放的历史面。
- `req get` 默认返回当前批次视图与历史计数/引用；显式 history 查询才展开全部批次。close、verify、审计和归档使用完整历史，不依赖模型默认视图。
- 每个批次绑定 `batch_id`、commit hash、测试记录 ID 和稳定的 symbol/heading 锚；file:line 只作为当时定位提示，不作为唯一恢复键。
- 禁止通过多行自由文本 update 追加历史；写者必须走机器字段/事件写通道，避免既有游离段落不可删除问题。

**取舍**：优点是默认上下文只携带“现在做什么”，又不牺牲 close 的完整证据；代价是新增历史查询与迁移边界，必须处理旧文档的自由文本解析、CAS/锁和 archive 双真源。不能用物理删除旧进展换 token，旧证据仍需可审计。

**收益估算**：若一个活动条目已有 3 个旧批次、每批 500–1,000 字符，默认视图从 2,000–3,500 字符降为当前批次 500–1,000 字符，场景节省约 1,500–3,000 字符，即 375–750 粗 token/条目注入。当前账单没有把 tracker 条目字段拆成单独块，因此这是中低置信估算，不计入 B1 实测平均值。

### 6.4 方向四：压缩与注入协同，机械可重取内容不进纪要预算

**现状证据**：`docs/design/context_compaction.md:45-53` 已定义 L0 prune、L1 纪要和统一触发线；`:60-104` 已定义半结构化纪要、机械事实清单双通道、质量闸和原文事件指针。实现侧 `crates/kanzei-app/src/run/persistence.rs:324-419` 已先做 L0 机械清理，再调用 core 压缩；`crates/kanzei-core/src/runner/context.rs:42-80` 对纪要输入做工具名/结果截断。

**候选方案**：给 system 注入块增加“可重取/不可重取”分类，并让压缩器只承担不可重取的语义：

- 可重取：工具 schema、当前 resolved-control-state、tracker 索引/当前 WIP、memory hints、设计索引。它们由 commit/session/条目 ID 绑定，压缩纪要只保留引用与刷新条件，不复制正文。
- 不可重取或必须保留：用户原始指令、用户拍板、被否决方案及理由、失败原文、当前未提交判断；进入半结构化纪要和机械事实清单。
- 压缩后先机械重取可重取块，再把当前 WIP 与依赖闭包重新注入；若重取失败，明确显示缺失而不是让纪要模型补写。
- file:line 锚点改为 `(path, commit, symbol/heading, optional line)`；当前 HEAD 与锚定 commit 不同就标 stale，并调用 `symbols`/`grep` 重定位，不能沿用腐烂行号。

**取舍**：优点是避免“同一可机械重取内容既占压缩输入又占下一轮 system”，也避免模型把过期的 resolved state 当历史事实；代价是压缩后装配顺序和刷新失败路径必须可观察，且需要给每个注入源增加稳定身份与版本。D-573 的压缩事务缺陷仍独立处理，本方案不吸收其实现范围。

**收益估算**：现有 `DIGEST_SOURCE_CHARS` 上限为 24,000 字符；如果其中有 8,000–16,000 字符属于可重取注入块，排除后约少送 2,000–4,000 粗 token 的纪要输入，同时减少纪要重复携带的 2,000–4,000 粗 token。这个估算不承诺减少 provider 最终 input_tokens，因为刷新后的 system 仍会进入下一次请求；它只估算压缩过程与纪要重复的负担。

## 7. 初步取舍与待用户决策

基于 B1 数据，初步建议不是立即砍掉 conventions 或工具 schema，而是按风险排序：

1. **优先设计并实施方向一**：机械事实与判断字段分离，收益明确、可验证、能减少后续重复抄写。
2. **随后实施方向三**：当前批次视图与完整历史分离，直接针对状态机自由文本膨胀；前提是先设计旧文档迁移和审计回放。
3. **方向四与方向二协同**：先给注入源增加身份/可重取标记，再决定是否把非 WIP 条目降为索引；不要在没有字段级账单前直接删除全量块。
4. **暂不改变 D-201 conventions 全量注入和 R-104 memory 注入口径**：B1 只证明 conventions 平均 17.4%、memory 平均 5.7%，尚不足以替用户作破坏兼容性的决策。

需要用户拍板的选项：

- 是否接受“当前 WIP+依赖全文、其他条目索引化”的默认注入层级；
- 是否接受“active 当前批次视图 + 可回放完整历史”的 tracker 形态；
- 是否接受方向一作为第一批实施条目、方向三/四作为后续实施条目；
- 是否允许在后续实施条目中对 conventions 做按需化实验（默认仍保持 D-201 全量）。

本节仍是草案。用户未评审前不登记实施条目、不写 accepted decision，也不关闭 R-312。

