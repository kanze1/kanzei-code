# Requirements

## R-001 harness 双模式 dev/research profile [done]
- 本次工作: 完成项目结构、代码实现、测试结果与风险分析；未修改业务代码

## R-002 Tauri 桌面端(类 VSCode 布局) [dropped]

## R-003 SQLite 事件溯源 + steer/queue 调度(M2) [doing]
- 当前工作: 继续完善 SQLite 事件溯源与 steer/queue 调度，同时完成新需求 R-020 的 diff 默认收纳展示
- 范围顺序: R-003 → R-004 → R-007 → R-008 → R-009 → R-010 → R-011 → R-012 → R-013 → R-014 → R-016
- 说明: R-003 涉及 SQLite 表结构、事件语义和调度核心流程，按仓库规则先提交方案，不直接改代码。
- 实施顺序: 阶段一：R-003 → R-009 → R-013
- 当前阶段: 继续完善 runner 内部事件边界与 steer/queue 活跃调度；R-009 将复用同一事件日志做完整消息历史投影
- 实现内容: 新增 state.db 迁移、SessionStore、事件追加与 steer/queue 输入存储；随后接入 runner
- 已完成: CLI 与桌面端在项目 .kanzei/state.db 记录 session 创建、prompt.admitted、prompt.promoted、run.completed/run.failed；core 提供项目状态路径和稳定 session_id
- 文档: 新增 docs/design/m2-sqlite-store.md，说明 schema、迁移与回滚；.kanzei/state.db 已加入 .gitignore
- 测试: cargo test -p kanzei、cargo test -p kanzei-app 已通过
- 当前进度: 已完成 SQLite 会话状态生命周期持久化：running/idle/failed 状态更新、状态事件和核心测试；CLI/桌面端均已接入。后续仍需 steer 前端入口、运行中 queue drain、事件恢复消息历史。
- 验证: cargo test -p kanzei-core：8 passed；cargo build -p kanzei-app 通过；node --check ui/main.js 通过；cargo test --workspace 全部通过。格式检查仅保留仓库既有 app/tools 差异。下一步：运行中 queue admission/drain。
- 进展: 已修复 D-021 的递归 runner Send 编译阻塞：run_once 改为显式生命周期的 Send boxed future；同时完成 R-003 事件恢复第一阶段代码。cargo test --workspace 全部通过（kanzei-core 10 tests），node --check 通过。下一步：提交当前已验证的 R-003/R-012 改动，并继续按编号推进。

## R-004 本地模型跑并行子代理(M4) [done]

## R-005 桌面端基础件:多项目管理/运行状态/设置页 [done]

## R-006 桌面端 UI 美化(用户反馈:现在有点丑) [done]

## R-007 复用订阅额度:Claude Code(OAuth)/Codex 凭证当 provider [doing]

## R-008 自举:用 kanzei 开发 kanzei(dev 模式吃自己狗粮) [doing]
- 当前工作: 按用户确认重新构建桌面端与工作区，完成后由用户重启验证
- 本次工作: 按仓库 §9 执行发版构建，并使用 kz --version 验证安装结果
- 发版验证: cargo test --workspace 全部通过；kz CLI 已安装并验证为 kanzei 0.1.0 (0aca267 2026-08-06)。kzapp release 构建成功，但因正在运行，安装结果为 kzapp.exe.pending，需关闭 kzapp.exe 后重跑 .\scripts\release.ps1。

## R-009 对话历史记录持久化 [doing]
- 范围: 持久化主Agent与子Agent的对话消息、会话元数据、时间顺序及关联任务，重启后可恢复并查询
- 验收: 应用重启后历史记录不丢失；可按会话查看消息、角色、时间和任务关联；写入失败有明确错误且不阻塞当前会话
- 关联缺陷: D-008 上下文超限错误未被正确处理已修复
- 备注: 本次修复接入运行时上下文超限的一次性安全压缩重试；完整持久化历史压缩仍属于后续能力。

## R-010 需求与缺陷分级及可编辑管理 [doing]
- 范围: 需求和缺陷均支持等级/优先级编辑，支持对标题、描述、验收标准、复现信息、状态等字段进行编辑并保留变更记录
- 验收: 可编辑已有需求和缺陷；等级至少可区分高/中/低或等价枚举；列表可按等级筛选/排序；非法修改被拒绝并提示原因

## R-011 Agent 通用工具能力对齐 Codex 与 Claude Code [doing]
- 范围: 为Agent提供统一工具抽象，覆盖文件读写/编辑、命令执行、搜索、任务跟踪及并行调用等能力，并评估Codex与Claude Code的功能差异
- 验收: 工具协议和权限边界统一；支持安全的并行工具调用与冲突处理；完成Codex/Claude Code能力矩阵；关键能力有集成测试和失败可恢复行为

## R-012 将子Agent调度能力开放给主Agent [done]
- 范围: 主Agent可按任务创建、排队、并发调度、暂停/取消、汇总子Agent，并复用统一的事件与结果协议
- 验收: 主Agent可发起并行子任务并获得可关联结果；支持并发上限、失败/超时、取消和重试；调度过程可观测且不破坏主会话；结果可回写历史和事件流
- 进展: 按用户要求定位 SubagentRuntime 与 explore_agent 定义位置。

