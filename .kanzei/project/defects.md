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

- 阻塞: 2026-08-16 复核:工程面①②③早已交付并全量绿,阻塞仍是验收④一条(用户复查)。原文点名的 build-9a06e05 已过时——当前最新发布为 **build-e579472**,其后又叠了多轮修复。解除动作: 装 build-e579472 后打开 Memory 页,看 SOP 的总结质量/查看展示/产生时机三处是否确有改善,确认即可关闭。解除人: 用户。

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
- 阻塞: 2026-08-16 复核:工程面已交付,阻塞仍是验收②后半的一条实测动作。原文点名的 build-9a06e05 已过时——当前最新发布为 **build-e579472**。解除动作: 装新版后跑一轮任务,中途点「停止」,立刻发一条新任务,看模型能不能复述被打断那轮做过的事(能复述=被打断轮的对话历史没整轮丢)。用户实测并反馈后补关验收②。解除人: 用户。
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
- 阻塞: 2026-08-16 复核收窄:**R-244 已 done 并归档**(Tool Pipeline 结果阶段已稳定),原阻塞的前半已解除;只剩等 R-245 实施,而 R-245 自身只剩等 R-242(见该两条)。当前仍作为事实丢失缺陷登记(high),不单独修——在 R-245 的 Result Policy 与 spill 落点上一并解决。解除人: 依赖自然解除。
- 验收: ①超过阈值的 bash/git/test_record/web 类结果完整原文进入 durable artifact，事件只存 preview+artifact_id+bytes+sha256+retrieval_hint；②重启后按引用取回内容与工具原始字节 sha256 一致；③artifact 写失败时不得提交成功引用事件，事件写失败时无引用 artifact 可由整理入口识别；④UI/模型明确显示结果已外置而非已丢弃；⑤read 的原文件 offset/limit 回读不重复复制；⑥现有工具权限与错误码不变。
- 优先级: P1

## D-419 编排派发的子代理条目卡在「运行中」:ToolEnd 要等整波过屏障才统一发,单条停止必然报「不在运行中或已结束」 [open]
- 严重程度: medium
- 优先级: P2
- 标签: 前端 后端
- 复现: 2026-08-17 01:27 用户实测截图——子代理面板头部显示「运行中 5 · 已完成 3」,architecture_scout 条目仍是 running 态并带「停止」按钮;点停止后运行日志连续 5 次「停止失败:子代理 architecture_scout 不在运行中或已结束」(01:27:31、01:27:36 ×4)。同轮该条目显示「43s · 工具调用 8 · token 0」。
- 根因: crates/kanzei-app/src/phase_pipeline.rs:386-401——`dispatch_roles` 把全部角色的 `RunEvent::ToolEnd` 放在 `join_scouts`/`join_reviewers` 屏障**过完之后**统一发。而单个 scout 一返回,它的 `TaskCancellationGuard` 就 drop 并从 `TaskCancellations` 注销(crates/kanzei-core/src/runner/subagent.rs:687 注册、77-81 Drop 注销)。于是存在一个必然窗口:后端该子代理已终态且不可取消,面板却还没收到 ToolEnd、仍显示 running 并给出停止按钮 —— 点必失败。波内有一个角色慢(或超时,timeout_secs 兜底)时,窗口等于最慢角色的剩余时长。
- 影响: ①面板「运行中 N」计数在整波结束前不可信,用户无法判断子代理是否真的还在跑;②停止按钮对已结束条目仍可点且必然失败,连报错刷屏;③与 R-174「子代理单条停止通道」的设计意图相悖——单条停止在编排派发路径上对已完成角色形同虚设。
- 修复方向: 角色终态即发 ToolEnd,不等屏障。ScoutTask 的 async 块里 reports.push 之后(phase_pipeline.rs:348)就把该角色的终态经既有 tx 通道发出去(与进度事件同一条通道,select 循环已在转发),屏障之后那段循环改为只补发未见终态的角色(超时/未产出结果那一类)兜底,避免重复发。
- 来源: 2026-08-17 用户实测截图并问「看下这个为啥卡住了」;勘察确认事件侧确实有发 ToolEnd(phase_pipeline.rs:392),断点在**发的时机**而非有没有发。
- refs: R-174 R-173 R-281
- 备注: 同轮「token 0」是另一回事——子代理跑 fast 路由(qwen3.5:4b / Ollama),StepEnd usage 由供应商回报,本地模型多半不报数,与本条终态时机无关,未并入本条。

