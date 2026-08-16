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

## D-392 plot 回退轨失效+假承诺:vega-cli 三重断 SVG 没落盘 [open] (medium)
- refs: R-274
- 影响: 回退轨等于不存在且零测试;假承诺文案把 agent 引去读不存在的 .svg、传被忽略的参数——按「弱模型也能照着走」准绳危害放大。
- 期望: vega-cli 轨删掉或修真;文案与实现对齐(真落 SVG 或删承诺);width/height 实现或删。
- 来源: 2026-08-16 交付质量审计
- 标签: 核心
- 根因: vega-cli 轨三重失效:.cmd shim 检测不到(plot_tool.rs:198-208)+调用缺输出参数(161)+指引与 R-274 自家勘察矛盾(vega-cli 只有 vg2png);「SVG 已落盘供复用」三处文案(5/30-31/185)为假,代码只产 spec JSON+PNG,e2e 用 chart.json 冒充断言(367-368);description 承诺 width/height 但 schema 无、代码不读。
- 优先级: P2

## D-393 latex/plot 路径边界未实施:任意路径可写 [open] (medium)
- refs: R-273 R-274 R-221
- 影响: 配 allow 规则后两工具任意路径裸写;只读档可经 Ask 写盘,档位口径不齐。
- 期望: workdir canonicalize 后限研究工件目录+显式白名单;readonly 档 deny 或同步收窄。
- 来源: 2026-08-16 交付质量审计
- 标签: 核心
- 根因: ctx.cwd.join(workdir)对绝对路径直接替换基底、..不设防、无 canonicalize 无白名单(latex_tool.rs:71、plot_tool.rs:69);R-273/R-274 条目边界「限研究工件目录与显式指定目录」只存在于 schema 描述文本;ReadonlyProfile 硬 deny 了 write/edit/bash 却没管 latex/plot 两个写盘工具(profiles.rs:710-716)。
- 优先级: P2

## D-394 latex 验收测试成色:副本断言/偷换分支/Tectonic 零验证 [open] (medium)
- refs: R-273
- 影响: 验收⑥单测证据无效;回落轨=零安装目标场景可信度为零。
- 期望: Missing/pdftoppm 缺失测试走真生产分支(PATH 操纵);Tectonic 真 exe 至少一次真编译实测留记录;行号测试加 skip guard。
- 来源: 2026-08-16 交付质量审计
- 标签: 核心
- 根因: 「后端缺失给下载指引」断言的是测试内硬编码文案副本,生产 Missing 分支零执行(latex_tool.rs:487-500);「pdftoppm缺失给诊断」实测的是 PDF 不存在分支(556-566),名不副实;Tectonic 真轨用假 .cmd 脚本(0 字节假 PDF)替代(569-607),真 exe 从未编译过真文档(关闭叙述如实记录了替代,诚实但验收字面未满足);「错误诊断含行号」测试无 skip guard,无 TeX 机器假失败。
- 优先级: P2

## D-395 跨树围栏并发误伤:他线窗口内合法自写被回滚 [open] (high)
- refs: R-186 R-268 R-184
- 影响: A 线一条分钟级 cargo build 收口时,B 线并发工作被整体回滚、新建文件被删、误归因到 A(隔离区可捞但 live 工作被破坏)——并行自举的正常形态互相绞杀;叠加 2000 文件上限在 before/after 间成员漂移的误判放大。无测试、无记录覆盖此场景。
- 期望: 跨树面接写日志吸收(B 线自写有凭据即吸收)或按变化 owner 放行;补并行双线真场景测试(A 长 bash 期间 B 写自己树不被回滚)。
- 来源: 2026-08-16 交付质量三路只读审计
- 标签: 核心
- 根因: enforce_other_trees 把 A 线 bash 窗口内 B 树的任何变化判为 A 的越界并回滚(cross_tree.rs:145-284)——并行自举里 B 线在窗口内写自己的树是常态;跨树面没有 R-268 式写日志吸收,也不按变化的实际 owner 判定。
- 优先级: P0

## D-396 跨树快照超限语义混淆:>4MiB 文件被当新建删除 [open] (high)
- refs: R-186
- 影响: 其它线树里 >4MiB 文件(target 产物/资源)被 bash 收口误删。
- 期望: 照搬 managed.rs 三态(存在/超限保持现状/不存在删除);超限至少记 len+mtime 指纹使改动可检出并如实报告。
- 来源: 2026-08-16 交付质量审计
- 标签: 核心
- 根因: FileImage=Option<Vec<u8>>(cross_tree.rs:38)把「执行前不存在」与「超限」都编码为 None;回滚分支(250-263)把超限文件当新建直接删除;超限↔超限改动 None==None 检测不到;注释 32-33 声称「记指纹/能检测/会说明」三点全不成立。对照 managed.rs:157-171 有正确三态区分。
- 优先级: P1

