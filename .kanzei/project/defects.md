# Defects

## D-204 SOP 用户易用性不佳:总结质量/查看展示/产生时机三处都不行 [fixing] (medium)
- refs: D-205 R-105 R-107
- 原始描述: SOP易用程度有问题，似乎总结的不太好
- 澄清(2026-08-09 用户逐项指认): 所指为**用户**查看/使用 SOP 时的易用性,不是 SOP 对模型的可消费性。三个维度都有问题:①**总结质量差**——条目内容泛化、丢关键步骤,看了不知道怎么照做;②**查看入口/展示**——界面上找到、打开、阅读 SOP 的路径不方便,展示形式不适合阅读;③**产生时机/数量**——该沉淀的没沉淀、不该沉淀的乱沉淀,产出节律不对。检索/命中用户未勾选,暂不在范围内。
- 复现: 桌面端 Memory 页(R-107)查看 sop 类条目;对照近期自举轮次的 SOP 产出(如 inbox 里的「候选 SOP:完成 D-155 的流程」类,只有工具顺序罗列,无判断依据与边界条件)。
- 影响: SOP 是 R-105 记忆蒸馏的主要产出形态之一,人读不动就只剩模型消费一条腿;产生时机不对还会稀释记忆库信噪比。
- 验收: ①总结质量:SOP 条目有可照做的结构(适用场景/步骤/每步判断依据/边界),不再是纯工具名罗列;②查看展示:Memory 页的 SOP 有适合阅读的排版,入口可发现;③产生时机:沉淀门槛可说明(什么样的流程值得成为 SOP),乱沉淀实例(纯机械序列)被拦;④用户复查确认三个维度都有改善。
- 备注: 本条登记过程本身暴露了快记的信息保真缺陷(伪复现「查看 SOP 时」+丢"用户"限定词),已单独登记为 D-205 并修了第一层。
- 优先级: P3
- 标签: 核心

- 批次: 2/2
- 进展: 两批交付+全量绿,逐条证据:①总结质量——harvest_sop 候选 detail 给 manager 可照做结构模板(1.适用场景 2.操作步骤:每步做什么+判断依据 3.边界与例外)(crates/kanzei-tools/src/memory/mod.rs harvest_sop),manager_agent() prompt 加 SOP 提炼规则(纯工具罗列不算 SOP、一次性流程 NOOP)(memory/manager.rs manager_agent),不再纯工具名罗列;②查看展示——Memory 页列表行 sop 加左边框+SOP 徽标(13-memory.js loadMemoryList + style.css .memory-row.sop/.memory-row-cat.sop,入口可发现),详情正文 renderMemoryBodyRead 识别「N. 标题」编号行渲染为结构化步骤块(.memory-sop-step 标题加粗+正文剥离冒号);③产生时机——harvest_sop 加工具序列门槛(tools<3 机械拦截,纯机械序列被拦,memory/mod.rs + 回归单测短流程不投)。④待用户复查确认三个维度。验证:T-1786451023(批1 定向 232 绿)+T-1786451128(批2 前端四冒烟)+T-1786451243(关闭前全量全绿)。2026-08-13 用户定调:要改但不是现在,需求比较边缘,优先级 P2→P3,阻塞保持成立。

- 阻塞: 2026-08-14 前置已满足:新版 build-9a06e05 已发布,工程面①②③早已交付并全量绿。剩验收④一条:装新版后打开 Memory 页,看 SOP 的总结质量/查看展示/产生时机三处是否确有改善(新排版 + 新沉淀门槛)。解除动作: 用户复查后确认改善即可关闭。解除人: 用户。

- priority: 

