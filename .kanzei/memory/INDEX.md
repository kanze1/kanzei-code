# Memory Index (project)

- M-001 [fact] 前端动态 i18n 必须保存源文案并在语言切换时重算 — 处理前端动态 i18n 中文内容无法回译/切换失效故障时必读
- M-003 [fact] tracker 状态机只进不退,doing→todo 会被直接拒绝 — req/defect/goal 的 update 反复报 cannot move backward 时必读:状态只能沿列表顺序前进
- M-004 [fact] TrackerTool req/defect list 阻塞感知稳定后置，不改写 Markdown 顺序 — 处理 TrackerTool req/defect list 输出、阻塞条目排序或相关回归测试时必读:list 具备阻塞感知稳定后置能力
- M-005 [sop] .kanzei/project 托管文件禁止 edit,须用专用工具 — 处理 .kanzei/project 下文件 edit 被 ruleset 拒绝(permission denied / policy-managed)时必读
- M-006 [fact] 前端需求/缺陷显示阻塞原因与筛选,独立文档页顺序与调度一致 — 处理需求/缺陷 UI 阻塞显示与筛选、独立文档页排序、docs_snapshot 或 renderDocList 相关改动/回归时必读
- M-007 [fact] 设置页工作资料导出功能(export_project_data) — 需要了解/修改设置页导出记忆、需求、缺陷、项目配置功能(实现位置、目录约束、返回值)时必读
- M-008 [fact] runner 首次请求统一清洗 prior 历史(filter_message_history) — 调试 runner 首请求消息构造、prior 历史孤儿 ToolCall/ToolResult、上下文压缩相关问题时必读
- M-009 [sop] edit 报 old_string not found 时须先 read 重读文件再精确匹配 — 处理 edit 替换失败(old_string not found / must match exactly including whitespace)时必读:先 read 重读磁盘实际内容再构造 old_string;报错自带 "Closest line in file" 提示会直接揭示文件真实排版(多个 key 挤同一行 vs 自己按每行一个 key 构造),按该排版重造 old_string。已复发于 main.rs 的 #[test] 缩进(2 次)。
- M-010 [sop] edit 报 old/new 相同是 no-op 拒绝而非失败 — 处理 edit 报“old_string and new_string are identical — nothing to do”时必读：停止重试，先 read 确认目标是否已是期望状态；若未完成则修改 new_string 使其与 old_string 不同，不要用 bash 绕过。
- M-012 [fact] ID 同现于活动与归档时完整性门禁拒绝所有 tracker 写操作 — goal/defect/req 写操作报 tracker integrity is broken / present in BOTH active and archive 时必读
- M-013 [fact] git commit 报 exit code 1 且提示 Changes not staged 表示没有暂存内容 — git commit 失败(exit code 1、"Changes not staged for commit")时必读:先检查同批前置 git add 是否已报 pathspec did not match;不能判定时只记症状,不要断言忘记 add。关联 D-159
- M-014 [fact] HTML 静态文案必须登记进资源表,否则断言测试失败 — node 断言测试报 "HTML 静态文案未进入资源表"(AssertionError,exit code 1)时必读:新增/修改 HTML 静态 UI 文案后必须同步登记到资源表
- M-015 [fact] SSE 流内 context overflow 恢复须重建请求,OpenAI 错误分类须同查 type/code — 调试 kanzei-core runner SSE 流内 context overflow 恢复、压缩后仍发超长历史、或 OpenAI context_length_exceeded 未被识别时必读
- M-019 [sop] bash 整文件重写(Set-Content)被环境拦截,须用 edit 做定点修改 — bash 里用 Set-Content / 重定向整文件重写被拦截(报 "whole-file rewrites via shell bypass the edit/write tools' syntax validation and diff display")时必读;也说明 edit 容忍换行符差异、连续两次 miss 后展示文件实际内容
- M-021 [sop] edit 报 old_string 匹配多处时先 read 定位并收窄，非批量勿设 replace_all — 处理 edit 报“old_string matches N locations”时必读：不要重复提交同一个宽泛 old_string；先 read 当前目标文件并用文件路径、函数/区块边界及邻近行构造唯一上下文，确认仅命中 1 处后再 edit。只有明确要改全部命中时才设 replace_all=true，并先核对每个命中范围。
- M-022 [sop] Rust/验证失败勿用 bash 反复跑测试，改用结构化验证 — 处理 Rust 测试、verify.ps1 或 smoke probe 在 bash 返回 exit code 1、但输出包含具体业务断言失败时必读：不要重复 bash/cargo 重跑；先按断言定位实现问题，并用 test_record/结构化验证记录终态。
- M-023 [fact] edit 报 cannot read 拒绝访问 (os error 5) 是瞬态错误,重试即成功 — 处理 edit 报 "cannot read ... 拒绝访问 (os error 5)" 时必读:这是 Windows 瞬态访问拒绝,不是真实权限/路径问题——先 read 重读再重试 edit 即可成功,不要改 bash 绕过,也不要误判为死路而放弃。
- M-026 [sop] test_record 请求校验缺失必补、非重试即成功可复用知识 — [fp] detection key — 处理 test_record 输入验证失败（缺少字段/重复提交）必读：补全必填字段再发，避免环境误判为死路
- M-027 [fact] edit 替换若净删除内容,须确认 allow_deletion 或改为插入式替换 — 处理 edit 报「净删除 N 行」或「看着像插入却没保住 old_string 原文」时必读:先 read 重读目标文件核对删除意图;本意是插入就把 old_string 原文逐行原样写进 new_string 再追加新内容,只有确认要删除才设 allow_deletion=true;正文的 [fp:edit|...] 标记是复发检测的键,不得删改。
- M-029 [fact] 所有 git mutation 在 bash 都被拦截,必须走结构化 git 工具 — 处理任何 Git 分支/索引变更(merge/restore/rebase/add/commit/reset)在 bash 报 "is blocked in bash: git mutations must use the structured git tool" 时必读:不要换别的 git 子命令重试,改用结构化 git 工具——显式 stage 指定文件、核对 staged_hash/diff,再用该 hash commit;快进合并走 git merge_ff。
- M-030 [fact] 鞭挞与 backlog 判定集中在引擎，前端只执行 autoAction — 处理自动运行鞭挞、backlog 或继续文案改动时必读：判定逻辑只改 harness/kanzei-tools 单源，桌面端转发，前端仅执行 autoAction；不要在前端重复判定或维护旧继续文案。
- M-032 [fact] R-163 记忆回放评估台已交付(六臂对照量化记忆决策值) — 需要用 CLI kz replay-eval 做记忆决策价值评估、或要改 replay.rs 数据层 / Arm 六臂枚举 / J 判据与报告渲染时必读:交付形态、四批实现分工与关键接口约定都在这里。
- M-041 [sop] autonomous 会话报 permission requires user approval 是档位限制,不是死路 — 处理 autonomous(自动推进)会话里 edit/bash/git/cargo/conventions_patch 被拒并报 "permission requires user approval" 时必读:这是权限档位而非工具故障——把该动作留给交互轮或先在 .kanzei/kanzei.toml 加白名单;不要反复重试、不要换等价命令绕道、也不要判定为死路而放弃整条任务。
- M-058 [sop] 发版 SOP:dev 全绿 → 发布树 ff → verify 十步 → package -Ack → 容器外静默装 — 执行发版动作(打包/发布/装机)时必读:标准步骤序列与三个已知坑位
- M-059 [sop] 记忆清理 SOP:归档不裸删,动全局先确认恢复源,清后三处一致 — 手动清理 .kanzei/memory 或 ~/.kanzei/memory 前必读:防数据永久丢失与索引悬空
- M-061 [sop] 自举复盘 SOP:审提交流水、验收证据、测试面匹配、证据链、残留 — 每次自举运行结束后做质量复盘时必读;方法来自 2026-08-13 对 R-199 运行的复盘(逮出 D-320)
- M-062 [fact] 环境约束:本机 WebView2 151 DevTools 端口从不绑定,e2e CDP 路线不可用 — 想走 e2e-smoke / connectOverCDP / WebView2 DevTools 端口路线前必读:当前机器已 9 轮实验证实不可用,不要重推

(4 candidate 条待验证晋升)
