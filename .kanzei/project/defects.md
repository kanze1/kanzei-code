# Defects

## D-049 快记按钮不应依赖展示展开按钮 [fixed] (medium)
- 原始描述: 快速记录需求和缺陷的按钮不应该依赖于展开按钮
- 复现: 检查快速记录和缺陷记录的UI，确认它们是否直接可用而无需先展开
- 修复: 快记表单改为挂载到对应分区的 section-title 内，并在标题中独占一行；折叠分区只隐藏标题之外的子节点，需求/缺陷快记不再依赖先展开分区。
- 验证: node --check crates/kanzei-app/ui/main.js；cargo test -p kanzei-app（7 项通过）；手工验收：折叠需求或缺陷分区后点击 ✎，表单仍可见、可输入、可取消/提交。

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

## D-065 通知 sequence 分配与插入非原子,INSERT OR IGNORE 吞掉冲突静默丢通知 [open] (medium)
- 复现: 同一会话/线程有两个并发通知源(如运行结束通知与状态通知同时落库)。
- 根因: kanzei-core/src/store.rs:192-199 的 next_notification_sequence(MAX+1 读取)与 173-190 的 append_notification 是两个独立公开方法,中间无事务包裹(调用方 kanzei-app/src/main.rs:2528-2540 先取后插);两个并发写入方对同一 thread_id 取到相同 sequence,而 INSERT OR IGNORE(178)会忽略任何约束冲突——既包括预期幂等的 event_id 主键,也包括 UNIQUE(thread_id, sequence)(502),第二条通知被静默丢弃,无错误无日志。
- 影响: 通知永久丢失且不可观测,移动端按 cursor 回放永远看不到。
- 验收: 在单个事务内完成 MAX+1 与插入;OR IGNORE 只用于 event_id 幂等重放,(thread_id, sequence) 冲突需报错或重算;补并发写入不丢通知的测试。
- 优先级: P1

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
