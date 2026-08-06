# Goals

## G-001 把 kanzei 打磨成日常主力开发工具 [active]
- 类型: 长期
- 说明: 方向性目标,不主动关闭;好用压倒一切(上下文透明/多agent协作/快/少打断/信息清晰)

## G-002 前端与后端能力对齐 [active]
- 类型: 短期
- priority: P0
- 验收: R-036、R-033 完成且 R-030/R-037 由 Claude 落地后,前端不再落后于后端能力;达成即 `goal update G-002 achieved`
- 来源文档: docs/design/frontend-phase3.md、docs/design/interaction-modes.md
- 进展: R-036 已完成：新增 dev-pair 结伴人格，桌面端模式选择器默认结伴开发并支持自主推进/research，run_prompt 已传递显式 agent，连跑仅在自主推进模式可用。R-033 已完成：消息区智能滚动跟随/回到最新、消息与工具一键复制、对话内搜索及上下匹配跳转。下一步按 priority 推进 R-030/R-037（由 Claude 落地）后的剩余前端对齐项。

## G-003 工具面与体验补全 [active]
- 类型: 短期
- priority: P1
- 验收: R-016、R-028、R-029、R-023 全部 done;达成即 `goal update G-003 achieved`
- 说明: 发版自更新闭环 + agent 工具面(todo/question/websearch)
- 进展: R-023 已完成：research profile 新增受权限控制的 websearch，复用代理配置，返回有界结构化搜索结果并补充解析测试。R-029 已完成：question 工具新增结构化提问，runner 统一 AskRequest/AskResponse，桌面端复用 ask 弹窗支持选项/文本/取消，CLI 支持终端输入。R-028 已完成：dev profile 新增 todowrite，运行内计划整体替换与状态校验，桌面端右侧当前计划面板显示条目和完成比例。G-003 验收项 R-016 尚未完成，下一步推进发版自更新闭环。
