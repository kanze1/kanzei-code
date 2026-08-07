# Defects





































## D-051 bash「总是允许」仍按首个可执行词泛化,重定向和程序自身执行入口可绕过 [fixing] (high)
- 复现: 先对 `git status` 选择「总是允许」得到 `git *`,随后执行 `git status > .kanzei/project/requirements.md`;当前 SHELL_CHAINING 不含 `>`/`<`,命令直接命中 Allow 并可覆盖硬保护文档。`git -c alias.x=!calc x`、`python -c ...`、`pwsh -Command ...` 等也说明“同一首词”本身不等于同一权限范围。
- 根因: 首轮修复仅用 8 个字符 `; & | 换行 \` $ (` 做黑名单(config.rs:232-247;permission.rs:100-112),仍把任意无这些字符的命令泛化为 `首词 *`。Shell 与各 CLI 的执行语义无法用有限字符黑名单穷举。
- 已完成部分: 常见串联、管道与 `$()`/反引号命令替换会降级为 Ask,弹窗也已能展示记住规则。
- 未完成风险: 重定向可以绕过 write/edit 的硬 deny;解释器、包管理器、Git alias 等“单条命令”仍可承载任意执行。该问题属于权限模型缺陷,不应继续以补字符方式修补。
- 验收: 取消默认“首词通配”或改为按工具/子命令的结构化白名单;记住规则必须展示并让用户确认精确作用域;补重定向、Git alias、解释器 `-c/-Command` 回归测试。
- 优先级: P0
- refs: R-083
- 阶段: 1
- 不变量: 权限:授权范围精确可解释
- 证据等级: E2
- 进展: 桌面真实 UI E2 因无前端测试 harness 暂缓；本轮完成旧裸 bash 规则的只读识别与可见提示：KanzeiConfig::legacy_bash_rules 仅识别 action=bash 且非 command/workdir JSON 的旧规则，不改写配置；CLI 启动时 stderr 提示其将降级逐次询问，桌面 run_task 通过现有 kz:status 展示同样提示。新增配置检测回归；cargo test -p kanzei-harness -p kanzei -p kanzei-app 全部通过（29/3/9）。仍缺桌面真实 UI E2、正式迁移方案与并发写入证据。





































## D-061 OAuth 凭证无锁读改写且非原子覆盖,与官方 CLI 共享文件可致登录态失效 [open] (high)
- 复现: 两个 kanzei 进程(或 kanzei 与 Claude Code CLI)在令牌过期窗口内并发发起请求。
- 根因: kanzei-llm/src/auth/claude.rs:28-95、auth/codex.rs:20-101 的流程是 read_to_string → 判断过期 → POST 刷新 → `std::fs::write` 覆盖,无文件锁、无 tmp+rename 原子替换、无写前重读。这两个文件(~/.claude/.credentials.json、~/.codex/auth.json)同时被官方 CLI 读写。
- 影响: 双方各自用同一 refresh_token 刷新,而 OAuth 轮换 refresh token,后到者 invalid_grant,且先到者写入的新 token 可能被并发方以旧内容覆盖回去,登录态永久失效并殃及官方 CLI,需手动重新登录;truncate-then-write 中途崩溃会留下半截 JSON,下次解析直接报"请重新登录"。access token 约 1 小时一刷,窗口频繁。
- 验收: 刷新前后加文件锁,写入改为写临时文件再 rename 原子替换,写前重读校验;补并发刷新不互相覆盖的测试。
- 优先级: P1
- 阶段: 1
- 不变量: 配置与文档:多文件变更原子提交
- 证据等级: E2
- 阻塞: 涉及与 Claude Code/Codex 官方 CLI 共享 OAuth 凭证文件的并发写入、文件锁与原子替换，属于第三方集成/凭证高影响改动；依据 conventions.md 第 1 节需先提交方案并等待用户确认。解除条件：确认锁实现、跨进程协作与 Windows 原子替换策略。下一步：暂跳过，继续 D-059。





































