# C2：typed 会话事实与投影依赖图

首次只读定位基线 `336e68ef`（C1 已整合为主树 dev `76aa80f7`）。正式实施分支 `kanzei/audit-c2` 从 `a962248c` 建立，两所属源文件与只读定位基线相同。已全文读取两文件及测试/失败分支，root 授权实施投影下标、checked append、recovery、seed 事务/来源修复及 events 的身份 helper 切片；最终验证基线为 `5b2686e031e19dd59774e750e5643cd73dd1a368`（A1/B1/C1/M1）。本文件是依赖/状态地图，不增加公共 coverage，不代替逐文件结论。

## 范围

- `crates/kanzei-core/src/store/typed.rs`：2531 行；事实格式、状态机、checked 追加、legacy seed/恢复、TypedSessionWriter。
- `crates/kanzei-core/src/store/typed/projection.rs`：444 行；事实重放 surface/transcript、InterruptedAssistant、shadow comparison。
- `crates/kanzei-core/src/store/events.rs`：仅原 append_event_tx 的 event_id 生成式/helper 切片，已由 root 授权；C0 全文覆盖不重复计数。
- 两者是同一个复杂状态域，保持同包所有权。后续 C3 为 work/episodes → decisions/task；本包只读这些邻接依赖的入口，不计它们全文覆盖。

## 依赖和所有权

```text
kanzei-llm::{Message,Part,Role} + serde/sha2
                 ↓
typed.rs SessionFact / Envelope / SessionInvariant / stable hash
                 ↓                         ↑
typed/projection.rs 纯重放与 shadow comparison ← events::session_event_id
                 ↓                         │
events::append_event_tx + SessionStore SQLite/session_events
                 ↓                         │
typed.rs checked append / seed / recovery / TypedSessionWriter
                 ↓
app typed_events 薄适配 → run assembly/events/persistence
CLI run/events/finalize、schedules executor、conversation/rewind/mobile
```

文件 import 有域内环：projection 使用 typed 定义的类型/hash，typed 读取/影子核对使用 projection。事实定义及写入状态归 typed；projection 是可重新计算的视图，没有独立持久状态。

| 状态 | 唯一 owner | 使用/边界 |
| --- | --- | --- |
| 持久事实、会话 sequence、event_id | SessionStore/events 的 SQLite session_events | 事实与 compaction/legacy 快照共用事件日志；不能用投影视图覆盖事实 |
| turn、step、draft chunk/finalized、declared tool/call resolved、terminal | SessionInvariant（验证状态） | apply 在克隆上验证；checked batch 的 invariant 只有 DB commit 成功后才前移 |
| writer 草稿 buffer/attempt、source/logical step、open_calls、errors/terminal | TypedSessionWriter 的运行时状态 | 桌面/CLI/scheduler 使用 Arc<Mutex<writer>>；跨进程/独立 writer 不共享此 Mutex |
| legacy seed provenance | typed seed 指向 conversation.updated 的 source_event_id/sequence/hash | D-375 新 seed 不复制 messages；读时回填源事件；同源 source_sequence 才覆盖本会话事实，fork/general 外源不截断目标事实；幂等必须同时查 source ID/sequence |
| 模型 surface / 完整 transcript / 中断诊断 | projection 的纯计算结果 | compaction 只替换模型 surface，再追加其 sequence 之后的事实；transcript/诊断来自全部原始事实 |
| subagent transcript | session_events 的 subagent.transcript | 按 call_id 取最新可解析快照，非 SessionFact 状态机成员 |

## Public API → caller

| API 组 | 已定位生产 caller |
| --- | --- |
| SessionFact/SessionInvariant + append_session_facts_checked | TypedSessionWriter；app/mobile.rs、general_chat.rs、conversation_actions.rs；core rewind.rs |
| TypedSessionWriter | app/typed_events.rs 别名薄适配；run/{assembly,coordinator,persistence}.rs、run/events/mod.rs；CLI run.rs 和 run/{events,finalize}.rs；tools/schedules/executor.rs |
| list_session_facts/list_latest_segment_facts | app/conversation.rs、mobile.rs、processes/workspace.rs；CLI run.rs/run/finalize.rs；core rewind.rs；writer shadow report |
| prepare_typed_session、seed、recover_interrupted_session_facts | CLI run.rs；core rewind.rs；app/typed_events.rs 将它 re-export 为 prepare_session，桌面装配通过该别名准备 |
| project_session_facts/project_session_facts_with_surface | app/conversation.rs、processes/workspace.rs；CLI run.rs/run/finalize.rs；typed seed恢复/shadow；核心 item_context 测试 |
| commit_work_context_surface | app/run/events/mod.rs；CLI run/events.rs；core runner/item_context 测试 |
| recover_subagent_transcript | app/run/coordinator.rs 的恢复 provider；typed 本域测试和 background_subagent_dispatch 集成测试 |
| shadow comparison | TypedSessionWriter::write_shadow_report；app/conversation.rs 的 compare_shadow；投影状态仅作为诊断输出，不代替持久事实 |
| summarize_shadow_reports | CLI cli/shadow.rs；writer 只追加 shadow_compared，不使用统计结果改变事实 |
| stable_json_hash/stable_message_hash、decode_session_fact | typed/projection 本域；core rewind；app general_chat/fork；desktop bridge receipt；app conversation 历史序号类型识别 |

搜索结果区分生产与测试；没有生产 caller 的公开入口也要核对其文档合同，但不能把仅测试调用的场景当成实际运行路径。

## 按底层 owner 的审查顺序

