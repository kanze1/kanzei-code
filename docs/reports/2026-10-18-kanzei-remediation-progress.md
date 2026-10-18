# Kanzei 修复 验收与发布进度

本次交付目标：修复归档后详情残留，完成原生 Computer Use、搜索答案质量与 Luna 订阅搜索兼容，并正式发版。此前 27 项清单的逐项边界继续保留如下；未完成的扩展验收不标成已完成。起点为 8d479de1；开发分支 kanzei/reliability-browser-memory。源码、定向回归、软件操作、完整发布门禁分别记录，不能互相替代。

## 逐项状态

| 项目 | 当前状态 | 下一项完成证据 |
| --- | --- | --- |
| RUN-01 | 请求投影去重与按证据扩展的调查提示已实施；原生 8 项、真实 Luna 两组固定源码配对均保留正确结论 | 更长调查任务的读取量、上下文峰值、压缩次数和交付正确性对照，最终发布版复验 |
| RUN-02 | 每次变更工具持有租约，普通开发会话可并行调查；原生双会话 8 项检查通过 | 最终发布版复验及冲突用例完整门禁 |
| RUN-03 | 桌面 628 项单测通过，含输入启动失败/取消/排队的重启恢复，待状态操作验收 | 真实 UI 状态及故障恢复一致 |
| RUN-04 | 日期/时区按日刷新，身份/模型/构建先持久化再进上下文；原生请求已核对 | 报告与最终发布版本关联复验 |
| SEC-01 | 六个文件工具、缓存、Skill 和 Git diff 的读取范围及凭据门已接；目录链接反例先失败后通过，原生 Skill 联合 22 项通过 | 发布版联合验收 |
| SEC-02 | 页面/目录能力、独立来源已实现，真实 Edge 与原生内部面板均通过 | 最终 HEAD 完整门禁及发布版复验 |
| SEC-03 | 定向反例先失败后通过，待完整验收 | 子代理、并行及串行路径的整体回归 |
| SEC-04 | 独立 isolated 档位只装配 8 个观察/隔离工具；创建、设置、恢复和发送共用档位边界，无项目与陈旧 Dev 请求原生负向验证通过；真实 OS 19 项、原生软件 22 项通过 | 最终 HEAD/发布版复验；隔离需要 WSL2/bubblewrap |
| SEC-05 | 启动配置已纳入精确授权策略，待操作验收 | 创建、改写及宽授权场景 |
| SEC-06 | 网页、浏览器、文件和进程输出采用结构化来源边界；恶意网页驱动的真实原生负向操作通过 | 最终 HEAD/发布版复验；该结果不等于模型免疫提示注入 |
| SEC-07 | 默认拒绝的只读工具声明和动作校验；Git status/diff/log 改为进程内观察，仓库程序反例先失败后通过，Git 40 项及起点 6 项通过 | 最终完整门禁及发布版联合复验 |
| WEB-01 | 14 项搜索回归通过，含无跨服务 HTTP 断言 | 软件内搜索与最终完整门禁 |
| WEB-02 | 订阅直接搜索与 Responses API 分开适配；真实 Luna 四对象单次查询均返回 codex_search 结果，无回退；Responses 的 max_tool_calls 400 原观察保留 | 最终 HEAD/发布版复验；订阅私有接口变化明确失败 |
| WEB-03 | 采集记录绑定任务/会话/运行与原始及最终 URL，工件 SHA-256 在引用时复核；网页系列 48 项通过 | 动态正文与软件内端到端验收 |
| WEB-04 | HTML 登录/布局异常零结果反例先失败后通过；类型化健康状态与尝试路径已实现，22 项回归通过 | 软件内真实查询及最终发布版复验 |
| WEB-05 | browser source 已接两后端；内部面板与 Edge 原生表单/正文联合 36 项、现有 Edge/iframe 原生 11 项通过，工件和会话/run 身份已核对 | 最终研究 finding 联合操作及发布版复验 |
| WEB-06 | 四对象检索与四份正文 SHA-256 核对；真实 Luna 两组固定证据问题通过，七个已回答属性的引用正确，一个缺失对象明确披露；判分器 12 项自检 | 发布版复验；近期新闻和长程自主调查不在该固定集结论范围 |
| WEB-07 | 地区码、排除域及子域、未知参数与预算已实现，实际 HTTP 参数回归通过；搜索系列 26 项通过 | 最终软件内查询及发布版复验 |
| BRO-01 | select/check/upload/validate 与受控字段填写已接两端；原生联合 25 项通过，含内部面板和实际 Edge 完整填写与提交 | 最终 HEAD 门禁及发布版复验 |
| BRO-02 | 用户授权的本机 CDP 连接、标签页/iframe 身份与断开已实现；辅助层 16 项与原生现有 Edge 11 项通过 | 最终发布版复验；现有 Edge 须已提供用户授权的 CDP 端点 |
| BRO-03 | 会话持久化后端及面板代次；明确选择 pane/headless、前后台切换保持原后端、批准期间同 URL 重建拒绝旧动作；原生并发联合 30 项通过 | 最终 HEAD 与发布版复验 |
| BRO-04 | 原生窗口能力与 UIA 工具已接入；系统 19 项、软件合同 17 项、旧 UIA 密码边界 2 项通过，真实 Luna 实际提交表单成功；TCP EOF 取消单测通过 | 最终 HEAD/发布版复验；需要 Windows 和 PowerShell 7 |
| MEM-01 | 候选机会、召回、注入、读取和明确采用/纠正反馈已区分，原生反馈 8 项通过 | 类型化晋升和恢复证据串联 |
| MEM-02 | 版本化机会、冷却降级、隔离归档和恢复已实现，原生联合 19 项通过 | 最终发布版复验 |
| MEM-03 | 做梦按钮、冻结片段、引用校验及受限候选整理已实现，原生联合 19 项通过 | 最终 HEAD 与发布版联合复验 |
| MEM-04 | 对话来源、候选推断、凭据、冲突拒收、版本复核与撤销已实现，原生采用/撤销通过 | 完整版本编辑与恢复边界，最终发布版复验 |
| MEM-05 | 普通与做梦共用项目作业锁、收据、预算和续跑，冻结片段原子准入与消费去重已实现 | 最终完整门禁及发布版复验 |