## D-068 错误分类忽略 kind,限流可被误判为上下文超限触发破坏性压缩 [fixing] (medium)
- 复现: provider 在流内返回带 token 字样的限流/配额错误;或任何 429/529。
- 根因: kanzei-llm/src/error.rs:34-57 的 classify_provider 完全忽略 kind(流内 error 事件走此路径),只对 message 做宽泛子串匹配,词表含 "token limit"、"too many tokens"、"input_tokens" 这类会出现在配额文案中的模式,命中后 kind(如 rate_limit_error)被丢弃归为 ContextOverflow;runner 对 overflow 的响应是原地压缩消息历史再重试(runner.rs:268-284)。同时 LlmError 没有 RateLimited/Overloaded 变体,429/529 落为普通 Http 直接终止,retry-after 头被无视,client 重试只覆盖建流前的 connect/timeout(client.rs:142)。
- 影响: 误判时无谓压缩掉真实对话历史后重试,限流未解除则二次失败而历史已受损;正常限流没有退避重试,长跑 agent 一遇 TPM 峰值即整轮失败。
- 验收: classify_provider 先按 kind 判定再回落文本匹配;新增限流错误分类并按 retry-after 退避重试;补限流不触发压缩的回归测试。
- 优先级: P1
- refs: R-075
- 阶段: 1
- 不变量: Provider:错误分类不改变原始错误事实
- 证据等级: E2+E4
- 进展: 补齐真实本地 HTTP E2：client 测试启动 127.0.0.1 临时服务，连续返回两次 429 + Retry-After: 0，第三次返回真实 OpenAI SSE；断言收到 3 次 POST、触发 2 次重试且最终 TextDelta 为 ok。测试位置 crates/kanzei-llm/src/client.rs::rate_limit_http_retries_with_retry_after_then_returns_stream。cargo test -p kanzei-llm 29 项通过。HTTP E2 已覆盖，仍未覆盖流内 SSE rate_limit_error 的 provider 边界，保持 fixing。





































## D-104 最小支持窗口下顶栏折成三行,固定侧栏持续挤压核心对话区 [fixing] (medium)
- 复现: 按 tauri.conf.json 声明的最小窗口 800x500 打开桌面端。静态浏览器验收在 1024px 时 topbar 已为两行(约 69px 高),800px 时为三行(约 101px 高);活动栏 48px + 侧栏默认 280px 后主区仅 472px。
- 根因: #topbar 使用 `flex-wrap:wrap`,把进程、鞭挞、上下文、搜索、模型、模式等低高频控件全部常驻;侧栏只支持 220-460px 调宽,没有折叠/断点策略。D-029 只避免了竖排与横向溢出,没有解决信息层级和主区保底宽度。
- 影响: 小屏或分屏时消息阅读高度被顶栏吞噬,输入区与对话区变窄;控件顺序随换行漂移,形成明显的寻找成本。
- 验收: 800/1024/1280 三档视觉回归;顶栏在支持宽度内保持单行,低频动作进入明确的溢出菜单;侧栏可一键折叠且主对话区有最小可用宽度。
- 优先级: P1
- refs: D-029 R-089
- 阶段: 3
- 不变量: 界面状态:800/1024/1280 三档可用
- 证据等级: E3























- 进展: 本轮最小步骤已完成但条目保持 fixing：index.html:144-145 新增真实 sidebar-toggle 调用方；main.js:211-226 以 localStorage 持久化折叠状态并同步 aria-expanded/title；style.css:52-55 提供 collapsed 布局，折叠后主区释放侧栏宽度。验证：node --check crates/kanzei-app/ui/main.js、git diff --check、cargo test --workspace 全部通过。缺口：顶栏低频动作溢出菜单、800/1024/1280 真实视觉回归尚未完成。














