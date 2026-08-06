# Requirements

## R-007 复用订阅额度:Claude Code(OAuth)/Codex 凭证当 provider [doing]
- 已完成: Codex 凭证(auth.json 刷新回写、Responses 协议、gpt-5.6 三兄弟)
- 剩余: Claude Code OAuth provider(~/.claude/.credentials.json)
- 进展: 已完成 Claude OAuth token 自动刷新：接入 console.anthropic.com/v1/oauth/token，使用 Claude Code client_id 与 refresh_token，在过期前 5 分钟刷新并回写 accessToken/refreshToken/expiresAt；构建请求继续复用 Anthropic OAuth headers。cargo test --workspace 全部通过。剩余真实 Claude Code 端到端验证。

## R-010 需求与缺陷分级及可编辑管理 [done]
- 范围: 需求/缺陷等级与字段编辑、按等级筛选排序、非法修改拒绝并提示
- 已完成: 侧边栏展开编辑、状态流转按钮、缺陷 severity 展示
- 剩余: 需求优先级枚举、列表筛选/排序
- 完成说明: 需求条目新增 P0/P1/P2/P3 优先级枚举并由 tracker 硬门禁校验；桌面端 docs_snapshot 透传优先级，支持需求按状态/优先级筛选及按优先级、状态、编号排序；编辑入口支持保存优先级。cargo test --workspace 与 node --check crates/kanzei-app/ui/main.js 全部通过。
- 验收: 非法优先级被拒绝并提示合法值；需求列表可按状态和优先级筛选，并按优先级、状态、编号排序；优先级持久化在 requirements.md 的“优先级”字段。

## R-013 支持回到之前的对话 [todo]
- 范围: 会话列表、历史会话加载与继续对话(R-003 落地后的 UI 层)
- 验收: 用户可查看历史会话并打开任意会话,消息上下文正确恢复后继续对话
- refs: R-003

## R-014 多模态模型支持上传图片和 PDF 等文件 [todo]
- 已完成: 三协议的 image/document 消息映射(协议层就绪)
- 剩余: 前端上传/粘贴入口(与 R-024 一起做)
- refs: R-024

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