## D-397 跨树 mtime 粗筛未实现:注释假承诺每 bash 双全量读 [open] (medium)
- refs: R-186 D-233
- 影响: 验收④点名的 D-233 反模式复现(比哈希更重);真仓多线+未跟踪 target/node_modules 场景开销未知;截断静默留检测盲区。R-186 关闭证据对粗筛未实现只字未提。
- 期望: 真实现 mtime/len 粗筛(命中再读内容);截断显式报告;补真仓规模实测数字。
- 来源: 2026-08-16 交付质量审计
- 标签: 核心
- 根因: 注释(cross_tree.rs:13-18/184)承诺 mtime 粗筛,实现是每条前台 bash 对每棵其它树两次全文件内容读取+整树驻内存(93-132/155),零 mtime 采集;2000 文件上限静默截断(35),不像 managed 有 truncated 标志拒绝;性能实测仅 5 树×31 小文件玩具规模(73.9ms)。
- 优先级: P2

## D-398 写日志覆盖洞:test_record/conventions/archive 未接线 [open] (high)
- refs: R-268 D-364 D-112
- 影响: 未接线写者失去旧窗口锁保护又无新凭据,与他线 bash 窗口重叠即被收口误回滚;archive 场景=活动侧删除被吸收+归档侧新增被回滚→条目从两个文件同时消失(D-112 级数据丢失,仅隔离区可捞)。
- 期望: 全部专用写者接 write_log(含归档文件);尽快发版消除新旧混跑。
- 来源: 2026-08-16 交付质量审计
- 标签: 核心
- 根因: write_log 只接 tracker 活动文件(tracker.rs:451-470)与 memory 三处;test_record.rs/conventions.rs/architecture.rs 零接入;tracker archive 写活动+归档两个文件却只对活动文件记日志。旁证:主仓 .kanzei/.write-log 目前不存在而 R-268 合入后有大量 tracker 写——生产二进制未含 R-268,新围栏×旧写者混跑期风险真实(发版可消一半)。
- 优先级: P1

## D-399 写日志回滚回窗口开点+prune 死代码+record 吞错 [open] (medium)
- refs: R-268
- 影响: 混合写场景丢合法数据;写日志目录无限膨胀。
- 期望: 回滚用最后合法日志内容;补同路径混合定向测试;prune 接线;record 失败至少告警。
- 来源: 2026-08-16 交付质量审计
- 标签: 核心
- 根因: 收口回滚目标是窗口开点 before(managed.rs:495-496)而非 R-268 条目方向明文的「最后一次合法日志内容」;WriteLogEntry.content 整存全文(write_log.rs:31-33 注释自述用途)却零使用;同路径「先合法写后越界写」场景合法写一并丢——交付的混合测试用两个不同路径绕开(managed.rs:833-885),关闭证据以此核销验收③,降级未记录。prune_before 全仓零调用(write_log.rs:156-174),日志无限增长且每条含全文 hex(2×体积);record 调用点全部 let _= 吞错,与模块自述契约「宁可失败不静默」矛盾。
- 优先级: P2

## D-400 浏览器工具错误通道断裂:click/type 失败报成功 [open] (high)
- refs: R-269 R-272 D-389
- 影响: 交互断言全面假绿——R-272 巡检、R-271 自检等一切消费方的「操作成功」不可信;这是移动链假验收(D-389)的机制成因之一。
- 期望: Rust 侧统查 result.error 并透传为工具错误;click/type 失败必须报错;挂死辅进程有超时兜底;注释与实现对齐。
- 来源: 2026-08-16 交付质量审计
- 标签: 核心
- 根因: 辅进程把所有错误(含 catch)写进 result.error(browser-helper.mjs:171-179),Rust 只查顶层 parsed["error"](browser_tool.rs:167,已当场核验)永远查不到;click/type 无视 result.error 直接报成功(479-483/513-517);open 失败被吞后以「截图缺 png 字段」类误导文案冒出(353-355)。附带:模块注释声称 Drop 收尾但无任何 Drop 实现(14);read_line 阻塞使 60s 超时对挂死辅进程失效(149-171);reaper 被 break 后因 Once 永不重启。
- 优先级: P1