## D-105 主导航与多类可点击容器没有键盘/可访问语义 [fixing] (medium)
- 复现: 只用 Tab/Enter 操作桌面端。activity-item、project-item、workspace-card、doc-row 等用 div + click 实现,没有统一 role/tabindex/键盘处理;自动放行/鞭挞的真实 checkbox 被 `display:none`;大量图标按钮的可访问名称只剩 `＋/↗/✎/🗑`。
- 根因: 交互由 3200 行原生 JS 零散绑定,只对 sidebar section title 补了 role/tabindex/aria-expanded,没有组件级可访问性约束。浏览器 accessibility snapshot 中活动栏项表现为 generic,图标按钮名称是符号本身。
- 影响: 键盘用户无法完成项目/视图/文档切换,屏幕阅读器无法理解按钮用途;R-040 的少量全局快捷键不能替代完整焦点顺序。
- 验收: 所有可点击对象使用原生 button/a/input 或完整 role/tabindex/键盘语义;图标按钮有稳定 aria-label;焦点可见;仅键盘可完成核心路径并形成自动化冒烟记录。
- 优先级: P1
- refs: R-040 R-091
- 阶段: 3
- 不变量: 界面状态:仅键盘可完成核心流程
- 证据等级: E3

























- 进展: 继续拆分完成第十一步：main.js:445-456 reasoning-head 改为原生 button，补 aria-label/aria-expanded；451-456 展开时同步状态，仅多段正文可展开；style.css:554-555 重置按钮样式并增加 focus-visible。既有思考摘要与正文折叠逻辑保留。验证：node --check、node scripts/ui-a11y-smoke.mjs、git diff --check、cargo test --workspace 全部通过。D-105 仍 fixing，活动面板其他动态标题及真实浏览器键盘冒烟仍待完成。












## D-106 错误与长结果普遍依赖 2.6 秒 toast,用户无法追溯、复制或恢复 [open] (medium)
- 复现: 触发项目初始化失败、设置保存失败、权限规则删除失败、工作树操作结果等;多数路径只调用 toast(String(error/result)),2.6 秒后消失。长文本被塞入同一浮层,body 又默认 user-select:none。
- 根因: toast 同时承担轻提示、错误报告、长结果查看三种职责,没有按严重度/可操作性分流;部分路径虽写 log,但并非统一契约,也没有“查看详情/重试”入口。
- 影响: 用户看不清错误原因、不能复制给开发者,失败后不知道状态是否改变;D-096 只是该设计问题在 worktree diff 上的一个确定性表现。
- 验收: toast 只承载短暂成功确认;错误与长结果进入可持久查看/复制的通知或详情面板,包含操作名、结果、时间和可用的重试/打开入口;状态改变类操作必须能追溯最终态。
- 优先级: P1
- refs: D-096 R-090
- 阶段: 3
- 不变量: 操作反馈:失败反馈持久、可复制、有恢复入口
- 证据等级: E3





































## D-108 英文模式只翻译少量静态节点,动态状态与操作反馈长期中英混杂 [open] (medium)
- 复现: 设置语言为 English,创建/切换项目、运行任务、打开文档/设置并触发 toast;静态导航的一部分变英文,动态生成的状态、日志、错误、按钮和 300 余处中文字符串仍保持中文。再切回中文还会触发 D-092 的属性不可逆问题。
- 根因: applyLanguage 只遍历当前 DOM 文本节点和少量 title/placeholder,I18N_EN 仅覆盖有限字典;后续 JS 动态生成的文本不会经过翻译函数,也没有以 key 为中心的统一文案层。
- 影响: 英文模式无法作为完整产品能力使用,错误与权限等高风险信息尤其容易出现语义断层;R-069 原验收“所有产品/功能文案”未满足却被归档 done。
- 验收: 所有用户可见文案由稳定 key 生成,动态内容与属性同源且可逆;中英文分别跑页面/操作快照,不得出现非用户数据导致的混合语言;补缺失 key 检查。
- 优先级: P2
- refs: D-092 R-069
- 阶段: 3
- 不变量: 操作反馈:文案进入统一 i18n 资源
- 证据等级: E3





