## D-420 window.prompt 输入弹窗在 WebView2 下失效:5 处(自定义 provider/项目重命名/新建项目)需迁到内联输入或自定义输入弹窗 [open] (medium)
- 复现: 5 处输入弹窗仍用浏览器原生 window.prompt:08-compose.js:1345(填 provider:model)、09-sessions.js:761(重命名项目显示名)、09-sessions.js:846(新项目目录路径)、09-sessions.js:848(新项目显示名)、16-settings.js:369(填 provider:model)。桌面端为 WebView2,15-views-misc.js:85 注释明确『webview 无 window.prompt』(新建想法已因此改为内联输入 R-252)——这 5 处在真实桌面端弹不出输入框/返回 null,输入功能失效。
- 影响: ①桌面端 5 个输入功能(自定义 provider 模型、重命名/新建项目)实际不可用(webview 下 window.prompt 返回 null,输入丢失);②与 D-418 确认弹窗收敛同源:原生浏览器弹窗在自定义 UI 体系下割裂。
- 来源: D-418 修复复核(test_reviewer 发现 window.prompt 遗留);grep 全量确认 5 处 + 15-views-misc.js:85 的 webview 无 prompt 注释佐证。
- 标签: 前端
- 优先级: P2

## D-423 opencode zen /responses 对 assistant 侧输入条目一律 500:多轮工具循环第二轮必死(kanzei 侧应能识别并给出可操作诊断) [open] (medium)
- 复杂度: 中
- 标签: 模型
- 优先级: P2
- 来源: 2026-08-17 修 D-422 后暴露:工具调用恢复了,第二轮却 HTTP 500(重试 2 次后整轮失败)。
- 复现: curl 直连 https://opencode.ai/zen/go/v1/responses(model=mimo-v2.5-pro),逐项对照 `input` 里的条目形态——
  `[user, assistant(content 数组: output_text / input_text / text 三种都试)]` → **500**;
  `[user, function_call, user]` → **500**;
  `[user, function_call_output, user]` → 200;
  `[user, assistant(content 纯字符串), user]` → 200;
  `[user, user]` → 200。
  即:该网关的 /responses shim 只认纯字符串形式的 assistant content,任何数组式 content 与 function_call 条目都 500。而 kanzei 的 deepseek_responses::build_body 两种都发。
- 影响: 用该 provider 的 responses 协议时,凡历史里有助手输出(即第二轮起)必然整轮失败。用户侧现象是重试两次后「provider returned HTTP 500」。
- 处置(最终): `[providers.OPEN-code]` **保持 deepseek-responses**。中途曾改成 openai(/chat/completions)去救 mimo——实测三个模型的多轮工具循环在那条路上都是 200 且 finish_reason=tool_calls——但 D-425 判定 mimo 无解之后,改协议就只剩代价:chat completions 回放不了无签名的 Reasoning part(openai.rs build_body 直接跳过该 assistant 消息),推理模型跑多步工具循环时每步都看不到自己上一步的思考,而 deepseek-responses 会原样回放(deepseek_responses.rs 的 `Part::Reasoning` 分支)。已回退并复验:`KANZEI_MODEL=OPEN-code:deepseek-v4-flash kz run --readonly` 两步、真实 read 调用、缓存命中 5248。
- 端点按模型分裂(同一个 zen/go/v1,实测存档): deepseek-v4-flash 的 /responses 事件集完整(created / output_item.done 都发)、吃 assistant 历史 → 该走 responses;mimo 系两头都不通(responses 缺 done + assistant 条目 500;chat completions 退化成 XML)→ 不配进来。protocol 是按 provider 配的、不是按模型,真要同时用两类模型得拆成两个 provider 条目。
- 社区侧确认(2026-08-17): 不是我们配错,是**这个端点压根没实现**。anomalyco/opencode#23655(feature request,open、无维护者回复)原文:「The OpenCode Go service currently only supports the `/v1/chat/completions` endpoint (OpenAI Chat Completions API format)」,请求的正是给 `https://opencode.ai/zen/go/v1/responses` 补上 Responses 支持。也就是说 zen/go 这一支只有 chat completions 是正规军,/responses 是个半吊子(deepseek-v4-flash 能跑纯属它那条路径恰好完整)。改协议是唯一正解,不是绕路。
- 待办: 引擎侧目前把这类方言不兼容表达成裸 HTTP 500 + 两次无意义重试。应在 responses 路径识别「带 assistant 历史即 500」这一形态,给出可操作诊断(点名 provider/协议,建议改 openai 协议),而不是让用户从 500 反推。是否再做一个 responses 方言开关(assistant content 降级为纯字符串、跳过 function_call 条目)由用户拍板——改协议已能解决,方言开关只对「必须走 /responses」的场景有价值。
- 验收: ①带 assistant 历史的 responses 请求失败时,错误信息点名 provider/协议并给出改协议的建议,不是裸 500;②该形态的失败不做无意义重试(500 当前会退避重试 2 次);③若实现方言开关,mimo 系在 /responses 下能跑通多轮工具循环。