## 已取得的原问题与回归证据

- 归档详情残留：原生旧版本复现已归档但旧详情仍开着；修复后真实桌面 23 项通过，文档资源事件回归 10 项通过。手动完成/放弃/不修复均回到所属列表，非终态刷新正确保留详情，显式历史只读。
- 原生窗口、订阅搜索协议、搜索论据与引用的详细证据及范围见 [原生窗口与搜索交付验收](2026-10-18-desktop-search-acceptance.md)。

- 成功收尾反例：durable_outcome_commits_before_success_publication 原实现断言失败，LiveRun.last_outcome 为 None，预期 completed。修复后 durable_outcome 系列 10 项通过。
- 步数反例：真实 runner 的 mock provider 在最后一步返回工具调用，原实现生成 forbidden-side-effect 文件。修复后 exhausted_budget_rejects_provider_tool_calls_before_side_effects 通过，调用在子代理派发和普通工具执行之前被拒绝。
- UI 状态：ui-state-admission-smoke 16 项通过；parallel-lines-regression 和已改 UI 文件 ESLint 通过。这些是 UI 冒烟，尚不能证明安装版桌面操作验收。
- 协作状态：桌面单测 628 项通过，含 10 项协作模块用例，覆盖真实协调器排队、待批准、启动失败、取消和重启后最新输入事实；终态提交在生命周期锁内完成。
- 搜索路由：websearch::tests 14 项通过，使用本机 HTTP 服务确认显式 DDG 不连接 Codex、显式 Codex 失败不连接 DDG、auto 只按 primary 判定。
- 敏感资源：通配 Allow 读取 .env 的反例先失败；统一策略后 permission:: 系列 38 项通过。六个文件工具、递归搜索、精确授权读取的联合用例通过；真实执行器确认 AutoAllow 与通配会话规则不能放行凭据，精确人工授权可到达串行与并行的实际 I/O。
- 浏览器与静态服务：preview_server::tests 19 项通过，含伪造 Host/跨授权路由拒绝。普通 sibling 数据反例先得到 200，修复后得到 403，显式目录能力仍可读取其数据且不扩大旧页面能力。真实 Edge 两条联合测试通过：ES module 正常、私有文本/凭据/JSON 均为 403、外传请求数为 0；两个页面的 localStorage、Cookie 和跨来源读取隔离。
- 原生 Kanzei 软件操作：ui-browser-security-native-smoke 8 项通过（真实 Rust 静态服务与 WebView2，隔离配置及临时项目）。模块加载、私有数据 403、独立页面存储、返回页面恢复、跨来源读取被拒绝、应用协议拒绝；预览页实际调用 app_info 得到 `app_info not allowed. Plugin not found`。证据 output/playwright/browser-security/native/acceptance.json 及 native-preview.png；该次构建为 dev，正式发布版仍需复验。
- 原生并发与运行事实：ui-run-concurrency-native-smoke 8 项通过。A 的实际模型 HTTP 响应保持未完成时，B 读取真实来源文件、进入待批准、获得一次批准、写出结果并完成；A 随后独立完成，重载界面仍为两个 completed。实际发往 provider 的事实中，本地日期、时区偏移、provider/model、input_id 与两条独立 run_id 正确。证据 output/playwright/run-concurrency/native/acceptance.json 与 native-concurrency.png。
- 变更租约：思考和只读工具不持写权，主/子代理、串行/并行入口复用 execute_coordinated；代码树锁和逻辑资源锁使用明确的不同并发类型。两个实际写区间没有重叠，期间读取真实文件可完成；取消等待立即清理队列，零写入，取得/释放审计成对。变更后的 core 428、harness 217、桌面端 629 项全量通过，跨树 merge_ff 4 项通过。
- 运行版本边界：原生临时项目位于另一 Git 仓库之下时，不再借用上级仓库的 HEAD 和改动列表。嵌套非仓库反例回归及协作 10 项通过，版本与改动状态明确保持未知。
- 记忆反馈：Core 将 candidate_ids 与 retrieved_ids 分开计算机会和召回，注入不等于采用；明确的用户采用、纠正、撤销以 memory.feedback 事实写入 state.db，并绑定原始内容版本。memory_observations 6 项与内容版本/重新打开用例通过，core 全量 429 项通过。真实桌面 ui-memory-feedback-native-smoke 8 项通过，含按钮操作、重复点击不重复计数、跨项目同编号隔离、旧版本拒绝、撤销及重载恢复；证据 output/playwright/memory-feedback/native/acceptance.json 和 native-memory-feedback.png。该结果不表示做梦与自动清退已经完成。
- 上下文组装：context_assembly 10 项通过，保留原有完整来源、刷新失败和容量边界回归。
- 记忆整理作业：复用 inbox.checkpoint.json，自动、手动和 CLI 共享每项目 OS 作业锁。原生 ui-memory-job-native-smoke 12 项通过，覆盖实际 fast 模型路由、进度、另一次入口返回 busy、旧作业 ID 拒绝、停止保留草稿、刷新保留状态、同一后台进程继续存活、续跑完成、用量以及跨后续作业的旧请求重放。停止状态先写检查点和请求收据，再对查询公开；本机 HTTP 模型报告的用量为输入 200、输出 40 tokens。证据 output/playwright/memory-job/native/acceptance.json 和 native-memory-job.png，属于 dev 构建操作验收。
- 实际操作发现并修复两条原因：后台曾把任意命令返回的 stopped=true 当作退出后台；现限定为 shutdown/retire_if_idle。Tauri 2.11.5 的 ipc-protocol.js 在刷新导致 fetch 失败时会通过 postMessage 重发；实际观察到两次 HTTP IPC 对应三次后台入口。因此整理请求使用 UI 生成的唯一标识与 state.db 的 memory.job 收据，同一请求跨刷新、跨后续作业均不重执行。原生验收脚本原先错误地把异步查询传入 waitForFunction，已改为等待实际界面终态后读取后台结果。
- 草稿身份：新草稿有固定 ID，正文以 JSON 字符串保留，引用网页的 note 标题不能伪造队列记录。删除只作用于唯一身份或唯一摘要，正文不参与定位；本批次能力拒绝删除后来追加的条目。完成量根据本批次 ID 计算，并发追加不抵消已完成量；相似摘要和并发追加的负向测试通过。
- 停止后的自动调度：a_stopped_job_is_not_restarted_by_automatic_consolidation 反例在修复前确实重新受理作业并返回 Failed，预期保留 Cancelled。现自动入口对已取消、失败、未完成或中断的队列等待主动续跑，并把自动检查结果写成可核对事实。记忆作业定向 6 项通过；原生套件扩展为 14 项，在停止后又实际运行一次主模型对话，确认主运行正常 completed、自动整理记录 cancelled、整理模型请求数没有增加，之后用户按钮仍可正常续跑。失败原因也随终态一起持久化，可在刷新和重放后读取。
- 输入归属：跨会话复用 input_id 的反例先失败后通过，准入不能再借用另一会话的输入；重复的同会话输入仍保留第一次的原文和来源。original_session_inputs 按会话读取未展开附件的原始输入及真实准入来源，未标注来源不等于人类输入；输入模块 17 项、Core 432 项全量通过。
- 凭据与来源底座：新记忆新增/更新、草稿投递和拒绝审计均经过凭据策略，JSON 按字段处理，保留正常偏好与空配置模板，脱敏标记可重复处理。修复了把已脱敏 JSON 的结束符误当密钥值的反例；Memory 218 项全量通过。来源工件只写所属项目的 sources 目录，同名不同内容拒绝覆盖，读取核对 SHA-256 和会话边界。
- 做梦来源层：读取当前会话段的原始事实而非模型压缩摘要，原始输入不夹带展开附件；助手文本、未知来源和旧上下文保留区别，推理/签名、图片数据、工具参数和原始工具输出不复制。按真实序列化大小无损切分长中文、换行、转义与 emoji，引用只能来自本片段的真实原文，角色及字节位置由引擎记录，不能由模型声明。5 项定向测试通过，含反篡改、跨项目、伪造引用和角色拒绝；这些是来源组件证据，按钮、模型候选整理与完整软件操作尚未完成。
- 记忆生命周期：成功的真实检索记录当前内容版本的可用机会，每个 run 只记一次，编辑后的内容不继承旧版本的统计。默认策略为至少 30 次机会、90 天没有使用后 active→shadow，再经过 30 天隔离后可恢复归档。用户明确创建的记忆、偏好、习惯、标记保护的约束，以及全局记忆均不根据单个项目的零命中自动清退。明确采用可晋升事实候选，未经工具恢复验证的 SOP 不因采用次数而被当作已验证。Core 430、Memory 212 项全量通过，含固定时间、机会不足、保护条目、隔离期、归档恢复和版本隔离用例；该证据尚不等同于桌面自动维护的完整验收。
- 三个受影响库的首轮全量结果：core 425 通过，harness 217 通过，tools 883 通过、1 失败、3 ignored。唯一失败是旧浏览器默认文件放行断言，已按新的结构化文件授权契约修改且 base::tests 2 项复跑通过；这不是最终完整门禁结果。
- UI runtime 使用验证清单要求的 VM Modules 模式通过：128 个模块、3392 次 invoke、11 个视图切换、0 运行时错误。记忆作业阶段桌面端 631 项全量通过，Core/Memory/Tools/App 四个 crate 的 all-targets Clippy（-D warnings）通过。旧界面仅凭文件年龄三天、累计命中为零就标记“长期零命中”的判断已移除，只展示后台已确认的冷却降级；陈旧但缺少观测的条目仍可见并允许人工处理。