## D-109 对话 Markdown 不支持列表、表格与链接,Agent 核心输出退化为纯文本 [open] (medium)
- 复现: 让 agent 输出有序/无序列表、Markdown 表格和 `[label](url)`;renderMarkdown 只转换代码围栏、行内码、加粗和标题(ui/main.js:292-310),列表与表格没有结构,链接不可点击。
- 根因: 自研 markdown-lite 覆盖面与 coding agent 的真实输出形态不匹配,也没有测试定义支持子集。
- 影响: 计划、缺陷对比、测试矩阵和来源链接难以扫读,直接损伤“看输出”这一最高频路径;长回复缺少语义导航。
- 验收: 明确安全 Markdown 子集并支持列表、表格、链接、代码语言标识;外链有清晰安全行为;渲染必须先安全处理并有 XSS 回归测试。
- 优先级: P1
- refs: R-090
- 阶段: 3
- 不变量: 操作反馈:安全 Markdown 渲染并通过 XSS 用例
- 证据等级: E3





































## D-110 todo 与活动两个右栏可同时占宽,最小窗口会把主对话区压到近乎不可用 [open] (medium)
- 复现: 打开活动面板,再让 todowrite 显示当前计划;todo-panel 与 bg-panel 均为独立 300px 固定右栏且可同时显示。在 1280px 默认侧栏下主区只剩约 352px;800px 最小窗口下两右栏与左栏总宽已超过窗口。
- 根因: 两个面板没有互斥、tab 合并或窄屏 overlay 策略,宽度都以 flex-shrink:0 的侧栏语义参与主布局;设置的可调最小宽度仍为 240px。
- 影响: 运行越复杂、信息越多时主对话区越不可读,与“对话为主布局”目标相反;用户只能手动隐藏活动面板,计划面板由事件出现。
- 验收: todo/活动合并为一个可切换右栏或在同时出现时共享宽度;窄屏采用 overlay/抽屉且不压缩主区;800/1024/1280 覆盖单面板和双面板场景。
- 优先级: P1
- refs: R-037 R-089
- 阶段: 3
- 不变量: 界面状态:多面板不挤压主对话区
- 证据等级: E3





































## D-074 前端 5 类静默失败:附件丢失、设置页白屏、启动链中断、快记内容丢失、系统通知永不弹 [open] (medium)
- 复现: ①粘贴截图不写文字点发送→附件 chip 消失什么也没发生;②配置损坏时打开设置页→空表单无提示;③projects_get 失败→启动后半段全不执行,状态栏停在初始态;④快记写完提交失败→表单已销毁内容找不回;⑤等长任务完成→系统通知从不出现。
- 根因: ①send() 在 sendText 前就清空 attachments(ui/main.js:1436-1446),而 sendText 的 `if (!prompt) return`(1264-1266)静默吞掉——该函数注释自己写着"任何拒绝发送的理由都要说出来,绝不静默(D-004)";②loadSettings(2956-2957)首行 invoke 无 try/catch;③启动 IIFE(3156-3162)的 invoke 在 try 块外;④快记 submit 先 form.remove() 再 await invoke(2459-2473),失败时输入已随表单销毁(对照目标新建表单 2530-2544 失败保留);⑤全项目从未调用 Notification.requestPermission,permission 恒为 default,`=== "granted"` 条件恒 false(175-181)。
- 影响: 五处都表现为"点了没反应",用户无从判断是卡了还是失败了;其中 ① ④ 直接丢失用户输入。
- 验收: 五处分别补明确反馈——附件无文字时给出提示或允许纯附件发送;设置页与启动链 invoke 失败时可见报错;快记失败保留表单内容;首次需要通知时请求权限并在被拒时说明。
- 优先级: P2
- refs: D-004
- 进展: 已修 5 项中的 4 项:①只带附件发送时补默认描述,附件不再被静默吞掉;②loadSettings 增加错误处理,失败时提示并停止渲染空表单;③启动链改为逐步捕获,任一步失败只提示该步并继续后续步骤;④快记表单改为提交期间禁用而非销毁,失败保留内容可重试。⑤系统通知 requestPermission 仍未补(需在合适时机请求,留待后续)。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。
- 阶段: 3
- 不变量: 操作反馈:失败反馈在用户确认前持续存在
- 证据等级: E3





