1. typed 的格式/解码/hash、SessionFact/Envelope 和 SessionInvariant 转移。
2. projection 的确定性重放、tool result 分组、中断 materialization、seed/surface sequence 与 shadow 分类。
3. typed 的 DB checked batch、终态跨 writer 检查、seed 引用/幂等与 reset/deletion、恢复。
4. TypedSessionWriter 的 flush/restart/terminal、source/logical step 与失败时内存状态、锁 lifetime；补 caller 行为核对。
5. 核对 work-context surface 与 A1 已定稿的 source comparison/liveness 契约；跨范围变更交 root 协调。root 已授权 events 身份 helper，保持原 ID 字节；未修改 session/mod。

## 契约门槛与验证安排

- 保持 C0 输入/事件原子准入合同；checked facts 不另起 session 状态拥有者。
- A1 改变 manual compaction 的 source 比较和 artifact 生命周期窗口，本包需在统一基线上核对 typed-only append、reset、compaction surface 的真实版本关系后再修改。
- 并发/持久化 bug 需用两个连接或确定性 hook/channel 证明真实状态变化和失败路径；不靠 sleep，缺测试本身不记为 bug。
- root 授予独占 Cargo 串行槽后完成定向 typed 34 项、core 全包 401 项及 all-targets check/Clippy/fmt/diff；最终均通过。负例、源码恢复 hash、结果见 C2-verification.json，未把 core 结果当作全工作区/UI 验证。

## 全文审查确认的实际路径

| owner | 问题与实际 caller | 最小闭环 / 状态 |
| --- | --- | --- |
| projection | 旧 turn 的非 superseded 中断草稿仅进入 transcript；新 turn 连续两条 tool result 使用 surface 下标访问 transcript，第二结果写错消息。真实 writer finish/recovery 均可产生该链，conversation_shadow_get 返回错误 transcript | 私有分组使用两个独立下标；真实 TypedSessionWriter 合法链回归通过，旧实现明确失败 |
| checked append | CLI/desktop 共用 session/DB，writer 与 recovery 使用独立连接；事务外 terminal 预检后等待另一 writer，后者提交 terminal 后旧 writer 仍追加。caller 局部 invariant 还可能漏掉已提交的非 terminal draft/call 状态 | Immediate 内重建涉及 turn 的 DB 状态、验证/追加；commit 后推进 caller，未触及 turn 保留；真实 writer 等锁屏障/stale 状态/SQL 失败回滚均通过 |
| recovery | 事务外读取 open draft 后，另一 writer 提交同 message；恢复随后仍写重复 interrupt/terminal，重建 invariant 失败 | 读取、决定、写入共用 Immediate 和 append_session_facts_tx；真实等锁回归通过，旧实现失败 |
| seed | 未播种的旧 legacy source 在事务外读 source/floor 后，CLI --new 另连接提交 reset；旧 seed 追加到新段，当前 prior 恢复旧历史 | source/floor/duplicate 同一 Immediate；真实 reset writer 等锁回归通过，旧实现失败 |
| seed 来源边界 | 旧 raw legacy@1→手机 typed user@2→真实 prepare 的 seed@3，按 seed 发布位置截断永久隐藏手机输入；fork 外源高序号若直接作截点会隐藏目标事实，外源低序号又可撞中本会话 duplicate 检查 | 复用 events 的 canonical ID helper，仅真实同源按 source_sequence 截断；外源 cutoff0；幂等匹配 ID+sequence。4 条实际 store 回归通过，旧投影/旧 duplicate 分别明确失败 |
| work-context surface | item_context 从 runner 局部 messages 生成候选、期间手机独立线程提交 user fact；unchecked surface 以更晚 sequence 隐藏该输入。当前 report/event 不带候选原 source，提交时取最新序号不能证明其归属 | root 已分派 A2，需等本包 typed.rs 提交后接续 producer/event/两入口的 source 合同联动；本包登记开放项，不单方修改 public contract |

范围外 mobile consumer 还存在已确认路径：历史 post-terminal 脏条使其 apply 全历史失败，或 typed 追加失败；raw mobile.message 已写且响应仍 202，但该 raw 类型没有恢复消费方，UI/内存可见输入在重启后消失。root 已收到精确 caller 与持久状态，将另包修复；本包不编辑 app/mobile.rs。

另一个上层已报路径：CLI 并发 prepare/recovery 可闭合桌面活跃 turn，随后 TypedSessionWriter 的 assistant/tool-called 批次被拒绝；它只记录 errors、void 返回，app/CLI sink 及 runner/drive/history 无提交 ack，仍 Proceed 执行工具。设计要求 tool_called 先落盘再副作用；该问题需 producer/event/执行边界联动，不能把本包 DB 守卫当成执行层已安全。

修复项已经在统一基线上验证；A2/M2/执行 ack 的开放项没有宣称闭环。D-417 的 43 条报告是原代码记录的历史依据，本包没有读取真实用户数据库。

## 新来源修复的 caller/identity 核对

- app/conversation_actions.rs:119：fork inline seed 使用 `fork:{原session}:{原序号}`，source_sequence 属于原会话；目标新会话可在 seed 前后接收独立输入。
- app/general_chat.rs:158：general inline seed 使用 `general:{session}`、source_sequence=0；按外源处理保持初始化消息和后续输入。
- typed.rs 的 seed_latest_legacy_snapshot：source_event_id 引用本会话 conversation.updated 的真实 StoredEvent；list_session_facts 回填 messages 后仍保留来源 ID/sequence。
- 所有生产 session_events INSERT 仅 events::append_event_tx；初始历史 e267fa06 store.rs 就使用 `evt_{session}_{sequence}`，拆域历史 2fd36cf2 延续；schema/C0 v7 恢复只保留原 ID，没有另类本会话 ID 生成器。
- root 授权将原 ID 表达式提为 store 内 session_event_id，append 与 projector 共享，避免重复 identity 规则；同源与外源保持既有序列化字段。