## D-319 WebView2 当前环境 DevTools 端口不监听:e2e-smoke connectOverCDP 20 秒超时(参数已传入但不绑定) [fixing] (medium)
- 复杂度: 中
- 复现: 2026-08-16 D-289 实测验证中发现:无论 WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS 环境变量还是 KANZEI_E2E_CDP 注入路径,WebView2 进程命令行均带上 --remote-debugging-port=<port> --remote-allow-origins=*(进程命令行实证),但端口 20 秒不监听、user-data-dir 无 DevToolsActivePort 文件、进程树完整(renderer/gpu/network 均在)、会话与用户 kzapp 同为 Session 1、无策略禁用(注册表 HKCU/HKLM EdgeWebView/Edge 均空)。对照:同参数字符串起 Edge --headless,1 秒即监听。结论:WebView2 在当前机器/环境不启动 DevTools 端口,与参数注入路径无关。
- 影响: R-101 e2e-smoke 基座在自举环境无法实测 connectOverCDP(端口不监听→20 秒超时→FAIL)。这独立于 D-289 的 origin 白名单修复——即使端口能监听,D-289 也是必需的(M111+ 拒非白名单客户端);但端口不监听会让 e2e-smoke 永远失败。
- 来源: self-found(D-289 实测验证中发现)
- 标签: 流程
- 优先级: P2
- 进展: 2026-08-16 取活诊断(9 轮实验,证据链完整)。已排除:①注入通道——环境变量 WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS 与 KANZEI_E2E_CDP additional_browser_args 两条路都传参成功(msedgewebview2.exe 命令行实证含 --remote-debugging-port/--remote-allow-origins/--remote-debugging-address);②参数格式——同参数字符串起 Edge(含非 headless)1 秒监听;③策略——HKCU/HKLM EdgeWebView/Edge DeveloperToolsAvailability 全空;④AppContainer/容器——whoami 无 AppContainerSid,Session 1 正常;⑤日志——--enable-logging=stderr --v=1 无 devtools 相关输出;⑥版本——WEBVIEW2_FIXED_VERSION 指定 151.0.4129.59 未生效(Tauri 仍用 78),151.0.4129.78 签名有效、安装完整;⑦进程树——renderer/gpu/network 完整,webview 已创建但 browser 进程不绑定端口、无 DevToolsActivePort 文件。结论:WebView2 Runtime 151 在当前机器不启动 DevTools HTTP 服务,与参数注入/代码路径/系统策略均无关。排除后剩余变量是 WebView2 Runtime 本身或其与系统环境的交互(需重装/更新 runtime 或换环境验证,或改用 WebDriver/tauri-driver 路线)。阻塞:解除人=用户。
- 阻塞: WebView2 Runtime 151 在当前机器 DevTools 端口不绑定(9 轮实验证据链,见进展)。解除动作:①用户重装/更新 Microsoft Edge WebView2 Runtime 后重跑 e2e-smoke;或②用户提供 WebView2 DevTools 正常的环境验证;或③用户拍板改 WebDriver/tauri-driver 路线(不在本条范围内)。解除人:用户。

## D-289 R-101 harness CDP 注入缺 --remote-allow-origins:可能致 e2e-smoke connectOverCDP 握手失败 [fixing] (medium)
- severity: medium
- 优先级: P1
- 复现: R-101 harness 基座静态审查:crates/kanzei-app/src/main.rs:110-116 的 KANZEI_E2E_CDP 注入只加 --remote-debugging-port=<port>,未加 --remote-allow-origins=*;同轮实验脚本 output/e2e-exp/env-var-exp.mjs:16 加了 --remote-allow-origins=*(且 .playwright-cli 08-11 快照证明 CLI 曾连上)。WebView2 基于 Chromium,自 M111 起 CDP 要求显式 origin 白名单,否则非 DevTools 客户端(playwright-core connectOverCDP)握手被拒。
- 影响: scripts/e2e-smoke.mjs 可能 connectOverCDP 失败,harness 基座验证被卡;若 e2e-smoke 实际能连上则本条为误报,实测后关闭。
- 来源: self-found(2026-08-13 R-101 静态审查)
- 标签: 流程