## D-088 CLI 会话历史无限累积且无清理入口 [open] (medium)
- 复现: 在同一项目里正常使用若干次 kz run,上下文与耗时持续增长,最终撞上下文上限。
- 根因: 每次 kz run 无条件取最新 conversation.updated 全量作为 prior(kanzei/src/main.rs:145-153),runner 以 prior.to_vec() 开局(runner.rs:206),运行结束又把累积后的完整 messages 写回(277-281);usage(47-52)中不存在 reset/new/continue 任何选项。
- 影响: token 成本每次递增、缓存命中下降,最终报错;用户唯一自救手段是手删 state.db,顺带丢掉全部事件。
- 验收: 提供 `kz run --new`(或 kz reset)显式开新会话;补新会话不携带旧 prior 的验证。
- 优先级: P2
- 阶段: 3
- 不变量: 会话控制:上下文增长有清理入口
- 证据等级: E2





































## D-107 侧栏缩放手柄随滚动内容移动,长列表时无法持续调整宽度 [open] (low)
- 复现: 侧栏内容超过一屏后向下滚动,再尝试拖动右侧宽度手柄;handle 是 sidebar 的绝对定位子元素,而 sidebar 本身 `overflow-y:auto`,手柄随滚动内容离开可视区。
- 根因: setupResize 把 resize-handle 追加到滚动容器内部,没有把滚动层与固定边框/手柄层分离;同时 pointerdown 未 preventDefault,也没有键盘调整或双击重置。
- 影响: 需要缩放时手柄反而不可达,且拖动可能选中文本;R-074 声称“面板和容器支持缩放拖拽”的核心体验不稳定。
- 验收: 手柄固定在面板边界且不随内容滚动;支持明显 hover/focus、键盘微调和恢复默认宽度;在长侧栏、todo、activity 三类面板验证。
- 优先级: P2
- refs: R-074 R-089
- 阶段: 3
- 不变量: 界面状态:分栏控件在滚动后仍可达
- 证据等级: E3





































## D-075 上下文成分浮层是死功能,状态栏承诺的点击查看无法打开 [open] (low)
- 复现: 运行一轮后点状态栏 token 文字(title 写着"点击查看上下文成分"),浮层永不出现。
- 根因: renderContextDetail 只写 innerHTML 从不移除 hidden 类(ui/main.js:811-825),而 `.hidden { display:none !important }`(style.css:329);全项目无其他代码碰 #context-detail(index.html:339)。
- 影响: 承诺的上下文透明功能完全不可用——而"上下文透明"是 G-001 明确的产品方向。即使修好显示也没有关闭路径(无 blur/再点切换),需一并补。
- 优先级: P2
- 阶段: 3
- 不变量: 操作反馈:承诺的入口必须可达
- 证据等级: E3





































## D-077 独立文档页拖拽启用条件读错筛选状态,可提交残缺顺序 [open] (low)
- 复现: ①侧栏筛选全为 all 但独立页把状态筛成 doing → 拖拽仍可用,提交不完整的 ID 序;②独立缺陷页选 P1 优先级(status 仍 all)→ 同样可拖拽并提交残缺顺序;③反向:侧栏有筛选时独立页无筛选却拖不了。
- 根因: reqDragEnabled/docDragEnabled(ui/main.js:2039-2046)只读侧栏的 reqFilters,而独立页实际按 documentFilters 过滤(2399-2403),且缺陷分支完全没有判断 documentFilters.defect.priority。代码注释自己写明"order 必须覆盖全部条目,有筛选时禁止拖拽"。
- 影响: 排序是 agent 取活顺序的唯一依据,提交残缺顺序会直接改变后续工作队列。D-032/D-036 的修复未覆盖独立页的筛选来源。
- 优先级: P2
- refs: D-032 D-036
- 阶段: 3
- 不变量: 界面状态:排序提交必须覆盖全部条目
- 证据等级: E3





































