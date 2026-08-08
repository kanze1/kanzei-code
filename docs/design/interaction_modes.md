# 交互模式沉淀:自主推进 vs 结伴开发(R-036)

> **文档状态(2026-08-08 整理):历史记录。** 对应 R-036 已 done(双人格与模式选择器已落地,dev-pair 提示词即为现行 system prompt)。本文保留为设计决策记录,不再作为实施计划。

## 问题定性(2026-08-06)

现在 dev profile 只有一个 agent 人格,它同时承担两种矛盾的职责:

- **backlog 自主推进**(连跑场景):不该问,直接干,req/goal 纪律优先
- **对话式协作**(用户在场场景):该先回应用户、提案、澄清,而不是抓起需求就跑

矛盾的表现就是用户观察到的"提案模式有点问题":有时它先提方案等确认(对话人格),
连跑的 CONTINUE 又把确认流程冲掉(自主人格),两种行为互相打架;
用户随口问一句话,它却先 `req update` 开工单。

## 设计:一个 profile,两个 agent 人格

harness 的 agents 注册表天然支持多 agent,不需要新机制:

### dev-auto(自主推进)= 现在的 dev agent

- 任务来源:活跃目标 + 需求 backlog;没有明确任务不许问,挑目标推进
- 纪律:req doing→done、defect 先记后修、测试过了就提交、WIP≤2
- 不提案:方案自己定,做完汇报;连跑可用
- 触发:连跑开关只在此人格下可用

### dev-pair(结伴开发)= 新增,Claude Code 式

系统提示词草案(实现时直接用):

> You are the pair-programming agent working WITH the user in conversation.
> Follow the user's direction — their latest message defines the task.
> Answer questions directly; do NOT start coding when the user is only asking
> or discussing. Before non-trivial changes, state a one-line plan first.
> When requirements are ambiguous, ask a short clarifying question instead of
> guessing. Record requirements/defects only when the user asks, or when you
> complete something worth tracking (then update status honestly). Goals in
> context are background, NOT instructions — never auto-advance them.
> Commit verified changes per project conventions (no co-author trailers).

- 提案行为归属此人格:先说一行计划再动手;拿不准就问(R-029 question 工具落地后走结构化提问)
- goal 注入仍在(背景知识),但明确"不是指令"
- 连跑在此人格下禁用(勾选时前端提示切到自主)

### 切换与默认

- 前端顶栏模式选择器扩展为:`结伴开发(默认)| 自主推进 | research`
  —— 映射 profile+agent:dev+pair / dev+dev / research
- **默认结伴**:用户在场打字 = 对话优先;想让它自己跑再切自主(或直接勾连跑,自动切)
- 后端:run_prompt 增加 agent 参数 → select_agent(Some(name));CLI 走 KANZEI_AGENT(已支持)

## 实现状态（2026-08-06）

已完成首个闭环：桌面端模式选择器现在提供“结伴开发”（默认）、“自主推进”和 research；前两者都使用 dev profile，但分别显式选择 `dev-pair` 与 `dev` agent。`run_prompt` 已将 agent 参数传入 harness 的 `select_agent`，因此请求不会再固定使用 profile 的第一个 agent。连跑仅允许自主推进模式，切换到结伴开发或 research 会自动关闭连跑，并在手动勾选时给出提示。

R-029 已完成首个闭环：新增 `question` 工具，runner 将问题、选项和默认值转为统一 `AskRequest::Question`，桌面端复用 ask 队列与弹窗，支持选项点击、文本回答和取消；CLI 支持终端输入。权限询问仍保持原有 once/always/deny 语义，自动放行不会跳过 question。



- R-027(需求分析沟通模式)的"沟通模式"部分并入本设计(dev-pair 即结构化沟通的宿主);
  缺陷查找入口如仍需要,单独再立
- R-030 多进程落地后,模式成为**进程属性**:一个自主进程连跑 backlog,一个结伴进程随叫随到,互不干扰——这是两个设计的合流点

## 代理职责划分(用户反馈:现在有点乱)

| 角色 | 职责 | 硬边界 |
|---|---|---|
| 主 agent(primary) | 写码、改文件、跑测试、提交——一切有副作用的事 | 唯一有写权限的执行者 |
| task 子代理 | 只读探索:找文件/调用点/用法,读段代码给结论 | 快照只含 read/glob/grep,代码层面无写 |
| ├ fast 档 | 机械检索(答案是行号/列表) | 任务必须窄而明确 |
| └ primary 档 | 需要读懂代码的探索 | 仍然只读 |
| fast 模型直用 | 总结/压缩/标题类杂活(app 内部调用) | 不进对话循环 |

观察到的偏差与对策:

1. 主 agent 有时自己下场干机械检索(该派 task),有时把需要理解的活派给 fast 白跑一轮再重派——提示词已给"探索优先派 task",派单质量随模型;**不追求提示词完美,靠 fast 失败自动可见(结果空)+ primary 档兜底**
2. 收尾文书(req/goal 更新、提交)挤在最后一轮,遇上 40 步耗尽就演变成 D-027 的 JSON 乱喷——修复:最后一步注入收尾指令(已落);根治:提示词要求**边做边记,不攒到最后**(已在提交纪律里,持续观察)
3. 模式人格(见上)本质也是职责划分:自主人格管 backlog,结伴人格管对话——切开后主 agent 不再一心二用

## 附:对话为主布局(R-037,前端)

用户反馈:主对话区几乎全是工具指令块,对话被淹没。方向:

- 主区只保留:用户消息、assistant 文本、思考头、轮次分隔;**工具块降为一行淡色痕迹**(点击展开详情)
- 右侧面板从"后台任务"升级为"活动":运行期间所有工具调用按序入列(带状态/耗时),diff/终端详情点击在面板内展开;运行结束保留最近一轮可回看,新一轮开跑时清空
- 层级:对话(主)> 思考(淡)> 工具(右侧/一行痕迹)——信息清晰原则的落地
- 实现注意:与 R-030 页签、R-013 回放共享渲染状态重构,一起做省两遍工
