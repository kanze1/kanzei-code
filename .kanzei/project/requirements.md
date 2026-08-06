# Requirements

## R-007 复用订阅额度:Claude Code(OAuth)/Codex 凭证当 provider [done]
- 已完成: Codex 凭证(auth.json 刷新回写、Responses 协议、gpt-5.6 三兄弟)
- 剩余: Claude Code OAuth provider(~/.claude/.credentials.json)
- 进展: 已完成 Claude OAuth token 自动刷新：接入 console.anthropic.com/v1/oauth/token，使用 Claude Code client_id 与 refresh_token，在过期前 5 分钟刷新并回写 accessToken/refreshToken/expiresAt；构建请求继续复用 Anthropic OAuth headers。cargo test --workspace 全部通过。剩余真实 Claude Code 端到端验证。

## R-014 多模态模型支持上传图片和 PDF 等文件 [doing]
- 已完成: 三协议的 image/document 消息映射(协议层就绪)
- 剩余: 前端上传/粘贴入口(与 R-024 一起做)
- refs: R-024
- 进展: 开始推进前端多模态入口，范围与 R-024 输入体验关联；先检查现有 composer、拖拽/剪贴板事件和 LlmRequest 图片文档字段。

## R-016 kzapp 启动时自动完成 pending 自更新 [todo]
- 范围: 启动检测 kzapp.exe.pending 并自替换,发版后重启即新版

## R-018 对话结束时播放提示音并显示完成提示 [todo]
- 验收: 成功/失败/停止均提示;失焦可感知;通知失败不影响对话结果

## R-023 research 模式补 websearch 工具 [todo]
- 已完成: webfetch(走代理、输出截断)
- 剩余: websearch 检索入口,结果可直接 source add

## R-024 输入体验:提示词历史(上下箭头)、@文件引用补全、粘贴/拖拽图片文件 [todo]
- refs: R-014

## R-025 权限规则管理:设置页查看/删除已记住的放行规则 [todo]

## R-027 需求分析沟通模式与缺陷查找入口 [todo]
- 范围: 需求澄清/边界/验收的结构化沟通模式;缺陷查找按钮与状态展示
- 验收: 具体交互方案与权限边界在实现前补充确认

## R-028 todo 工具:运行内任务清单,长连跑会话的结构化计划 + 前端可视化 [todo]

## R-029 question 工具:agent 结构化向用户提问(带选项),复用 ask 弹窗通道 [todo]

## R-030 进程与项目解耦:多进程并行,每进程独立模型选择与子代理开关(设计 docs/design/r030-process-decoupling.md) [todo]
- 前端优先级: P1
- 来源文档: docs/design/frontend-phase3.md
- 验收补充: 进程与项目解耦后，多个进程可并行运行且各自拥有模型选择与子代理开关；前端以页签/进程视图呈现。

## R-031 子代理轨迹透视:task 块可展开子代理完整工具轨迹,后台面板历史可回看 [todo]
- 前端优先级: P1
- 来源文档: docs/design/frontend-phase3.md
- 验收补充: task 块可展开查看子代理完整工具轨迹，后台面板条目可回看，不因短时超时消失。

## R-032 队列可视化:排队输入列表(内容/交付方式)+ 单条撤销 [todo]
- 前端优先级: P1
- 来源文档: docs/design/frontend-phase3.md
- 验收补充: 运行中的 queue 输入显示内容与交付方式，并支持单条撤销，状态与后端 admission 同步。

## R-033 阅读体验:智能滚动跟随+回到最新按钮、消息一键复制、对话内搜索 [todo]
- 前端优先级: P0
- 来源文档: docs/design/frontend-phase3.md
- 验收补充: 消息阅读支持智能滚动跟随、回到最新、单条复制和对话内搜索，滚动历史时不被新事件强制拉回底部。

## R-034 research 模式前端:来源/发现侧边栏、引用跳转、报告入口 [todo]
- 前端优先级: P1
- 来源文档: docs/design/frontend-phase3.md
- 验收补充: research 模式显示来源/发现侧边栏，引用可跳转，支持报告入口，并与后端 sources/findings 对齐。

## R-035 diff 查看器升级:语法高亮、并排视图、多文件改动汇总 [todo]
- 前端优先级: P2
- 来源文档: docs/design/frontend-phase3.md
- 验收补充: diff 支持语法高亮、并排视图及多文件改动汇总。