## D-079 运行中发送按钮禁用但排队功能存在,鼠标用户不可达 [open] (low)
- 复现: 运行中在输入框打字并选好"插入 steer",发送按钮是灰的点不动;但按 Ctrl+Enter 却能成功排队。
- 根因: setRunning(true) 禁用发送按钮(ui/main.js:287),而 sendText 专门实现了运行中的 queue/steer 投递分支(1277-1296),交付方式下拉也常驻可选;键盘路径直接调 send() 完全绕过按钮禁用(1571-1573)。
- 影响: 按钮状态与实际能力矛盾,排队/steer 这个卖点功能对鼠标操作不可达、可发现性为零。
- 验收: 运行中保持发送按钮可用并按交付方式提示"将排队/插队",或明确禁用整条路径(含快捷键)保持一致。
- 优先级: P2
- 阶段: 3
- 不变量: 操作反馈:按钮状态与实际能力一致
- 证据等级: E3





































## D-089 子代理进度事件在任务完结时未 drain,工具块卡在进行中 [open] (low)
- 复现: 子代理在收尾阶段密集发事件时,UI 上偶见 trace 末尾缺块,工具显示"进行中"但任务已结束。
- 根因: kanzei-core/src/runner.rs:455-472 的 select 循环在 jobs.next() 返回 None 时直接 break,此刻 rx 中可能仍有子代理临完成前发出的 TaskProgress(含 ToolEnd trace,703-711)未被消费;select 分支无偏向,完成分支可能先于积压的进度事件被轮询到,break 后 rx 随作用域丢弃。
- 影响: 仅 UI 显示,不影响正确性。
- 验收: break 前用 try_recv 清空缓冲事件。
- 优先级: P3
- 阶段: 3
- 不变量: 界面状态:活动轨迹与后端事件一致
- 证据等级: E3





































## D-090 bgEntries/diffSummary 不随 DOM 修剪,长时间运行内存与定时器负载无界增长 [open] (low)
- 复现: 一晚上鞭挞连跑数千次工具调用且不切项目/进程。
- 根因: ui/main.js:490-492 的修剪只删 DOM(`list.firstElementChild.remove()`),bgEntries Map(452)与 diffSummary 仅在 bgClear()(切项目/进程)时清空;687-691 的每秒 interval 遍历全 Map,对已脱离 DOM 的 detached 节点持续更新。
- 影响: 内存缓慢增长,detached DOM 持有 diff 大块内容;自用长跑恰是主用例。
- 验收: 修剪 DOM 时同步删除对应 Map 条目;定时器只遍历在册条目。
- 优先级: P3
- 阶段: 3
- 不变量: 界面状态:长跑不产生无界增长
- 证据等级: E3





































## D-092 语言切回中文时 title/placeholder 属性停留在英文 [open] (low)
- 复现: 设置页语言 zh→en→zh,悬停活动栏图标,tooltip 仍是英文,需重启才恢复。
- 根因: ui/main.js:52-59 文本节点用 WeakMap 存了原文可逆,但属性没有原文存储;切回中文时属性值已是英文,`I18N_EN["Attach"]` 为 undefined 直接跳过。
- 影响: 语言切换不完全可逆。
- 优先级: P3
- 阶段: 3
- 不变量: 操作反馈:文案切换可逆
- 证据等级: E3





































## D-093 标题 🔔 提示只在 visibilitychange 复位,窗口可见时失焦回焦不清除 [open] (low)
- 复现: 双屏使用,kanzei 一直可见但焦点在别处,任务完成后回到窗口,标题仍是"🔔 运行完成 · kanzei"。
- 根因: ui/main.js:173-187 设置条件是 `!document.hasFocus() || document.hidden`(失焦即设),但复位只挂在 visibilitychange 上;窗口未被遮挡时失焦→回焦不产生该事件,缺一个 window focus 监听。
- 影响: 陈旧完成提示让人误以为又跑完一轮。
- 优先级: P3
- 阶段: 3
- 不变量: 操作反馈:提示状态不陈旧
- 证据等级: E3





