- 复杂度: 小
- 进展: 2026-08-16 修复落地(commit 待定):main.rs KANZEI_E2E_CDP 注入的 additional_browser_args 追加 --remote-allow-origins=*(仅 E2 注入路径,生产不带)。验证:①编译+kanzei-app 137 测试通过;②WebView2 进程命令行实证参数已传入(--remote-debugging-port=<port> --remote-allow-origins=* 均出现在 msedgewebview2.exe 命令行);③Edge 对照:同参数字符串起 Edge --headless 1 秒监听,证明参数格式/网络层无误。实测:e2e-smoke 20 秒超时 FAIL——根因已定位为 WebView2 在当前环境 DevTools 端口根本不监听(端口不绑定、无 DevToolsActivePort、无策略禁用、进程树完整),与 D-289 的 origin 白名单修复正交(端口能监听时 connectOverCDP 仍需白名单,D-289 是必要修复);环境问题已登记 D-319。按 §1.25 保持 fixing,验收(connectOverCDP 实测成功)待 D-319 解除后补齐。

- 阻塞: D-319(WebView2 当前环境 DevTools 端口不监听)未解决前,e2e-smoke connectOverCDP 20 秒超时无法实测。解除人:解决 D-319 或确认 WebView2 环境可起 DevTools 后重跑 e2e-smoke。