## D-424 chat completions 流式 tool_call 组装两处丢失:同 index/缺 index 的多条调用被拼成一条(workread),finish_reason 提前收尾丢掉后续参数增量 [fixed] (high)
- 复杂度: 小
- 标签: 模型
- 优先级: P1
- refs: D-422 D-425
- 来源: 用户 2026-08-17 会话 #122581(OPEN-code:mimo-v2.5-pro 走 openai 协议):模型连续两条工具调用全部失败,`unknown tool workread` 与 `Invalid input ... 你的原始输入是 {"action": "claim", "id": `,整轮取活死掉。
- 复现: state.db 里该轮 assistant 消息实证——`{"id":"call_332be4c46d0a4e6d8b1f79a7call_d6d78b1a7a604df0a48329d4","input":{},"name":"workread","type":"tool_call"}`:两条调用的 id 首尾相接、name 拼成 `workread`、arguments 是两段 JSON 相接故解析成空。
- 根因(两处,同在 openai.rs 的 30 行内):
  ①**槽位塌缩**:`tc["index"].as_u64().unwrap_or(0)` —— provider 不发 `index`(或把多条调用都标成同一 index)时全部落 0 号槽,而槽里 id/name/arguments 都是 `push_str` 累加的,于是两条调用被拼成一条不存在的工具。opencode zen 把模型吐的 Hermes XML 二次转 tool_calls 时正是这个形态(见 D-425)。
  ②**提前收尾**:`finish_reason` 一到就 `settle`,`calls_emitted` 置位后,finish_reason 之后还在来的参数增量被永久丢弃 —— 放出去的是 `{"action": "claim", "id": ` 这类切在 chunk 边界上的半截 JSON。原意是兜「服务端不发 [DONE]」,代价是任何「finish_reason 不在最后一帧」的方言都被截断。
- 修复: ①新增 `slot_for`:`index` 是权威键,但已被别的 id 占住的槽不接受新 id(另起一槽);`index` 缺席时带**新** id 的帧开新调用、不带 id 的帧是续帧。整条 id 每帧重发的 provider 不再被接成两遍。②`ProtocolState` 加 `finish()` 流末收尾钩子(默认空实现),client 在 SSE 循环退出后调一次;`finish_reason` 只记原因 + 关闭文本/推理块,工具调用改由 `[DONE]` 或流末 `finish()` 放出。顺带补上了旧路径的一个洞:不发 `[DONE]` 的 provider 此前**永远收不到 StepFinish**。
- 验证: 新增三条定向回归(同槽/缺 index 两条调用不得被拼成一条、缺 index 时无 id 的帧是续帧、finish_reason 之后的参数增量不丢且不发 [DONE] 也能收尾);既有 `incremental_tool_call_assembly` 按新时序更新(ToolCall 落在 [DONE] 帧)。kanzei-llm 51 + kanzei-app 193 + kanzei-core 214 全绿,fmt/clippy 干净。
- 验收: ①同 index / 缺 index 的多条调用各自独立,不再出现拼接工具名;②finish_reason 之后到达的参数增量计入最终调用;③不发 [DONE] 的 provider 能收到完整调用 + StepFinish;④合规 provider(OpenAI/Ollama/DeepSeek chat)行为无回归,既有测试保持绿。
- 备注: 本条只修「引擎把好好的流组装坏了」这一半。另一半(模型压根没发原生 tool_calls,而是把 Hermes XML 写进 content、由网关有损二次转换)是 D-425,不在本条范围。
- 社区侧确认(2026-08-17,槽位塌缩是生态通病而非本仓独有): ollama#15457「tool_calls index is always 0 for multiple tool calls」的描述与本条逐字同构——「When all indices are 0, the second tool call either gets merged into the first or silently dropped, causing 100% failure rate on any task requiring multiple tool calls in one response」,受害方是 Vercel AI SDK 的 @ai-sdk/openai-compatible(同样拿 index 当数组键);ollama#7881 是「OpenAI 兼容接口根本不填 index」;litellm 为此修了两轮(#14587 多调用 index 分配、#15962 流式 n>1 时 index 不填),另有 Bedrock 侧 index 从 1 起算(#32759)与 grok2api#239 缺 index。ollama#15457 里提到的既有绕法是「HTTP proxy that reassigns correct sequential indices based on unique tool call id values」——本条的 slot_for 就是把这件事做进进程内,方向与生态一致。pipecat#4987(id 与 name 分帧到达导致 tool_call_id 为空)也被 slot_for 一并覆盖。
- 社区侧确认(提前收尾/丢参数增量): litellm#20711「Responses API Streaming Drops Tool Call Argument Deltas」是同一类账——累加器键错(遇到 `id: None` 的续帧直接 continue,没有 index 映射),约 90% 的参数增量被静默丢掉,只有首片到达用户。同源病灶:把「哪一帧属于哪条调用」这件事判错,后果一律是半截参数。另见 NVIDIA NIM GLM-5 经 OpenCode 的 OpenAI 兼容端点吐出缺 `}` 的畸形工具 JSON,与本条 `{"action": "claim", "id": ` 同形。