## R-013 支持回到之前的对话 [doing]
- 范围: 会话列表、历史会话加载与继续对话
- 验收: 用户可查看历史会话并打开任意会话，消息上下文正确恢复后继续对话

## R-014 多模态模型支持上传图片和 PDF 等文件 [doing]
- 范围: 聊天附件上传、图片/PDF 文件传递与模型能力适配
- 验收: 用户可上传图片、PDF 等受支持文件；界面展示附件并在发送时传递给支持多模态的模型，模型不支持时给出明确提示

## R-015 对话全状态显示:edit/write diff 可视化、bash 终端块、轮次标记、思考块修正、markdown 渲染、git 状态、侧边栏开发规范 [done]

## R-016 kzapp 启动时自动完成 pending 自更新(检测 kzapp.exe.pending 并自替换,发版后重启即新版) [doing]
- 备注: 用户反馈发版/启动时仍短暂弹黑色终端窗口；本次修复启动链路并执行发版验证。

## R-017 终端命令执行不弹出黑色控制台窗口 [done]
- 范围: Windows 桌面端和 CLI 触发的终端命令执行
- 验收: 执行 bash/shell 工具时不额外弹出独立黑色控制台窗口；命令输出仍能正常回传并显示；命令失败行为不变
- 当前阶段: 实现 Windows shell 子进程隐藏控制台窗口，并补充平台条件测试/构建验证
- 实现: Windows shell 子进程设置 CREATE_NO_WINDOW；stdout/stderr 管道和超时/终止逻辑保持不变
- 影响范围: 仅 kanzei-tools bash 工具的 Windows 进程创建
- 测试: cargo test -p kanzei-tools 通过；Windows 条件代码成功编译

## R-018 对话结束时播放提示音并显示完成提示 [todo]
- 范围: 桌面端对话运行结束，包括成功、失败、用户停止
- 验收: 对话结束后播放一次提示音并显示可见完成提示；应用失焦时仍可感知；播放或通知失败不能影响对话结果

## R-019 支持设定目标并持久化长期工作 [done]
- 范围: 项目级目标管理与长期工作状态
- 验收: 用户可以创建、查看、编辑、完成和归档目标；目标持久化到项目状态，应用重启后仍可恢复；目标可关联会话、需求和缺陷；目标状态变更有记录且写入失败明确提示

## R-020 编辑 diff 默认收纳并显示改变量摘要 [done]
- 影响范围: 桌面端对话结果中的编辑 diff 展示。
- 需求: 编辑工具产生的 diff 默认折叠，不自动展开完整 diff 内容；折叠状态下仅显示改动文件、增删行数等改变量摘要，用户主动点击后才展开具体 diff。
- 验收: 发生编辑操作后，UI 中 diff 区块默认收纳；可见文件名/改动统计；点击后展开具体 diff，再次点击可收起。
- 当前工作: 定位编辑 diff UI，并将默认展开改为默认收纳，保留改变量摘要和手动展开
- 实现内容: 桌面端编辑 diff 默认收纳，工具头部显示文件路径和增删统计，点击头部可展开/收起详情。
- 验证: node --check crates/kanzei-app/ui/main.js；cargo build -p kanzei-app；cargo test -p kanzei-core，均通过。

## R-021 上下文自动压缩:占用超阈值自动总结并延续对话,压缩不丢数据可召回(替代手动总结+新对话) [done]

## R-022 LLM 请求瞬断自动重试:流未建立时自动重试(退避),UI 显示重试中,代理抖动不再整轮失败 [done]

## R-023 research 模式补 webfetch/websearch 工具(走代理,输出截断,来源可直接 source add) [doing]

## R-024 输入体验:提示词历史(上下箭头)、@文件引用补全、粘贴/拖拽图片文件(接通 R-014) [todo]

## R-025 权限规则管理:设置页查看/删除已记住的放行规则(现在只能手改 toml) [todo]

## R-026 glob/grep 检索工具:ripgrep 内核带 head-limit 早停,当前 agent 只能用 bash 绕路 [done]

## R-027 需求分析沟通模式与缺陷查找入口 [todo]
- 优先级说明: 按新增需求纳入整体需求顺序，排在现有需求之后。
- 范围: 新增需求分析沟通模式：围绕需求澄清、边界、验收标准进行结构化沟通；新增缺陷查找按钮：提供明确的用户入口触发缺陷查找/诊断流程，并展示查找状态与结果。
- 验收: 用户可在桌面端进入需求分析沟通模式并开始分析；用户可点击缺陷查找按钮触发缺陷查找，看到进行中、完成、失败或无结果状态；具体交互方案与权限边界在实现前补充确认。

## R-028 todo 工具:运行内任务清单(pending/in_progress/done),长连跑会话的结构化计划 + 前端可视化 [todo]

## R-029 question 工具:agent 结构化向用户提问(带选项),复用 ask 弹窗通道,替代纯文本猜测 [todo]