## D-094 运行中点开历史对话无守卫,流式输出错位嵌入历史视图 [open] (low)
- 复现: 运行中随手点一条历史对话回看,当前运行的输出继续追加在历史对话末尾。
- 根因: 历史行点击(ui/main.js:2685-2692)没有 running 守卫(对照"新对话"按钮 2738-2741 有);renderRecoveredMessages 重置 currentAssistant 并清空 messages(2605-2610),kz:text 继续到达时新建气泡追加在历史末尾;loadConversation 里的 bgClear 还会清掉正在跑的活动轨迹。
- 影响: 两段对话混在一起,正在进行的运行轨迹丢失。
- 验收: 运行中点击历史对话给出明确提示或改为只读预览。
- 优先级: P3
- 阶段: 3
- 不变量: 界面状态:历史只读与实时运行隔离
- 证据等级: E3





































## D-095 refs 跳转在独立文档页失效,特殊字符还会抛异常 [open] (low)
- 复现: 在独立文档页展开带 `refs: R-054` 的条目并点击该链接,页面无反应。
- 根因: ui/main.js:2188-2194 用全局 `document.querySelector([data-doc-id="..."])`,同一条目在侧栏与独立页各渲染一份且侧栏在前,而独立视图激活时侧栏副本被 `display:none` 隐藏 → scrollIntoView 对隐藏元素无效、高亮不可见;另 ref 值来自自由文本,含 `"` 或 `]` 时选择器语法错误直接抛未捕获异常。
- 影响: 关联跳转在最适合用它的页面上不工作。
- 验收: 在当前可见容器内查找目标;ref 值需转义后再构造选择器。
- 优先级: P3
- 阶段: 3
- 不变量: 界面状态:关联跳转在当前视图内生效
- 证据等级: E3





































## D-096 隔离工作树的差异查看只弹一次性 toast,无法阅读 [open] (low)
- 复现: 在隔离工作树里改了 20 个文件,点"差异"——几十个文件名塞进 2.6 秒即消失的小气泡,且文本不可选中复制。
- 根因: ui/main.js:1767-1769 直接 `toast(files.join("\n"))`;toast 存活 2600ms(131-138),body 设了 user-select:none 而 #toast 不在可选白名单(style.css:20-22)。
- 影响: 差异查看功能形同虚设,而项目里已有现成的 diff 查看器与文档查看器可复用。
- 验收: 改用应用内查看器展示文件列表与实际 diff 内容。
- 优先级: P2
- refs: R-050
- 阶段: 3
- 不变量: 操作反馈:长结果持久可见可复制
- 证据等级: E3





























## D-114 自举运行验证节奏低效:git 查询过密、全量测试时机不当、已知位置缺陷仍派子代理 [fixing] (low)
- 复现: 2026-08-07 完整落库轨迹:30 次终端调用中约 13 组 git status/diff/show 密集重复且常一次塞多条;文件仍处换行损坏时跑过全工作区测试;D-082 单文件已知函数缺陷启动子代理,28 次内部读查后因网络错误失败返回,主 agent 重查一遍。
- 根因: dev 提示词无验证节奏与子代理适用边界约束;runner/工具层对重复查询、无变化重测、已知位置探索无任何检测。
- 影响: 单轮约 14~18 次可避免的终端调用(占 47%~60%);重复输出稀释上下文,推高 token 成本与轮次时长。
- 验收: 提示词纪律落地(已完成);R-099 度量显示同类任务终端调用数与 edit 未命中率显著下降;若提示词不足以收敛,按 R-100 落 runner 层机械提醒。
- 优先级: P2
- refs: D-113 R-099 R-100
- 阶段: 1
- 不变量: 工具:每次调用都有信息增量
- 证据等级: E2
- 进展: a7892e1 已在 dev 提示词加入验证节奏纪律(git 检查按里程碑并合并查询、先定向测试后全量、无变化不重测)与子代理适用边界(已知文件+函数直接读);1d5e294 的 edit/bash 门禁已消除编辑连败与整文件重写两类根源。剩余闭环依赖 R-099 的度量数据,机械化提醒按 R-100 决策。