## D-425 mimo-v2.5-pro 在大提示面下退化为 Hermes XML 工具语法写进 content,网关二次转换有损:是否加 XML 打捞垫片待定 [open] (medium)
- 复杂度: 中
- 标签: 模型
- 优先级: P3
- refs: D-424 D-422
- 来源: 2026-08-17 排查 D-424 时读 state.db 发现。
- 复现: 会话 #122581 的 assistant 消息里,text part 是完整的 Hermes/Qwen 工具语法——`<tool_call>\n<function=work>\n<parameter=action>claim</parameter>\n<parameter=id>D-419</parameter>\n<parameter=reason>…</parameter>\n</function>\n</tool_call>`——而同一条消息的 tool_call part 是网关据此二次转换出来的畸形调用(参数截断/多条塌缩)。对照:同一模型在**小**请求下(1~2 个工具、短 system)curl 直连实测发的是干净的原生 tool_calls,`index` 也正常。差异变量是 kanzei 的真实提示面(36 个工具 / tools schema ~37k 字符 / system ~14k + conventions ~13k)。
- 影响: mimo-v2.5-pro 在本 harness 下不可用——一旦退化,工具调用要么名字错要么参数残缺,轮轮报 needs_correction。同网关的 deepseek-v4-flash 不退化(历史 run 里 217/258 次工具调用),是当前可用选择。
- 社区调查结论(2026-08-17,**不做打捞垫片**): 这是被反复讨论过的生态通病,而且结论是一边倒的反对客户端文本打捞。
  · LiveKit《Your Model Isn't Bad at Tool Calling. Your Serving Stack Is.》明确拒绝:框架「deliberately never scrapes tool calls out of text content」,理由是逐个模型家族去 scrape 语法既脆弱又破坏流式;并给出判据「no framework can recover a tool call the server never structured」。正解只有三条,全在服务端:换能正确解析该模型的 provider、自托管并配上对应的 tool-call parser(vLLM/SGLang 都支持按模型配)、或改用模型的原生 API。
  · Roo-Code 走过这条路又退回来了:#11526 直接把 XML 工具调用支持删掉(「XML tool calls are no longer supported」)。
  · 同类报障遍布 lmstudio-bug-tracker#2115、continue#11453 与 discussion#10534、mlx-lm#1096、openclaw#49508——共同点是「serving stack 的 parser 没接上,原生语法漏进 content」,没有一个是靠客户端解析收场的。
  · 我原先担心的误报(本仓散文里天然会出现讨论工具格式的 `<tool_call>`)在这里只是次要理由;主要理由是打捞会把「网关转换有损」这件事永久掩盖掉,而且流式下无法可靠切分。
- 处置: 关闭打捞方案,不实现。mimo 系在本 harness 下判定为不可用,改用同网关的 deepseek-v4-flash(历史 run 217/258 次工具调用,不退化)。
- 旁证(mimo 的工具调用在多个 agent 项目独立踩雷): opencode#24095(mimo-v2.5 调不存在的工具名,closed as not planned)、oh-my-pi#2005(MiMo V2.5 Pro 走 Anthropic 协议时 tool-call 渲染崩溃+无限重试)、opencode#39873(mimo-v2 系整体 Upstream request failed)。
- 验收: ①不实现打捞垫片(本条以 wontfix 收);②模型选择上记住 mimo 系不进自动推进档位。