## D-401 R-272 验收降级未记录:静态差集替代浏览器遍历 [open] (medium)
- refs: R-272 R-269
- 影响: 「跳转断裂」(容器在但切换 JS 崩)测不到;巡检对运行时死链盲。
- 期望: 补浏览器遍历批次(依赖 D-400 修复)或改验收口径并在条目诚实记录降级;KEY_PATHS 外置配置文件。
- 来源: 2026-08-16 交付质量审计
- 标签: 流程
- 根因: 交付为纯静态 regex 差集(ui-connectivity.mjs:54-89)+关键路径只查 HTML 存在性(77-89);PWA 4 条路径 3 条 needs_pair 跳过(146-150),唯一真开的是配对页;KEY_PATHS 为脚本内 const(33-51)非验收③要求的配置文件;原案「基于 R-269 从入口遍历+跳转失败/console 报错」运行时判定全部缺席。关闭证据如实描述静态形态但未点名与原案落差,四条验收照单核销(对比 R-264 对做不到的部分明确记「待专用批次」)。
- 优先级: P2

## D-405 主题切换位置不合理:占侧栏整块,建议移到左下角图标与设置同级 [fixing] (low)
- 复现: 当前主题切换是侧栏底部一整块 sidebar-section(index.html:114-119,#theme-section + #theme-toggle「亮色」按钮),低频操作却占用侧栏一个区块;activitybar(左下角 #activitybar)是视图切换图标的常驻区,设置按钮在底部(index.html:35),主题入口与设置层级不对称。
- 影响: 侧栏空间被低频操作占用;主题切换入口位置不直观,与设置同级操作不在同一视觉层级。
- 期望: 移除侧栏 #theme-section;在 #activitybar 底部(设置按钮旁/左下角)加一个主题切换图标按钮,与设置同级;点击切换亮/暗色并沿用 localStorage kz-theme 持久化(03-shell.js applyTheme 既有逻辑可复用,只需改挂载点与图标样式)。
- 来源: 用户消息(2026-08-16)
- 标签: 前端
- 优先级: P2
- 取活依据: override:D-404 已关闭,按用户消息顺序修第二条:主题切换移到左下角 activitybar 与设置同级
- 进展: 关闭证据(2026-08-16,commit 0d79d5b):①index.html:35 #theme-toggle 图标按钮(太阳 #theme-icon-sun/月亮 #theme-icon-moon 双 SVG,class=activity-item)插入 #activitybar 设置按钮前,与设置同级同层级;侧栏 #theme-section 整块移除(原 114-119);②03-shell.js:538-547 applyTheme 按钮更新从 textContent 改图标 hidden 切换+title/aria 保持,主题切换点击逻辑不变(558 行)。验证:T-1786853796 node --check+ui-runtime-smoke 全过(R-189 断言:theme-toggle 存在/不在 statusbar/位于 statusbar 前/点击切换 data-theme 与 localStorage kz-theme 双持久化/Monaco setTheme 联动均绿)。生效依赖:新版 kzapp 构建后运行(当前运行版不含此修复),构建发布走发版 SOP。
- observed_head: 0d79d5b130531e4e938e959bf49020f5ac369ca8
- observed_worktree_hash: fnv1a64:28d67e2167c4069d
- recorded_at: 1786853843829

## D-409 记忆 inbox 消化死亡螺旋:251KB/201 条整箱塞进单轮,失败还静默 [open] (high)
- refs: R-195 R-213 D-341 R-216
- 影响: 记忆控制平面的写入侧实际断流:memory_note 一路写进 inbox 但没有条目被提炼晋升;R-195 今日以「candidate 晋升与清退闭环完成」归档,闭环的是 candidate 生命周期,inbox→entry 这一段并未打通,用户直观看到 201 条待确认。
- 期望: ①分批消化:每轮取固定条数(建议 10~20)喂 manager,逐条 memory_inbox_discard 销账,剩余留待下轮;②失败可见:run 失败/未销账时记事件+轮末诊断,连续失败 N 轮升级为通知,不再静默;③积压护栏:pending 超阈值(如 100)时前端与轮末明确告警并给「一键整理」入口(UI 已有该按钮,需接到分批消化上);④存量 201 条按新链路清空,给实测数字。
- 来源: 2026-08-16 用户在桌面端看到「待确认候选 201」并指出记忆晋升未解决,当场取证:inbox.md 251612 字节/201 条。
- 标签: 核心
- 根因: ①无分批:consolidation_prompt(kanzei-memory/src/memory/manager.rs:1092)把整个 inbox 原样拼进 prompt——现已 251612 字节/201 条,单轮 max_tokens 仅 4096、steps 10,模型既读不完也逐条销不完账;②失败静默:consolidate_memory_inbox(kanzei-app/src/memory.rs:374)`let _ = run_once_with_parts(...)` 丢弃全部错误,primary/fast 两档都失败时无任何诊断、无事件、无通知,轮末照常「成功」;③无上限反馈:inbox 只增不减,越大越难消化、越难消化越大——用户端表现为「待确认候选 201」持续堆积,记忆晋升事实停摆。
- 优先级: P1

## D-412 研究文献侧仅读摘要却标 V2 一手来源:CoALA 分类学归因不成立 [open] (medium)
- refs: R-221 R-277
- 影响: V 表的可信度被稀释:V2 语义是「一手来源(论文原文/官方文档/仓库源码)」,摘要级证据混入 V2 后,读者无法分辨哪些结论经得起正文核验。本轮 12 篇文献里绝大多数结论确实落在摘要覆盖范围内(已抽查 Zep 94.8/93.4/18.5、Mem0 91%/90%、A-MEM NeurIPS 2025、Generative Agents 消融 均属实),问题不在幻觉而在**方法论披露缺失**与个别越界。
- 期望: ①V 表文献域补「摘要级」与「正文级」的区分(或规定摘要级封顶 V1),写进 conventions 时一并定(R-221 批3);②R-277 引擎的验收④「FACT 式论断-出处逐条核验」应把「该出处是否真含支撑文本」做成机械抽查,本次即为反例样本;③本报告 report.md:31 的 CoALA 归因改为取正文核验或降级标注。
- 来源: 2026-08-16 用户要求评估本轮 research 质量,机械核验 18 个 file:line 锚(全中)+9 个 arXiv ID(全真)+数值断言(全实)后,唯一抽出的实质问题。
- 标签: 流程
- 根因: 本轮 research 的文献检索通道是 arXiv API,拿到的只有 title+summary(摘要),全程未取正文。报告把这类来源一律标 V2「一手来源」,且未声明「仅摘要级」。抽查发现一处实质越界:report.md:31 称 CoALA(arXiv 2309.02427)确立「working/episodic/semantic/procedural」四类模块化记忆并标 V2/S-008,但实测该论文摘要里 working/episodic/semantic/procedural 四词一个都没有(只有 memory)——结论本身是对的(在正文里),但**引用的那份证据支撑不了它**。同一段落对 LangGraph(S-009,取的是正文 HTML)的三类映射则证据充分。
- 优先级: P2

## D-413 研究工件前端只读:文献打不开、条目改不了删不掉,后端全支持 [open] (high)
- refs: R-276 R-221
- 影响: ①来源里明明存了 URL 字段(.kanzei/research/sources.md 每条文献均有 `- URL: https://arxiv.org/abs/...`)却无法点击打开,用户原话「我想直接打开他参考的文献也不行」;②代码域来源的 `证据锚: file:line` 同样点不开;③条目无法编辑、无法删除,写错只能手改 markdown;④标题被 CSS 截断,来源列表 19 条全是「kanzei 检索/触发/反事实评估实现(index...」这类看不全的字符串;⑤发现条目 confirmed 后整条置灰,像是被禁用。研究模式的核心资产(来源与发现)在 UI 里事实上是死的。
- 期望: ①去掉 kind gating,source/finding 与 req/defect 同权:可展开、可编辑字段、可删除、可归档;②来源条目主操作=打开——文献用 URL、代码域用证据锚跳文件定位行;③标题不截断(卡片换行或悬停全文);④refs 里的 S-id 可点跳转;⑤confirmed 不等于失效,置灰样式要区分「终态」与「不可用」。
- 来源: 2026-08-16 用户在 research 首轮实测后逐条指出:文献打不开、条目删不掉、打开也没法编辑。
- 标签: 前端
- 根因: renderDocList 把展开/字段编辑/删除/归档等交互整体 gate 在 kind==="req"||"defect"(crates/kanzei-app/ui/11-docs-list.js:249-262),source/finding 走同一函数但只落到「一行截断标题」的裸渲染(12-docs-pages.js:810-811);而后端 docs_update 对 kind=source/finding 早已全支持 update/close/archive(crates/kanzei-app/src/docs.rs:402-413)。纯前端接线缺失。
- 优先级: P1
