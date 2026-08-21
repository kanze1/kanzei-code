---
kind: prior_art
topic: r323-prior-art
status: complete
trigger: core_requirement
entry_refs: R-323
websearch_round_limit: 4
---

# 先行方案对照

对照主题:工具编排的决定权归属——执行顺序与并行分组由引擎静态推断,还是由模型声明。

## 外部已有实现

### Claude Code 的批式并行工具调用(PostToolBatch)

- 出处: https://code.claude.com/docs/en/hooks
- 证据等级: V2
- 事实: 钩位表中有 `PostToolBatch`,原文 "After a full batch of parallel tool calls
  resolves"。即模型在**同一条 assistant 消息里**发出的多个工具调用构成一个批,批内并行、
  批间串行;分批的依据是模型的输出结构本身,而不是引擎对调用间依赖关系的静态推断。
- 差异: 本仓 `build_tool_execution_waves_with` 拿到模型的一整串调用后,**由引擎**按
  `ToolConcurrency` 冲突关系重新切波,模型无法表达「这三个可以一起跑、那个必须等」。
  引擎只看得到锁键,看不到语义依赖。
- 决策: **采用其「批边界由模型输出决定」的方向**,但不照搬——本仓的 `ToolConcurrency`
  冲突检测必须保留为**安全校验**(模型声明可并行、但两者写同一棵树时仍须拆开),
  即模型提议、引擎否决,而不是引擎独裁。这与 tool_pipeline 的 Guard 单调性同构:
  声明只能放宽到安全边界为止,不能越过。

### LangGraph 的静态图编排

- 出处: https://docs.langchain.com/oss/python/langgraph/graph-api
- 证据等级: V1
- 事实: 工作流建模成 State/Nodes/Edges 三件套,edges 决定下一步执行哪个 node。
  图由**开发者预先声明**,模型只在节点内部工作;控制流是框架资产,不是模型输出。
- 差异: 这是「引擎持有编排权」的极端形态,与本仓当前 wave 调度同侧但更彻底。
- 决策: **不采用**。用户定位为「模型自治程度取决于模式」,静态图把自治压到最低,
  与结伴档的目标相反;且本仓已有的阶段流水线(phase.rs)已经覆盖了「需要确定性图」
  的那部分场景(勘察-屏障-实现-复核),不必把它推广到每一次工具调用。

## 仓内既有设计

### ToolConcurrency 契约与确定性切波

- 出处: file:crates/kanzei-harness/src/tool.rs:1
- 证据等级: V3
- 事实: `Shared(key) / WorktreeWrite(key) / Exclusive`,默认 `Exclusive`——未显式审计的
  工具绝不自动并行。切波保序、首次冲突即封波,同样的调用序列永远切出同样的波。
- 差异: 保守默认与确定性都是资产,但「引擎独占分组决定权」不是必然推论:
  可以保留冲突检测作为安全网,同时接受模型的分组提议。
- 决策: **保留 `ToolConcurrency` 与确定性**,新增模型侧的分组声明通道;
  声明与冲突检测冲突时以冲突检测为准(单调收紧,不放宽)。

### tool_pipeline 的 Guard 单调性

- 出处: file:crates/kanzei-harness/src/tool_pipeline.rs:1
- 证据等级: V3
- 事实: 「Guard 只收紧不放宽——policy allow 永远不能覆盖 guard deny」。
- 差异: 该原则目前只用于权限,未用于调度。
- 决策: **采用同一原则约束调度声明**——模型的并行声明是 policy 层提议,
  `ToolConcurrency` 冲突是 guard 层否决,方向一致、语义可复用,不引入第二套心智模型。