## D-275 托管路径 OS 层写隔离(残余):后台进程与专用工具同窗口同前缀时仍可蒙混 [fixing] (medium)
- 优先级: P2
- 复杂度: 大
- 来源: 2026-08-11 D-258 关闭时转出(验收①的 OS 层条款未做,成本收益倒挂且与验收②互斥)
- 标签: 核心
- 缺陷: D-275
- 证据等级: E1
- 进展: B1 代价评估(2026-08-16,取活即评估):路线比对——①纯 ACL(icacls 锁两颗托管树):不可行。引擎与子进程共享用户 SID,ACL 按 user/group 判定,锁掉后台子进程同时锁掉引擎自身(引擎进程内工具 test_record/memory_note/tracker 都要写托管路径)。②受限令牌(子进程 CreateRestrictedToken+deny SID):技术上可行但集成面=全仓散点 spawn(kanzei-app processes.rs/bash 工具/docs.rs/background 任务等,std::process::Command 无令牌支持需改 CreateProcessW),且 POSIX 无等效(flock 是 advisory 不区分进程),跨平台价值存疑。③低完整性(low-integrity 子进程+target//node_modules/ 显式授予):唯一可区分进程的 OS 机制,但仍是 Windows-only、需在 spawn 面铺完整性级别管线+为 target/ 建低完整性 ACE+测试,与验收①「不破坏 target/ 写」可相容;POSIX 降级=显式告警无等效。风险收益:残余窗口是毫秒级(需恶意/故障子进程恰在专用工具执行时写同一托管前缀),D-258 快照吸收已覆盖常见路径,原评估定调成本收益倒挂。结论:OS 层隔离存在技术上可行路线(低完整性)但成本高、仅 Windows 有效,残余风险极低——是否值得投入由用户拍板:接受残余边界(文档化,关闭或维持 open)或另立范围化实现条目。
- 验收: ①存在一条后台进程在操作系统层面写托管路径失败的机制(受限令牌/低完整性/ACL),有实测证据,且不得破坏后台任务写 target/、node_modules/(与 D-258 验收②同口径);②该机制与托管写入窗口(managed_fence)组合后,窗口内后台进程与专用工具写同一批路径也不能蒙混——即吸收/回滚不再依赖镜像快照区分;③跨平台降级路径有明确说明(Windows 独占句柄 vs POSIX advisory lock),降级时不静默放行而是显式告警。
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-275
- 批次: 1/1
- 阻塞: B1 代价评估已完成(见进展),结论=OS 层隔离技术可行但成本高/仅 Windows/残余风险毫秒级,是否投入由用户定夺。解除动作: 用户拍板——接受残余边界(文档化后关闭/维持 open)或另立范围化实现条目(低完整性路线)。解除人: 用户。
- observed_head: 45fd276e9ac4ac6a23c0027b801f95d6c6c3fe4f
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786598491964

## D-342 停止运行 = handle.abort() 硬杀,被打断轮的对话历史整轮丢失 [fixing] (high)
- refs: R-236 docs/design/context_compaction.md
- 复现: 自动推进中点「停止」再发新任务:stop_runtime_and_finalize(kanzei-app/src/state.rs:534)直接 handle.abort() 杀掉 run_task 的 future;而对话写回只在轮末(run.rs:1032 内存表、run.rs:1089 conversation.updated 事件),abort 永远到不了那两行 → 被打断轮的全部消息(可能几十步工具调用/改动/结论)从对话投影消失,下一轮 prior 停在上一轮轮末。模型于是称"之前没做过 X"(用户 2026-08-14 实测报告)。
- 影响: 打断+插临时任务是自动推进的高频交互,每次都让模型对被打断轮完全失忆;episode/run.trace 有留档但那是回放用的,模型看不到。runner 侧没有优雅停止:halted_by_user=true 唯一产生路径是权限弹窗被拒(kanzei-core/src/runner/drive.rs:1059),步循环里没有任何 halt 检查点。
- 来源: 用户报告(2026-08-14 自动推进打断丢上下文)+ 读码定位
- 标签: 核心
- 进展: 2026-08-16 复核轮:实现(commit cbe768a)与既有测试逐条核对完毕。验收①③④⑤证据齐备:①cooperative_halt.rs 测试①(halted 轮 messages 完整交还:prior+本轮用户消息)+ run.rs:1114-1117 写内存表、run.rs:1228-1244 conversation.updated 事件落库 + conversation.rs:158 recover_messages_raw 从事件恢复 + conversation_tests.rs:94-107 写回→恢复链路测试;③drive.rs 步首 222/流内 select 422/工具间 790/步末 991 四类检查点 + cooperative_halt.rs 测试②(执行中停止挂起子代理被打断、取消占位配对、filter 后逐字节无孤儿);④state.rs:610 stale_run_needs_abort 纯函数 + process_tests.rs:129 兜底硬杀只认停止时那一代测试 + process_tests.rs:73 正常停止置令牌不 abort;⑤process_tests.rs:114 排队输入即刻取消(cancelled==1)+ finalize_interrupt 在 state.rs:592/603/633 全部路径保留。验收②机制部分(新一轮 prior 含被打断轮内容)由 cooperative_halt 测试①(prior 原样在 messages)与 run.rs:699 conversation_prior 取内存表证明;但「模型可复述被打断轮做过的事」需真实模型桌面端场景——R-236 联测(requirements-archive R-236 验收⑤)明确未覆盖 D-342 停止路径(CLI halt 通道为 None,桌面端停止场景待用户新版实际使用验证),本复核轮无真实 provider 桌面端联测条件,该项缺口保持。
- 验收: ①自动推进中途停止后,conversation_get 能看到被打断轮已完成步骤的消息(实测轨迹,不是只断言函数返回);②停止后立刻发新任务,新一轮 prior 含被打断轮内容,模型可复述被打断轮做过的事;③停止响应有上界:当前工具执行结束即停,不等整轮跑完;④abort 兜底路径保留(防挂死)且有测试,正常停止不走它;⑤停止仍取消排队输入并释放写租约(现有 finalize_interrupt 语义无回归)。
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-342
- 阻塞: 2026-08-14 前置已满足:新版 build-9a06e05 已发布并含 cbe768a 及后续。剩验收②后半一条实测动作:在桌面端跑一轮任务,中途点「停止」,立刻发一条新任务,看模型能不能复述被打断那轮做过的事(能复述=被打断轮的对话历史没整轮丢)。解除动作: 用户实测并反馈结果后补关验收②。解除人: 用户。
- observed_head: dd5e5fd66bfe1387331ccac3f449f51924d7a103
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786652156479

## D-349 工具大输出在事实入库前不可逆截断，trace 仅留 preview 且无完整原文回读 [open] (high)
- refs: D-209 R-180 R-245 docs/design/deepseek_harness_upgrade.md
- 复杂度: 中
- 复现: 执行输出超过上限的 bash/git/webfetch 或后台任务：bash/git 在工具层截断，run.trace 再仅记录 preview；当前会话没有 artifact_id 或回读指引。进程退出或上下文压缩后，用户和模型均无法从会话恢复完整原文。
- 影响: 工具结果的事实在写入事件日志前已经丢失；审计、故障复盘、后续精确引用和压缩后回读只能看到片段，可能隐藏真正报错或把截断结果误认为完整结果。
- 来源: 2026-08-14 DeepSeek Harness Spill 对照审计与现行代码核查。
- 标签: 核心
- 根因: 各工具各自实现容量上限和截断文案，ToolOutput 没有 Inline/Spilled 统一结果类型，也没有“完整 artifact 写成功后再提交引用事件”的原子契约。
- 证据等级: E2(静态读码确认截断点与 preview 入库路径；本地输出分布已量化)
- 阻塞: 等待 R-244 Tool Pipeline 结果阶段稳定并由 R-245 实施。R-244 已于 2026-08-14 由用户定调列入主任务、主线串行做,依赖链有确定落点,不再是「等用户决定」。当前仍作为事实丢失缺陷登记(high),不单独修——在 R-244/R-245 的 Result Policy 与 spill 落点上一并解决。解除人: 依赖自然解除。
- 验收: ①超过阈值的 bash/git/test_record/web 类结果完整原文进入 durable artifact，事件只存 preview+artifact_id+bytes+sha256+retrieval_hint；②重启后按引用取回内容与工具原始字节 sha256 一致；③artifact 写失败时不得提交成功引用事件，事件写失败时无引用 artifact 可由整理入口识别；④UI/模型明确显示结果已外置而非已丢弃；⑤read 的原文件 offset/limit 回读不重复复制；⑥现有工具权限与错误码不变。
- 优先级: P1

## D-372 鞭挞确定性饿死:auto_pending 不在相位表里,轮询把已结束的一轮复活,重试耗尽报「上一轮尚未结束」 [fixed] (high)
- refs: D-291 D-323 R-086 R-206
- 复现: 开鞭挞(dev-auto)跑完任意一轮 → kz:done 带 autoAction=Continue → 等 32 秒。实测现场 2026-08-15 21:40:57 运行完成(60 轮/3040.7s) → 21:41:29 报「鞭挞未续跑:上一轮尚未结束」,正好 2s + 15×2s = 首次 + AUTO_CONTINUE_RUNNING_GRACE 次重试全部耗尽。
- 根因: 03-shell.js transitionSession 相位表只有三个分支(starting/running、stopping、idle/stopped/failed),**auto_pending 一个都不匹配**,于是它既不置 converged 也不清 live_running。链路:①轮内 "running" 置 live_running=true/converged=false;②kz:done→"auto_pending",两个字段原样残留;③kz:idle 到达时 01-core.js 算 targetPhase = auto_pending ? "auto_pending" : "idle",唯一一次能收敛的机会被自己吃掉;④≤3s 后 process_list 校正(09-sessions.js:397-403)因 converged 为假不跳过、命中 live_running===true 分支 transitionSession(sid,"running") 复活;⑤armAutoContinue 每 2 秒复查 processRunning 恒为真。09-sessions.js 末尾那条 `!["auto_pending","stopping",...]` 例外说明作者本来就把 auto_pending 当静止态,只是相位表没跟上。
- 影响: 鞭挞是自举的主循环。它停摆 = 自举停摆,且失败形态是「界面显示待命、实际永不续跑」,不报错、不重试,只能人工再点一次。D-291 修的是「静默不续跑」,本条是「出声了但结论是错的」——同一入口的另一侧。
- 来源: 2026-08-15 用户截图报告 + 日志时间戳比对(32 秒签名与 01-core.js:78 注释里记录的上一次现场同型)。
- 证据等级: E1(反证实测:把 auto_pending 从相位表移除后 ui-runtime-smoke 5 条断言全红,其中「process_list 校正把 auto_pending 复活成 running」直接复现根因;加回后全绿)
- 验收: ①auto_pending 与 idle/stopped/failed 同组收敛(converged=true、live_running=false、local_start_pending=false,terminal_status 保持空);②收敛不得改写 phase(界面「等待下一轮」与待命徽标靠 phase);③process_list 校正与迟到进度事件都不得复活已收敛的一轮;④processRunning 在 auto_pending 下为假,续跑闸门第一次复查即放行;⑤宽限耗尽但后端权威 item.running=false 时按后端收敛并继续(自愈),而不是一律放弃;⑥ui-runtime-smoke 有反证型回归,移除修复即红。
- 优先级: P0
- 标签: 核心
- 进展: 已修。03-shell.js 相位表把 auto_pending 并入终态分支(带完整链路注释);08-compose.js armAutoContinue 宽限耗尽路径按后端权威自愈(item.running 为假则收敛本地态继续,为真才放弃),顺手删掉一处死变量 targetState;02-i18n.js 补自愈提示词条。ui-runtime-smoke 新增 5 条反证断言(①~⑤逐环)。六条前端冒烟全绿(ui-runtime/ui-lint/parallel-lines/ui-a11y/ui-i18n/ui-markdown)。
- observed_head: 9e79edc71bffaf52d9fd5b25f1c9bd4773382853

## D-373 加进建表批的 DDL 对存量库永久无效:D-297 的下推索引在真实主库里从不存在,验收却在新库上通过 [fixed] (high)
- refs: D-297 R-155
- 复现: 任意存量库(schema_version 已等于 SCHEMA_VERSION)执行 `EXPLAIN QUERY PLAN SELECT ... FROM session_events WHERE session_id=? AND sequence>? AND event_type=?`。实测本仓主库(132MB/74,184 行):计划为 `SEARCH USING INDEX session_events_session_sequence (session_id=? AND sequence>?)`,即按 (session_id,sequence) 扫完该会话 72,751 行再逐行过滤 event_type;代码里写着的 session_events_session_type_sequence 在 sqlite_master 里根本不存在(代码 DDL 与真库对象集合差集恰好只有它一个)。
- 根因: D-297 把 `CREATE INDEX ... session_events_session_type_sequence` 加进 migrate 的**建表批**,但没有提升 SCHEMA_VERSION。migrate 在 `version == SCHEMA_VERSION` 时直接 `return Ok(())`(schema.rs:34),于是建表批对**所有已经停在当前版本的库**一次都不会执行。新建的临时库走的是另一条路(无版本记录→跑全批),所以单测、验收、CI 全绿——「代码里有、真实库里没有」不产生任何信号。
- 影响: ①D-297 的读路径优化在真实环境从未生效,list_events_by_type 仍是全扫;②更要紧的是这是一个**类**而不是一条:今后任何加进建表批的表/索引/列都会静默跳过存量库,而唯一的使用者就是本机这一份长期库。
- 边界: 不要改成「每次 open 无条件跑全批」——open 是高频路径(每个 Tauri 命令/每条轨迹事件各一次),把建表批塞进去等于给每次 open 加一串 DDL 解析。正确修法是版本号 +1 加机械判据。
- 来源: 2026-08-15 用户要求三维度审视,只读核查真库 sqlite_master 与 EXPLAIN QUERY PLAN 时发现。
- 证据等级: E1(真库 EXPLAIN 实证 + 代码 DDL 与真库对象集合逐项差集 + 迁移后计划切换与耗时实测)
- 验收: ①SCHEMA_VERSION 提升到 14,存量库 open 后补齐缺失对象;②存在机械判据,往建表批加对象而不提版本号必然判红并指出修法;③反例实证该判据会拦下;④下推索引真的被查询计划选中(不只是「存在」);⑤顺带删除与 UNIQUE(session_id,sequence) 自动索引完全重复的 session_events_session_sequence。
- 优先级: P0
- 标签: 核心
- 进展: 已修。SCHEMA_VERSION 13→14(mod.rs 版本注释写明「改建表批=同时+1并更新 SCHEMA_OBJECTS」);建表批加 `DROP INDEX IF EXISTS session_events_session_sequence`。三条新测试:①建表批新增对象必须伴随schema版本提升(对象集合按版本冻结的机械判据,反例实测——插一条 zz_counterexample_idx 立刻判红并打印修法);②停在上一版的存量库open后补齐到与新库一致;③按类型取事件走下推复合索引而不是全扫(EXPLAIN 断言,挡「存在但用不上」)。真库副本(132MB)实测:迁移 268ms + 升级前整库备份 95ms;查询计划切到 session_events_session_type_sequence;run.completed 类查询 64ms→5.1ms;删冗余索引回收 5.3MB(132.0→126.7MB)。kanzei-core 196 passed,clippy 零警告。
- observed_head: bbf2241
