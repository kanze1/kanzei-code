# Requirements

## R-016 kzapp 启动时自动完成 pending 自更新 [todo]
- priority: P1
- 归属: kanzei
- 范围: 启动检测 kzapp.exe.pending 并自替换,发版后重启即新版

## R-018 对话结束时播放提示音并显示完成提示 [todo]
- priority: P3
- 归属: kanzei
- 验收: 成功/失败/停止均提示;失焦可感知;通知失败不影响对话结果

## R-023 research 模式补 websearch 工具 [todo]
- priority: P2
- 归属: kanzei
- 已完成: webfetch(走代理、输出截断)
- 剩余: websearch 检索入口,结果可直接 source add

## R-024 输入体验:提示词历史(上下箭头)、@文件引用补全 [todo]
- priority: P2
- 归属: kanzei
- 备注: 粘贴/拖拽附件已随 R-014 完成,本条剩输入框体验

## R-025 权限规则管理:设置页查看/删除已记住的放行规则 [todo]
- priority: P2
- 归属: kanzei

## R-028 todo 工具:运行内任务清单,长连跑会话的结构化计划 + 前端可视化 [todo]
- priority: P1
- 归属: kanzei

## R-029 question 工具:agent 结构化向用户提问(带选项),复用 ask 弹窗通道 [todo]
- priority: P1
- 归属: kanzei
- refs: R-036
- 备注: 结伴开发人格的"拿不准就问"依赖此工具

## R-030 进程与项目解耦:多进程并行,每进程独立模型选择与子代理开关 [todo]
- priority: P0
- 归属: Claude
- 设计: docs/design/r030-process-decoupling.md
- 验收: 多进程可并行运行,各自拥有模型选择与子代理开关;前端以进程页签呈现;默认进程兼容既有历史
- 备注: 大手术,与 R-037 的渲染层重构一起做

## R-031 子代理轨迹透视:task 块可展开子代理完整工具轨迹,后台面板历史可回看 [todo]
- priority: P1
- 归属: kanzei
- 验收: task 块可展开查看子代理完整工具轨迹,后台面板条目可回看,不因短时超时消失

## R-032 队列可视化:排队输入列表(内容/交付方式)+ 单条撤销 [todo]
- priority: P1
- 归属: kanzei
- 验收: 运行中的 queue 输入显示内容与交付方式,支持单条撤销,状态与后端 admission 同步

## R-033 阅读体验:智能滚动跟随+回到最新按钮、消息一键复制、对话内搜索 [todo]
- priority: P0
- 归属: kanzei
- 验收: 滚动历史时不被新事件强制拉回底部;有"回到最新"按钮;单条消息可复制;对话内可搜索

## R-034 research 模式前端:来源/发现侧边栏、引用跳转、报告入口 [todo]
- priority: P2
- 归属: kanzei
- 验收: research 模式显示来源/发现侧边栏,引用可跳转,支持报告入口,与后端 sources/findings 对齐

## R-035 diff 查看器升级:语法高亮、并排视图、多文件改动汇总 [todo]
- priority: P3
- 归属: kanzei

## R-036 双状态 agent:自主推进(backlog驱动/连跑)与结伴开发(Claude式对话协作) [todo]
- priority: P0
- 归属: kanzei
- 设计: docs/design/interaction-modes.md(含 pair 人格系统提示词草案,可直接用)
- 验收: 顶栏可切换 结伴(默认)/自主/research;结伴人格问答不开工、动手前说计划、连跑禁用;自主人格保持现有纪律

## R-037 对话为主布局:主区只留对话与思考,工具活动收束到右侧活动面板 [todo]
- priority: P0
- 归属: Claude
- 设计: docs/design/interaction-modes.md 附录
- 验收: 主区只保留用户消息/assistant 文本/思考头/轮次分隔,工具降为一行痕迹;右侧活动面板按序列出全部工具调用,详情(diff/终端)面板内展开
- 备注: 与 R-030 页签共用渲染状态重构,一起做
