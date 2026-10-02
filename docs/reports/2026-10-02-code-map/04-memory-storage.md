# 记忆上下文和数据存储

## 所有数据真源

| 数据 | 位置 | 真源与派生关系 |
|---|---|---|
| 全局配置 | kanzei home 的 kanzei.toml | provider、模型、权限和运行配置 |
| 项目配置 | 主根 `.kanzei/kanzei.toml` | 项目覆盖；线内副本不是项目资产权威 |
| 桌面偏好 | `~/.kanzei/app.json` | 项目登记、选中项目、线配置和 UI 偏好 |
| 研究库 | `~/.kanzei/research-library.json` | topic 身份登记，保留原目录路径 |
| 需求/缺陷/想法/A 决策 | `.kanzei/project/*.md` 与归档 | 普通 Markdown 是业务真源 |
| 规范和架构索引 | `.kanzei/project/conventions.md`、architecture | 专用写入口，用户可显式编辑 |
| 项目记忆 | `.kanzei/memory/*.md`、archive | 一条一文件，frontmatter 和正文是真源 |
| 全局记忆 | kanzei home 的 memory 目录 | 独立 global scope；实际召回/注入入口须分别核对 |
| 记忆索引 | memory/INDEX.md、index.db | Markdown 派生索引、FTS、向量、命中与管理观测 |
| 记忆草稿 | memory inbox 与 checkpoint | 暂存待管理草稿，不能当作 active 记忆 |
| 运行事实 | 主根 `.kanzei/state.db` | 会话事件、输入、通知、过程、工作事件和研究运行 |
| Work Unit | work_events | 事件真源，work_surfaces 是可重建表面 |
| typed 会话 | session_events | segment/messages 等表面由事件投影，保留 legacy 兼容路径 |
| 自主决定和用户复核 | session_events | decision.updated、work.user_reviewed 等追加事实 |
| 研究计划/loop/AUTO | topic 的 plan、loop、workflow JSON | 有版本与恢复语义的流程 checkpoint |
| 探索和结果 | topic/explorations Markdown 与结果工件 | 路线业务真源；research_runs 记录执行事实 |
| 来源全文和搜索 | topic source_text 与 Tantivy 索引 | 抓取全文用于核验；索引可重建 |
| 测试执行 | tests Markdown、执行日志和版本指纹 | 专用写工具产生记录；结果覆盖范围另行校验 |
| 后台验证 | `.kanzei/verification/*.json`、本机快照目录 | job 合同、冻结源码、日志、worker 状态 |
| 工具大输出和图像 | 上下文产物与 tool-images | 通过引用保留原始观察，消息内仅使用小表面 |
| 语音配置 | 全局 voice 配置和 launcher 登记 | 本地 service/port/runtime，不是云端语音状态 |

kanzei home 统一由 `harness/home.rs` 解析，支持 KANZEI_HOME；代码不应各自重新拼 home。项目根发现与 HOME 防碰撞在 `project_root.rs`，同时处理路径等价写法和文件系统身份。

## 记忆为什么单独存在

记忆不是会话尾部摘要。MemoryEntry 包含 id、category、title、description、status、source、refs 和自由 frontmatter。description 既用于索引展示，也影响候选相关性和是否值得召回。

写入侧验证枚举、必填、引用和 subject 约束；ID 有 ledger，删除/合并保留记录；INDEX 与 index.db 从文件重建。Markdown 手改保留可恢复性，派生索引不应成为唯一数据来源。

global 与 project 保留不同 ID 范围；episode 是一次运行证据，存进 state.db，不变成长期记忆正文。活跃、候选、shadow、deprecated、归档表示不同生命周期。

## 检索的实际顺序

`MemoryIndex` 统一三通道：

1. Fingerprint 通过 frontmatter 和正文 `[fp:tool|kind]` 建映射，精确命中优先。
2. Lexical 用 FTS5/BM25 产生候选，再做相关性筛选和排序。OR 命中并不自动表示该经验适用。
3. 配置 embeddings 后，dense 对 query 编码并用余弦扫描向量；内容 hash 用于增量嵌入缓存。

混合用 RRF，`k=60`，按 rank 贡献相加，不按原始分数线性加权。无 embedder 或向量结果为空会退化为 lexical。命中观测只在最终融合结果登记，避免两个通道各记一次。

当前 `index.rs` 排序实现保留相关性和状态权重；命中/读取/注入是观测，不直接当作采纳率加权。附近仍有较旧注释提“采纳率决策价值”，整理时应以执行代码为准。

`embed.rs` 是 `/embeddings` 适配；实际响应维度、缓存、耗时和失败保留观测。本次未调用嵌入服务，不能推断当前配置中 dense 已经可用。

## 什么时候注入记忆

Dev profile 注入常驻 preference 和有预算的索引，正文按需 memory_search。项目规范单独全量注入；记忆偏好和索引仍有预算，不能把“常驻”理解成全部正文永不截断。