- 草稿引用边界：网页引文中的 memory_stale 确实触发了旧版确定性归档，反例修复前失败；已删除文本命令执行与猜测式销账，自动批次仅保留检索、统计、创建候选和移除本批草稿。Memory 219 项全量、整理作业 6 项通过。每个作业固定开始时的草稿集合，全部批次共享可配置步骤预算（默认 64），预算不足不启动模型且保留草稿；未完成和中断在界面显示可续跑，不显示成功。

- 做梦模型与界面：当前对话的原始事实、准入来源和段边界在同一个 SQLite 读快照中冻结，完整来源工件按 SHA-256 保存。固定片段一次性准入，消费收据与队列在同一文件事务中写入，重复做梦不会重新加入已消费片段。模型只提出带真实引用的项目候选；来源角色和字节位置由引擎计算，伪造引用、助手建议冒充用户偏好、凭据、已有记忆冲突均拒绝。候选需要当前内容版本的明确复核，统一写入门禁拒绝绕过，修改正文会重新进入候选；采用和撤销均保留原文。原生 ui-memory-dream-native-smoke 首轮 15 项通过，含长对话 6 片、停止、刷新、续跑、重复运行、实际采用与撤销；扩展后的联合验收 19 项通过，实际来源链接在文件查看器打开所属项目的脱敏工件；临时数据库中受控历史观测触发普通对话收尾的自动降级和隔离后归档，明确偏好与用户约束仍启用，原生恢复按钮保留历史正文。证据 output/playwright/memory-dream/native/acceptance.json、native-memory-dream.png、native-memory-lifecycle.png，属于 dev 构建。
- 当前回归：Core 432、Memory 222、App 631 项全量通过，做梦 6 项定向与生命周期 4 项通过，四 crate all-targets Clippy 通过；Markdown 导航契约 14 项通过。App 全套曾两次卡在记忆管理对话，单独运行 0.17 秒通过，临时阶段跟踪确认四个 HTTP 请求、收尾和陈旧版本拒绝均完成后，全套 631 项在 18.03 秒通过；跟踪代码已移除，最终门禁仍须重新核对，不能据此宣称卡顿根因已经解决。

