# Memory Index (project)

- M-001 [fact] 前端动态 i18n 必须保存源文案并在语言切换时重算 — 处理前端动态 i18n 中文内容无法回译/切换失效故障时必读
- M-003 [fact] tracker 状态机只进不退,doing→todo 会被直接拒绝 — req/defect/goal 的 update 反复报 cannot move backward 时必读:状态只能沿列表顺序前进
- M-004 [fact] TrackerTool req/defect list 阻塞感知稳定后置，不改写 Markdown 顺序 — 处理 TrackerTool req/defect list 输出、阻塞条目排序或相关回归测试时必读:list 具备阻塞感知稳定后置能力
- M-005 [sop] .kanzei/project 托管文件禁止 edit,须用专用工具 — 处理 .kanzei/memory 或其他 policy-managed 文件写入时必读：不要调用 edit，改用 memory_note/记忆管理工具提交变更；遇 permission denied by ruleset 立即切换合法写入通道。
- M-006 [fact] 前端需求/缺陷显示阻塞原因与筛选,独立文档页顺序与调度一致 — 处理需求/缺陷 UI 阻塞显示与筛选、独立文档页排序、docs_snapshot 或 renderDocList 相关改动/回归时必读
- M-007 [fact] 设置页工作资料导出功能(export_project_data) — 需要了解/修改设置页导出记忆、需求、缺陷、项目配置功能(实现位置、目录约束、返回值)时必读
- M-008 [fact] runner 首次请求统一清洗 prior 历史(filter_message_history) — 调试 runner 首请求消息构造、prior 历史孤儿 ToolCall/ToolResult、上下文压缩相关问题时必读
- M-009 [sop] edit 报 old_string not found + must match exactly:先重读再精确构造含 whitespace 与缩进—非唯一匹配勿设 replace_all — 处理 edit old_string not found 时必读:先 read 重读文件排版再精确构造—match exactly including whitespace;多处匹配勿用 replace_all 盲改，并识别换行缩进陷阱
- M-010 [sop] edit 报 old/new 相同是 no-op 拒绝而非失败 — 处理 edit 报 "old_string and new_string are identical — nothing to do" 时必读:这是 no-op 拒绝(提交的 new==old 无改动),停止重试,先 read 确认目标是否已达成,未达成则让 new_string 与 old_string 不同,勿用 bash 绕过。
- M-012 [fact] ID 同现于活动与归档时改用 terminal 专用操作 — 处理 goal/defect/req 报 is archived、尤其需要把已归档条目改为 fixed/wontfix 等终态时必读：先停止普通 update，确认条目已进入 terminal，再执行 defect fix_terminal id=<id> status=<fixed|wontfix> reason=<why>；不要对 archived 条目重试普通写操作。
- M-013 [fact] 处理 edit 替换失败/换行符问题：先 read 重读再改 — 处理 edit 替换失败/换行符问题时必读:先 read 重读再改
- M-014 [fact] HTML 静态文案必须登记进资源表,否则断言测试失败 — 处理 edit 报 old_string not found 时必读:先 read 重读文件排排版再精确构造——match exactly including whitespace;多处匹配勿用 replace_all 盲改，保留 [fp:edit|old_string not found...]指纹
- M-015 [fact] SSE 流内 context overflow 恢复须重建请求,OpenAI 错误分类须同查 type/code — 处理 bash git 拦截时必读:改用结构化工具显式 stage+核对 hash;保留所有 [fp:bash|...]指纹
- M-022 [sop] [git commit] bash 拦截时须 check staged add 再提交 — [fp:bash|> action: git commit 失败...] — 处理 git commit 被 bash 拦截须先 check add — [fp:bash|git commit 失败]必查前置 add/确认 staging 状态，否则强制 stage再 commit
- M-023 [fact] edit 报 cannot read 拒绝访问与 grep invalid regex 的判别及处理 — 处理 edit 报 cannot read/拒绝访问或同时出现 grep 正则解析错误时必读：先 read 重读目标；grep 遇未闭合正则立即停止并修正/改用固定字符串，随后再重试 edit，不要把正则语法错误当权限问题或反复重试。
- M-029 [fact] 所有 git mutation 在 bash 都被拦截,必须走结构化 git 工具 — 处理任何 Git 分支/索引变更(merge/restore/rebase/add/commit/reset)在 bash 报 "is blocked in bash: git mutations must use the structured git tool" 时必读:不要换别的 git 子命令重试,改用结构化 git 工具——显式 stage 指定文件、核对 staged_hash/diff,再用该 hash commit;快进合并走 git merge_ff。
- M-030 [fact] 鞭挞与 backlog 判定集中在引擎，前端只执行 autoAction — 处理自动运行鞭挞、backlog 或继续文案改动时必读：判定逻辑只改 harness/kanzei-tools 单源，桌面端转发，前端仅执行 autoAction；不要在前端重复判定或维护旧继续文案。
- M-041 [sop] autonomous 会话报 permission requires user approval 是档位限制,不是死路 — 处理 autonomous(自动推进)会话里 edit/bash/git/cargo/conventions_patch 被拒并报 "permission requires user approval" 时必读:这是权限档位而非工具故障——把该动作留给交互轮或先在 .kanzei/kanzei.toml 加白名单;不要反复重试、不要换等价命令绕道、也不要判定为死路而放弃整条任务。
- M-059 [sop] 记忆清理 SOP:归档不裸删,动全局先确认恢复源,清后三处一致 — 手动清理 .kanzei/memory 或 ~/.kanzei/memory 前必读:防数据永久丢失与索引悬空
- M-062 [fact] 环境约束:本机 WebView2 151 DevTools 端口从不绑定,e2e CDP 路线不可用 — 想走 e2e-smoke / connectOverCDP / WebView2 DevTools 端口路线前必读:当前机器已 9 轮实验证实不可用,不要重推
- M-070 [preference] 开发重心:需求优先 — 取活/排优先级时必读:当前项目该先做什么
- M-112 [fact] Git tests 跨轮复发：先核对前置条件、环境与批次字段一致性 — 处理 failures: git::tests 跨轮复发或关闭前出现手写批次与 Git 提交历史标记不一致时必读：先核对测试前置条件、环境一致性及完整批次字段，再更新错误批次并确认失败测试/结束标记后关闭；不要把重试成功或单个 exit code 当作根因。
- M-113 [sop] git commit staged 缺失 SOP — 处理 git commit 失败时必读:Changes not staged for commit 必须先执行 git add 同批文件;4+ 次复发并有修复经验,确认为环境契约问题
- M-202 [fact] bash timeout导致命令终止并改用test_record成功 — 处理 bash/timeout类任务时必读——识别可复用错误模式与一次性噪声的关键标准
- M-205 [fact] bash 命令超时被 kill 后的正确重试策略 — 何时遇到 bash 命令超时/被 kill — 先查历史 timeout 失败记录再重试
- M-227 [fact] bash 测试证据失败不激活 — bash测试证据不激活时必读：第3次+修复成功才建candidate,否则是单轮噪声
- M-247 [sop] bash guard全文件改写拦截SOP — 识别whole-file rewrites via shell bypass并使用edit/memory writer完成写入【新版】 — M-247 bash guard全文件改写拦截 — 遇[fp:bash|...]必read再update：whole-file rewrites→ident→用edit/memwriter
- M-258 [fact] bash/cargo失败模式：先核对结束标记与目标配置再定位根因 — 处理 bash/cargo 测试或编译输出异常、尤其 exit code=0 但测试结果与 stderr 混排或输出被截断时必读：先读取完整 `test result:` 结束行，按 `failed` 计数和失败测试名判定是否真的失败；本例 `1 passed; 0 failed` 应判为通过/输出截断，不因首个 exit code、`ok` 或 shell 重试继续排查。确认真实失败后，再核对 stderr 原文、Cargo target 与命令配置，read/grep 上下文后定向修复。
- M-268 [fact] bash 批量测试输出含多行时优先定位 pathspec 根因 — 处理 bash/test runner 批量测试输出混杂多行、看到 exit code: 1 与大量 running/ok 行时必读：先截取并核对完整 stderr/pathspec 根因行及测试结束标记，再决定修复；不要把首个 exit code、进度中的 ok 或截断片段当作 pathspec 根因，也不要在未定位根因前重跑。
- M-269 [fact] UI lint 全局变量探针需由生成脚本同步 — 处理 UI 运行时冒烟提示 smoke probe marker 与源码不同步时必读：先运行 `node scripts/gen-ui-lint-glob` 重新生成标记文件，再重跑冒烟检查；不要把已通过的运行时错误数误判为失败根因。
- M-270 [fact] refresh_derived 写 INDEX 前必须逐行核对 active 描述 — 处理 MemoryStore 刷新 INDEX.md、或发现 INDEX 与 active M-*.md 描述可能串号时必读：写入前逐行核对 id 与源文件 description；有任何不一致就失败并先修复源数据，禁止生成不一致索引。
- M-271 [sop] read 报 path not found 时先核对路径再读取 — 处理 read 报 path not found 且同类错误复发时必读：先核对项目 memory 根目录和目标文件实际存在性，再读取确认存在的候选路径；禁止对失效原路径盲目重试，并记录路径映射以避免再次误读。

(5 candidate 条待验证晋升)
