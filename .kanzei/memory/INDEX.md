# Memory Index (project)

- M-001 [fact] 前端动态 i18n 必须保存源文案并在语言切换时重算 — 处理前端动态 i18n 中文内容无法回译/切换失效故障时必读
- M-002 [preference] 开发重心:需求优先 — 取活/排优先级时必读:当前项目该先做什么
- M-003 [fact] tracker 状态机只进不退,doing→todo 会被直接拒绝 — req/defect/goal 的 update 反复报 cannot move backward 时必读:状态只能沿列表顺序前进
- M-004 [fact] TrackerTool req/defect list 阻塞感知稳定后置，不改写 Markdown 顺序 — 处理 TrackerTool req/defect list 输出、阻塞条目排序或相关回归测试时必读:list 具备阻塞感知稳定后置能力
- M-005 [sop] .kanzei/project 托管文件禁止 edit,须用专用工具 — 处理 .kanzei/project 下文件 edit 被 ruleset 拒绝(permission denied / policy-managed)时必读
- M-006 [fact] 前端需求/缺陷显示阻塞原因与筛选,独立文档页顺序与调度一致 — 处理需求/缺陷 UI 阻塞显示与筛选、独立文档页排序、docs_snapshot 或 renderDocList 相关改动/回归时必读
- M-007 [fact] 设置页工作资料导出功能(export_project_data) — 需要了解/修改设置页导出记忆、需求、缺陷、项目配置功能(实现位置、目录约束、返回值)时必读
- M-008 [fact] runner 首次请求统一清洗 prior 历史(filter_message_history) — 调试 runner 首请求消息构造、prior 历史孤儿 ToolCall/ToolResult、上下文压缩相关问题时必读
- M-009 [sop] edit 报 old_string not found 时须先 read 重读文件再精确匹配 — 处理 edit 替换失败(old_string not found / must match exactly including whitespace)时必读:先 read 重读磁盘实际内容再构造 old_string
- M-010 [sop] edit 报 old/new 相同是 no-op 拒绝而非失败 — 处理 edit 报 "old_string and new_string are identical — nothing to do" 时必读:这是 no-op 拒绝而非真失败,说明目标内容已是期望状态或 old/new 复制成同一段,不要改用 bash 绕过
- M-011 [sop] 活动/归档同 ID 语义不同时用 repair_reused_id 修复,勿直接编辑托管文档 — 处理 tracker 完整性门禁报 present in BOTH active and archive / 活动与归档同 ID 语义不同的修复时必读
- M-012 [fact] ID 同现于活动与归档时完整性门禁拒绝所有 tracker 写操作 — goal/defect/req 写操作报 tracker integrity is broken / present in BOTH active and archive 时必读
- M-013 [fact] git commit 报 exit code 1 且提示 Changes not staged 表示没有暂存内容 — git commit 失败(exit code 1、"Changes not staged for commit")时必读:先检查同批前置 git add 是否已报 pathspec did not match;不能判定时只记症状,不要断言忘记 add。关联 D-159
- M-014 [fact] HTML 静态文案必须登记进资源表,否则断言测试失败 — node 断言测试报 "HTML 静态文案未进入资源表"(AssertionError,exit code 1)时必读:新增/修改 HTML 静态 UI 文案后必须同步登记到资源表
- M-015 [fact] SSE 流内 context overflow 恢复须重建请求,OpenAI 错误分类须同查 type/code — 调试 kanzei-core runner SSE 流内 context overflow 恢复、压缩后仍发超长历史、或 OpenAI context_length_exceeded 未被识别时必读
- M-016 [fact] docs 目录整理(2026-08-08):design 统一 snake_case、reference 归档 opencode-archive、R-050 移入 deep_parallel_dev、旧 G-003 重编号 G-005 — 处理 docs 目录/文档位置、R-050 POC 方案出处、goals 编号 G-003/G-005、architecture README 索引缺失条目(direction_taste/memory_system/deep_parallel_dev)时必读
- M-017 [fact] 需求条目缺 `- 优先级:` 字段时前端徽章显示「未设」(原 P?),新建需求必须填 P0-P3 — 处理需求优先级徽章显示、renderDocList/pri-badge 改动、新建需求条目、或界面上看到 P?/「未设」徽章疑似解析故障时必读