- 搜索健康状态：登录/布局变化页被误报成功零结果的反例已复现并修复。DDG 使用已有锁定版本 dom_query 解析 HTML，结果与摘要在同一卡片内关联，兼容单引号和属性次序。明确区分实际空结果、筛选后为空、部分响应、验证码、布局变化、HTTP/授权/限流和网络失败；每个查询保留后端、是否真实派发、状态及错误原因，只有 auto 可记录并切换后端。删除 alpha/search 内部重试和隐藏冷却，单次尝试只发送一次请求；错误不再把所有调查引向 arXiv。websearch 22 项通过，尚不能代替实际软件与发布版验收。

- 搜索约束：过长中文查询被静默截断的反例先失败后通过，现联网前明确拒绝，max_results=0/越界也不隐式改写。queries 支持 DuckDuckGo 官方地区码、exclude_domains（含子域），单域 site 约束和所有返回 URL 的白/黑名单复核；多域在返回候选侧筛选，不假设后端存在未文档化的并集语法。地区参数确实以 kl、时效以 df 进入实际本机 HTTP 请求，排除域以 -site 提示并本地复核。未验证地区支持的显式 alpha 适配器不发送请求；只有 auto 可记录原因再选 DDG。查询与过滤参数进入结果记录，未知字段拒绝，来源 URL 不接受用户凭据段。26 项定向与 Clippy 通过。地区和搜索语法按 2026-10-18 官方说明核对：[参数](https://duckduckgo.com/duckduckgo-help-pages/settings/params)、[语法](https://duckduckgo.com/duckduckgo-help-pages/results/syntax)。

## 发布验收

网页来源存证：旧 fetched_at 时间戳在没有工件时仍被当作已读来源的反例已先失败后通过。采集事实写入所属会话的 state.db，保存请求 URL、重定向后的 URL、任务/主题/运行、时间、HTTP 状态、原始响应和正文各自的路径/大小/SHA-256；不再依赖会被淘汰的 URL 索引时间戳。finding 保存具体采集记录及来源版本，跨主题/会话或篡改工件拒绝。下载 PDF 本身不算已读，实际 read 工具读取当前版本后才有独立检查事实；这一事实不代表已通读所有页或验证结论。缓存篡改明确报错并撤销该缓存，不暗中联网重试。web 系列 48 项、research_loop 4 项和 tools all-targets Clippy 通过，动态浏览器正文验证见后文。

动态正文：browser 的 source 动作保存当前渲染 DOM 与可见正文，共用采集与研究门禁，支持活动 topic/task_id；浏览器 DOM 观察的 HTTP 状态明确为 unknown。空壳、仍在加载的文档、挑战页和无正文的登录表单保留未核验记录，不能用于 finding；过大页面明确拒收而不冒充完整正文。挑战页误收反例已先失败后通过，web 系列 49 项、真实 Edge 浏览器系列 20 项通过。内部面板的后台传输原先丢失 session/run/browser owner 和结构化结果，现保留会话身份及完整 ToolOutput（终态、错误码、图片、工件）；相应传输回归通过，五 crate all-targets Clippy 通过。记忆来源链接及取消编辑复用捕获时的项目归属，实际 Markdown 导航 16 项通过。上述不替代最终原生窗口和发布版联合验收。

真实表单操作：新增 select/check/upload/validate；type 对文本、日期、空值和 React 受控字段采用同一套原生属性/事件语义，两浏览器后端共享实现。DOM 显示字段类型、名称、必填/禁用、勾选和 option value。上传先检查页面与每个文件的权限，用 ScopedFile 打开并读取固定字节，总量限制 16 MiB；宽权限不能绕过敏感文件精确授权，权限和大小负向回归 2 项通过。后台向桌面传输完整 Ruleset，保留普通规则、硬拒绝和精确授权；密码填写结果脱敏。

原生 ui-browser-forms-native-smoke 联合 25 项通过：实际 Kanzei runner 和 WebView2 执行 27 次动作完成 React 18.3.1 表单，观察 HTML/应用校验、清空、更正、下拉、多选、勾选/取消、radio、日期、实际附件字节、FormData 提交回显与双工件哈希。前台 provider 保持等待时，另一实际会话在独立 Edge 上完成同样 27 次动作；前台表单保留原值，随后在面板隐藏时继续从原 WebView2 采集正文，会话/run 归属正确。隐藏被误当身份变化的反例先失败后通过，已删除按隐藏时间回退其他浏览器的等待/迁移逻辑，截图仍单独检查可见性；预览 41 项通过。首轮测试模拟器曾把截图追加的图片消息误作新任务，现根据原始任务标记定位对话而保留图片，不跳过视觉路径。证据 output/playwright/browser-forms/native/acceptance.json 和 native-form.png，属于 dev 构建；用户现有已登录 Edge 的独立验收见后文。

现有浏览器接管：browser 新增 connect/pages/frames/disconnect。只连接用户已授权且已启用 CDP 的本机 HTTP 端点，返回会话专属 connection_id 与标签页身份；每次页面操作明确带 page_id/expected_url，iframe 再带 frame_id/expected_frame_url。两份 URL 都进入权限资源，执行前核对同一目标，eval 在目标文档内再核对；旧 ID 不重绑到同 URL 新标签页。用户的原有默认 context 不被替换，连接采用 noDefaults；断开和空闲回收只关闭传输，不关闭原浏览器。CDP 握手的 HTTP 与 WebSocket 不跟随跨端点重定向，发现响应限制 64 KiB，应用内部页面不可接管；现有连接需要 Node.js 22+ 与 Playwright 1.62 的公开自定义传输接口。[接口依据](https://playwright.dev/docs/api/class-browsertype#browser-type-connect-over-cdp)。

browser-connection-smoke 辅助层 16 项通过，含连接前已有登录、标签页选择、其他会话拒绝、旧 URL 拒绝、同 URL 重建后旧 ID 拒绝、iframe 独立操作及导航失效、弹窗新 ID、页面级控制台、跨端点 WebSocket 重定向零请求、断开/关闭 helper 后浏览器与登录态保留。原生 ui-browser-connection-native-smoke 首轮 9 项通过，扩展跨来源 iframe 后 11 项通过：实际 Kanzei 权限与 Rust runner 执行 32 次操作，接管软件启动前已有登录态的 Edge，完成 React 填表、真实文件字节、提交、更正、跨来源 frame 操作及正文存证，断开后原标签页、其他标签页及登录仍保留。证据 output/playwright/browser-connection/native/acceptance.json、native-connection.png；属于 dev 构建。连接相关两项 Rust 回归与浏览器系列合计 24 项通过，tools/app all-targets Clippy 通过。该能力不自动启用调试端口，不声称能直接接管未暴露调试连接的普通 Edge 窗口。

运行事实新增 run.facts，先落库后进上下文，与现有 run.baseline 起点版本共用 run_id；固定 UTC+08:00 的跨日及日内缓存稳定性用例通过。审计报告和 prior-art 中错误的 10 月 21 日已纠正为运行环境的 10 月 18 日，保留原文件名与更正说明。只读快照错误装配测试，以及 broad Allow、AutoAllow、会话通配授权下的串行/并行零写入测试均通过。

尚未发布。发布前需全部逐项证据成立，最终提交通过 verify.ps1 -Full，验证清单绑定最终 HEAD，无失败或跳过；然后执行正式打包发布，独立核对安装包大小、SHA-256、Range 和软件版本。软件操作测试须包含真实 Kanzei 窗口、内部浏览器、Edge、做梦与记忆管理；本机安装与重启后的结果另行记录。

浏览器后端归属：backend=pane/headless 成为明确的目标选择，省略时读取所属会话的 browser.target_selected 事实，不再随当前选中对话或面板可见性重新选择。事实按项目、会话和子任务隔离；内部面板保存页面代次与归属代次，权限计划还冻结 URL 和导航代次。桌面服务和嵌入式运行共用同一桥接计划，删除重复路由。已有浏览器依旧由 connection_id/page_id 明确定位。前台明确选择独立 Edge 的原版本反例先因未知 backend 参数失败，再通过；单纯切换会话后的导航在旧版本测试也通过，未把这个推断路径写成已复现缺陷。

ui-browser-forms-native-smoke 扩展至 30 项：保留两会话完整 React 填表与隐藏正文采集，新增前台独立 Edge、切换会话后后端不变、明确切回内部面板，以及真实待批准期间关闭/重建同 URL 页面后拒绝原填写，替代页面零字段变更。可见截图测试明确等待批准通知消失，不能用被遮挡的隐藏画面证明截图可用；重建页面则等待真实 CDP 导航事件，而非假定 IPC 返回即等于页面已加载。证据仍为 output/playwright/browser-forms/native/acceptance.json（开发构建）。

原生搜索执行契约：删除生产路径的私有 /alpha/search，改用配置 primary 的标准 Responses web_search；不再从 fast/compact/default 借用另一条订阅路由，也不按 Luna 型号拼写放行。保留原 codex 配置值作为 responses 的兼容别名。通过公共 build_route 取得实际协议、认证与端点约束，不跟随 HTTP 重定向；发送搜索问题和过滤参数，不发送完整对话。native_search 与禁用档位配置在普通和 research 工具入口分别生效；research 仍检查活动 topic/task_id 和 prior-art 预算。主循环直接附加 provider 托管声明明确不支持，在任何 provider I/O、询问和运行事件之前拒绝；注册的 websearch 工具提供受权限管理的原生执行入口。

每个原生查询发送 max_tool_calls=1，要求实际 completed web_search_call 与请求的 action.sources 来源列表；模型正文或自写引用不能充当检索证据。过滤和去重在客户端再次执行，不将供应商生成的总结冒充网页摘要。来源 URL、响应/搜索身份、实际 queries 和 usage 写入结果；搜索输入、所有尝试及终态另存带 SHA-256 的 JSON 工件，并以 web.searched 绑定会话、run、topic/task。搜索回执不替代网页正文采集或 finding 的已读门禁。DuckDuckGo 地区码和 recency 在原生协议没有已核验的等价硬约束，显式原生调用拒绝，只有 auto 可记录原因后选 DDG。[标准来源列表和过滤接口](https://developers.openai.com/api/docs/guides/tools-web-search)、[官方 SDK 的 max_tool_calls 定义](https://github.com/openai/openai-node/blob/main/src/resources/responses/responses.ts)。

真实供应商观察：本机 primary 为 gpt-6-luna，经已授权订阅端点实际返回 HTTP 400 / Unsupported parameter: max_tool_calls。该观察写为 supported=false、unsupported_capability，不能称为真实原生搜索成功；未删除调用上限、未换回私有接口，也没有跨供应商静默重试。仅 auto 保留显式尝试记录后允许 DDG 回退。output/remediation-20261018/search-live-capability.json 保存这项负向观察，手动 live probe 的“通过”仅表示观察契约验证成功，不表示供应商能力可用。

ui-web-search-native-smoke 18 项通过：真实 Kanzei 窗口点击拒绝和允许；批准前零原生请求，拒绝按既有契约停止本轮，无人值守缺授权时零请求，明确自动放行时一次请求。实际 Responses 传输携带调用上限和域名白/黑名单；返回的未检索引用和无效 URL 不进入结果，回执 SHA-256 与 SQLite 会话/run 身份正确，重载后答案来源仍可点击。测试使用确定的本地供应商，证明软件接线和权限，不证明实时检索相关性或当前 Codex 订阅能力。权限测试等待实际弹窗出现及防误触状态结束，拒绝终态按 stopped 核对，不要求模型在拒绝后继续回答。证据 output/playwright/web-search/native/acceptance.json 与 native-search.png。网页系列 53 项、能力配置 1 项、Core 全套 433 项及四 crate all-targets Clippy 通过；最终发布版复验仍待完成。

外部内容边界：ToolOutput 增加类型化 external_source；网页搜索/采集、浏览器 DOM 与图像、文件及进程输出在回喂模型时放入 JSON 字符串数据框。伪造 JSON、消息、工具终态和许可文本保留在 content 字符串内，不能关闭外层结构；实际执行器的终态位于框外。主代理、只读和可写子任务共用同一来源策略；角色局部缓存明确为待核实观察，不冒充用户偏好。界面实时显示原文，历史只剥一层合法传输框，源文中的伪造终态不提升为引擎事实。真实 HTML 的恶意内容反例先因缺失来源框失败，修复后通过；结构化 UI 契约 22 项通过。

原生表单扩展至 36 项：真实 Kanzei 自主/宽授权运行读取恶意网页，确定的供应商模拟器刻意遵从网页伪造指令，随后实际派发读取 .env 和改写 .claude/settings.json。两次操作均被精确授权门拒绝，原始文件字节不变，凭据金丝雀未进入模型请求。此证据证明执行器在模型受骗时仍守住这两个敏感边界，不证明真实 Luna 对提示注入免疫。已有 Edge/iframe 11 项和搜索权限/来源回放 18 项在来源传输改动后重新通过。回放测试重载后明确选择原对话，避免把另一个对话的正文当作来源链接丢失。

只读工具契约：将硬编码六个工具名字改为 Tool::read_only_support 的默认拒绝声明，分别表示只读、按动作验证和已审计内部控制接口；按动作工具仍须声明共享观察，普通写工具仍被拒绝。提问、团队消息和角色缓存仅在自身既有引擎范围内可用，不能改写用户资源或批准权限。完整 Tools 用例暴露此前名字白名单会拒绝 skill_read 等真实规划装配工具，修正契约后恢复，未通过新增名字例外绕过检查。内部缓存的 OS 路径约束和未知工具的安全性不由这个声明自动证明，仍属于后续完整隔离与敏感路径审计范围。

来源与只读契约的最终定向门禁：Core 433 项及集成 10+1 项、Harness 219 项、Tools 920 项全部通过；Tools 的 4 个明确独立联网/子进程入口未在本轮普通套件运行，不计为通过。App/Core/Tools/Harness all-targets Clippy 通过。最终发布版联合门禁尚未完成。

命令 OS 隔离：新增 sandbox 工具，Windows 通过默认 WSL2 发行版中的 Python 3/bubblewrap 0.9+ 执行，Linux 使用同一执行器。仅接受明确的普通相对文件，先经实际 ScopedFile 与敏感资源权限读取，再复制到引擎临时目录；上限 128 文件、16 MiB。宿主目录不映射为可写，输入只读，/usr 为只读运行库，工作区和 /tmp 分别由内核 tmpfs 限为 64/16 MiB。显式启用用户、挂载、网络、PID、IPC、UTS 命名空间，清空环境、去能力、禁止再建用户命名空间；没有 Windows 互操作程序或宿主进程入口。网络不提供允许开关，输出不自动写回项目。命令和全部复制文件都进入既有权限资源；隔离后端或握手失败直接拒绝，不回退宿主。Bash 描述明确它仍使用宿主用户权限。

受信控制器使用独立的 bubblewrap 状态管道取得内核命名空间身份，与自身比较后才记录 isolation_verified；不相信程序自报隔离。命令 stdout/stderr 仍是外部来源数据。超时、显式取消和应用 stdin 关闭都杀掉命名空间所属进程组，等待回收并排空管道后返回；即使被执行程序 setsid 脱离会话也清理。工作区不自动发布，故不能将此称作整个 Kanzei 会话已进入沙箱：当前仍需显式选 sandbox，整会话禁止其他宿主执行面的策略待补齐。

sandbox-boundary-smoke 16 项实际 OS 检查通过：明确输入、只读输入、宿主金丝雀不可见且不变、互操作入口不可见、无额外继承文件描述符、套接字联网失败、64 MiB 满盘，以及超时/取消/EOF 三种场景的脱离会话子进程确实消失。ui-sandbox-native-smoke 10 项通过：真实 Kanzei 模型工具调度和来源框、实际命名空间回执、工作副本可改而项目原文不变、AutoAllow 下 .env 复制被拒、敏感字节零回喂、原生停止等待 Linux 树回收。软件证据 output/playwright/sandbox/native/acceptance.json 和 native-sandbox.png；后端证据 output/remediation-20261018/sandbox-boundary.json。两项 Rust 定向通过。新增工具接入 Dev 延迟目录、只读档硬拒和 refgraph 区域映射，普通工具首轮常驻预算不增长。[bubblewrap 参数和隔离语义](https://manpages.debian.org/bookworm/bubblewrap/bwrap.1.en.html)、[WSL 执行方式](https://learn.microsoft.com/en-us/windows/wsl/basic-commands)。

记忆管理全套卡顿已定位：失败的 owned fixture 持久历史中，第一轮为本机模拟供应商预设的固定答案，第二轮却是另一段有效模型回答；同时测试进程只剩监听连接，正等待没有发生的第 4 个本机请求。用例原先逐轮通过进程全局 KANZEI_HOME 查找信任与配置，而 settings 等并行用例会临时替换该环境变量；只串行化写环境变量的用例，不能保护其他读取者。旧断言只看 followup changes 为空，允许错误路由返回的有效答案漏过。

修复将配置解析保持在 memory_chat_send 命令边界，run_chat 显式接收本轮不可变配置；正常软件仍每轮加载实际配置。用例改用自身持有的确定配置，不修改全局信任文件；还确认两轮确实产生 4 次 owned provider 请求，再等待服务器退出，不能让另一供应商的有效回答冒充通过。定向 0.16 秒通过，桌面完整 635 项在 17.95 秒通过，未使用串行全套、放宽超时、重试或跳过隐藏问题。该结果仍需最终 HEAD 的 Full 门禁复核。

整会话工具约束：新增 isolated ProfileKind，CLI/桌面/定时执行共用按 profile 构建 harness 的入口。隔离档使用独立的八工具装配，不执行普通 middle/tail 扩展，不加载 Markdown Agent/Skill 目录，不装配宿主 Bash/Git/process、浏览器/桌面、项目写工具或网络抓取/搜索；注册表外的工具即使 provider 强行返回、配置规则宽允许，也不能执行。子代理运行时返回 None；不附加原生 Hosted 搜索，不检索自动记忆上下文，不领取项目工作。复制目录的写入也改用 ScopedFile，保持实际 I/O 的固定根约束。命令输出仍走来源数据框。

隔离开跑另有宿主进程边界：Git status 本身可执行仓库 fsmonitor，因此 isolated 的 run.baseline 明确记录 not_observed_isolated、HEAD 和改动未知，不为展示版本启动宿主 Git。真实 Git 正向对照确认 owned hook 会产生文件，隔离 prepare 在同一仓库不执行 hook，事件仍按 run/session 保存。不能用未知版本冒充干净仓库。隔离配置延续已有的内容绑定项目信任门，不将未批准的仓库配置当成用户许可。

界面沿用执行模式选择器，显示下一轮的结伴开发、自主推进与隔离执行；切换会持久保存该会话档位，隔离不自动延续 Dev 工作队列。真实发送发现旧的项目登记监听只排除 readonly，错误截获了 isolated 输入；改为与 lineAgent 同源判断 Dev 工作流，同时不显示隔离会话的登记和选择工作入口。未绕过实际发送，原生验收从菜单选择模式、填写输入、点击发送并重载验证。

ui-sandbox-native-smoke 扩至 18 项通过，包含完整八工具模型请求的负向检查、供应商刻意返回 Bash 时 unknown tool 且 host-escape.txt 不存在、两轮起点未知事实和 UI 模式持久化；实际程序仍执行正常 sandbox、敏感输入拒绝、超时和停止。sandbox-boundary-smoke 扩至 19 项，新增正常退出时 detached 子进程也确实消失。隔离装配与 Git-hook 两项定向通过；桌面完整 635 项、Tools 923 项通过（4 个独立联网/子进程入口未计通过），App/CLI/Tools all-targets Clippy 与无障碍、3278-key i18n 静态及 i18n 契约 26 项通过。软件证据仍为开发构建，最终发布版联合复验尚待执行。

辅助读取和缓存旁路：实际目录链接将 Skill 的 SKILL.md 指向目录外时，旧 catalog 将 private description 带入结果；角色缓存文件夹链接到同项目 user-data 时，旧 agent_memory 既返回 private canary，又可通过正常 revision 写入该用户目录。两项反例确认在实际 I/O 断言失败，早期夹具的 Windows 路径分隔符和遗留锁文件处理问题单独修正，未将夹具创建失败冒充漏洞复现。

SkillDef 增加仅服务端持有、不经 IPC 反序列化的 content_root。目录发现与正文读取共用 ScopedFile 和 128 KiB 有界读取，保护文件身份也在读取前检查；发现后将原目录换成外部链接的正文请求同样拒绝。内置资源仍直接读取不可变嵌入正文。普通 read 之外的 grep/glob/files/symbols 和文件型 Skill 输出补齐外部来源框，角色缓存输出也采用该数据边界。

角色缓存严格绑定项目真实根下的固定角色 MEMORY.md，并将实际 I/O 固定到该角色目录；缺失缓存是合法空状态，其余路径/编码错误明确报告。上下文注入和工具入口共用同一读取方法，失败进入可选上下文的 Failed 报告，不当作空记忆静默丢失。缓存写入复用 ScopedFile 的原子替换；新增从已打开锁文件进入既有 OS FileLock 协议的入口，锁文件不再经环境路径打开。CAS 保留调用方的指纹算法，角色缓存继续使用原有 SHA-256；移除多角色共用的进程内串行锁。真实验证保留版本冲突拒绝、父目录替换后无外部数据/锁文件写入，以及普通 FileLock 与 ScopedFile CAS 的同线程兼容。

角色缓存凭据策略复用中央 memory 隐私校验：新写入含凭据直接拒绝，错误不回显秘密；遗留缓存的工具正文和上下文先脱敏，仍使用原始版本号支持修复。定向同时验证凭据值不落盘、遗留值不回喂、有效版本能够更新，然后再验证目录链接读写均拒绝且用户数据不变。Base 38 项、Harness 221 项、Tools 923 项全套通过（4 个独立入口另计）；凭据追加改动的角色缓存定向、中央 privacy 4 项和五 crate all-targets Clippy 通过。

ui-skills-native-smoke 原生 22 项通过：新装发现、恶意目录链接的描述/正文均不返回、生成草稿不落盘/不创建对话、保存、导入引用资源、编辑/复制、陈旧版本拒绝、禁用项不进入实际模型请求、显式调用正常 Skill、重启恢复、删除可恢复以及内置资源保护。private canary 未进入任何实际模型请求，外部源文件字节不变。夹具改用标准 WebView2 CDP 参数，等待真实页面创建；使用 source-qualified 偏好 ID，并在生成验证后显式创建测试对话。证据 output/playwright/skills-native/checks.json 与 skills-native.png；属于开发构建。

单 Skill 导入改为 skill_in_directory，使用与批量发现相同的纯元数据解析器及 ScopedFile 有界读取，只打开用户选中的目录，不再提前环境读取 SKILL.md 或扫描父目录及兄弟目录。Harness 解析 10 项、桌面 Skill 3 项通过。

无项目隔离与陈旧发送者：原规则将已保存 isolated 强制改回 dev 的反例先失败，再统一创建、更新、恢复、关联项目与发送入口的档位处理。已保存 readonly/isolated 由接收会话决定，旧发送者携带 dev/宿主 agent 不能升级；真正的界面档位切换仍通过既有持久设置入口。无项目聊天显示隔离模式选择器，不显示自主项目流程选项。

ui-sandbox-native-smoke 原生 22 项通过，包含项目隔离正常运行、强行调用 Bash 拒绝、旧 Dev 请求拒绝、无项目界面选择与实际发送、重开后仍为 isolated，零 host-escape.txt 写入。测试创建新聊天后强制取创建后的列表，避免复用旧轮询快照而把输入误发到上一条；更多菜单按实际可见状态展开，避免 IPC 场景下把已打开菜单误关。反例 general-isolated-red.log、定向 green 与 stale-mode、原生 acceptance.json 均保留。开发构建通过不替代最终发布版绑定。

Git 只读观察：实际 owned fsmonitor 通过普通 Git 正向对照创建金丝雀；原 status 工具也创建金丝雀，负向断言失败。status/diff/log 和运行起点现在共用进程内 libgit2 观察，不启动 Git 程序；Git 修改事务保留原有明确操作及租约。工具仓库归属判定也改为现有无进程的项目事实入口，不能在动作分发前启动 CLI。status 不写索引；工作区 diff 不调用自动转换，而是通过 ScopedFile 读取工作文件，再与索引对象比较；暂存 diff 同样在返回对象内容前核对文件范围与精确凭据许可。输出采用文件来源框。

Git 40 项通过，涵盖 log 条数/路径过滤、既有提交事务，以及 fsmonitor、clean filter、textconv 配置不启动测试程序；跟踪 .env 的工作区和暂存 diff 未授权时不回显金丝雀，精确 read 规则下才能返回。运行起点 6 项通过，包括部分状态、未知 HEAD、父仓库隔离、持久来源和 isolated 不执行 hook。新依赖 git2 0.21.0/libgit2 1.9.7 关闭默认传输功能；API 语义参照 [DiffOptions](https://docs.rs/git2/latest/git2/struct.DiffOptions.html) 和 [libgit2 status](https://libgit2.org/docs/reference/main/status/index.html)。联合回归桌面 636、Core 435+10+1、Tools 925 通过，Tools 4 个独立入口不计通过，三 crate all-targets Clippy 通过。

重复文件观察投影：只处理 read/grep/glob/files/symbols 的成功、纯文本、带引擎 File 来源框且工具输入和完整输出字节完全相同的结果。在当前请求保留最新完整副本，较旧副本改为指向其 call_id、SHA-256 和来源的引擎引用。图像/文档、错误、修改工具、变化的内容和其他来源不合并。持久历史、工具实际执行和 UI 原文不变；每次请求按仍在该请求中的证据重新计算，不留下指向已压缩历史的悬空引用。预算估计与发送使用同一投影。共享调查提示先定位路径/符号，再按问题读取，交接带版本、定位与未解项；需验真或版本变化时继续复核。

原生 ui-observation-native-smoke 8 项通过：真实界面发送后四次实际 read 均完成，实际模型请求有 3 个旧副本引用及 1 个最新正文，四个工具身份保留；SQLite 事实仍有 4 份完整观察，源文件未变，真实重载保留答案与来源。证据 output/playwright/observations/native/acceptance.json、native-observations.png。首次脚本因全新项目的登记入口截获 Dev 输入而未实际运行；改为用户可选的 isolated 观察档走实际发送，不将超时当作产品通过。

真实 gpt-6-luna 固定源码配对：两题各发 raw/projected 请求，第二题交换顺序，不重试。按源码路径核对答案与引用，4/4 正确；题一请求字节 4961→3509、总输入 1269→936 token（-26.24%）；题二 3398→3161、840→825（-1.79%）。记录同时保存 uncached/cache_read/cache_write，不把缓存当成上下文节省。是四次固定源码问答，不是完整自主调查或通用智能评分，两组压缩次数均为 0。首轮严格索要内部 call_id 时 raw 两题返回 functions.read、projected 返回 obs-3：代码结论都正确，但原始引用检查失败；该轮结果保留在 observation-benchmark-1，未删除或计作通过。改用双方都能从来源数据观察到的源码路径后，新输出保存在 observation-benchmark-2。更长调查对照仍待执行。