运行开始以用户 prompt 检索；自主开发从统一 work selection 取当前标题作为查询。提示放入本轮 system，用户消息保持原文。failure recall 在遇到适用工具失败后按指纹和分类召回。

明确路径差异：当前 Dev 常驻偏好扫描 `MemoryStore::project`。MemoryStore::global、图谱 global 和用户选择 Global 保存仍存在，但“存得下全局偏好”与“下一轮所有入口都用它”不是同一证据。

## 写入和整理逻辑

主 Agent 常规用 `memory_note` 投递草稿，不直接新增正式 active 条目。manager 独立迷你运行具有 memory_add/update/merge/stale/promote、inbox clear/discard 等工具。轮末 consolidation 有边界和最多一次的调用节奏，桌面与 CLI 共用。

主 Agent `correct` 例外只允许既有 title/description/body 的机械纠错，不能借此改变 status、extra 或新增/删除。用户记忆页、专用记忆对话和决策偏好保存是另几条显式入口。

Admission 区分可复用经验与日记、自明信息和未经验证推断；精确重复与 subject 冲突不重复新建。候选晋升需要实际 provenance：project scope 校验 episode 真实存在，证据先写成功再置 active。失败指纹目前第 2 次复发可产生 candidate，第 3 次且匹配恢复证据才可 active。

Lifecycle 和 reconciliation 处理候选老化、shadow、晋升、失效与墓碑。它们是非参数化外部记忆策略，没有修改模型参数的训练流程。

## 压缩和上下文整理

`runner/context.rs` 管预算估算、usage 校准、tail 和 chunk 规则。`compaction.rs` 管主动摘要、旧工具输出 pruning、digest、应急 overflow 与激进回退。保存原始运行事实与生成下次请求 surface 分开。

条目切换整理是另一条触发路径：成功 work claim 的工具调用/结果必须配对，全部同组工具结果完整返回后才检查 from/to。相同父需求、显式依赖、related_items 或范围交集判定为相关。

旧观察候选只来自允许归档的工具族，用户要求、受保护文档、代码修改和工具配对不被随意删掉。一般保留约 4096 最近 token，相关条目保留约 12288，并至少保留两份最近大观察；最多整理 16 个，250ms 时间上限，预估和实际节省都须至少 4096 token。

大结果先归档文件，再把 message 中内容换成可读引用。发 `WorkContextPrepared` 携新 surface，由持久化消费者确认 accepted 后才替换模型消息；保存失败留原上下文。边界 marker 防同一边界反复整理。

这些 token 数是源码估算口径，不是某 provider 的精确 tokenizer。入口在 `core/src/runner/item_context.rs:14` 和 `prepare:198`，调用在 `drive.rs`。

## 事件存储和恢复

SessionStore 使用 SQLite，当前 schema version 为 25。迁移前备份；遇到比二进制支持版本更高的库会拒绝打开，避免旧程序误改新库。升级是单向迁移，回退依赖备份和兼容二进制。

事件按 session sequence 保序；typed fact envelope 定义版本和输入/回合/消息不变量。projector 从事实构建当前表面；shadow 比对保留未知差异、兼容差异和写入错误。五类读路径可以分别经 projection gate 回退 legacy。

会话重置创建新 segment，不应清空历史事实。conversation prior 必须取当前 segment；切项目/切线的 session identity 和读取投影共同决定显示与续跑，不靠 UI 当前项目名推断。

## 全部 SQL 表的归属

| 数据库 | 表族 | 用途 |
|---|---|---|
| state.db | schema_meta、sessions、session_events、session_inputs | 版本、会话、追加事实、排队输入 |
| state.db | agent_notifications、delivery_cursors、mobile_devices | 通知、消费者游标、设备授权 |
| state.db | processes、retired_processes | 线路登记、持久字段和退休身份 |
| state.db | work_events、work_surfaces | 工作事件和当前投影 |
| state.db | episodes、recall_events、memory_recoveries、memory_sources | 轮次、召回、恢复和晋升来源 |
| state.db | memory_eval、memory_eval_agg | 回放结果和汇总 |
| state.db | research_runs、research_run_events、research_environment_leases | 实验事实、callback、环境占用 |
| state.db | file_checkpoints | 用户文件编辑恢复点 |
| memory index.db | memory_hits、memory_recalls、memory_vectors、novelty_events、recurrence_counts、manager_decisions | 检索、向量、复发、管理观测 |

FTS 虚表另由索引创建，不计入扫描器的普通 CREATE TABLE 27 名统计。所有表声明位置在 [09 接口索引](09-interface-index.md)。

## 回放和清理

六臂 replay 从 run.trace 提取失败场景，以不同检索策略真实调用 LLM，保留评估和汇总。历史外部工具不真执行，所以它能评价记忆引导的选择，不能代替修复代码的回归测试。

`kz artifacts` 先 stats/plan/dry run，明确 confirm 后才清理，有 backup 和 artifact 报告。quarantine 只识别符合契约的取证目录，未知目录保留。shadow 只读统计投影差异。存储清理不应和业务数据迁移混成一个动作。
